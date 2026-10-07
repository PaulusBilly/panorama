import { normalizeSubtitleAppearance, updateSubtitleAppearance } from "./subtitle-appearance";
import { discoverService } from "./service-discovery";
import { createCoreTransport, type CoreTransport } from "./core-transport";
import {
  embeddedSubtitleSourceLabel,
  extraSubtitleSourceLabel,
  extraSubtitleTracksFromCore,
  extraSubtitleTracksFromStream,
  isCurrentFilmRequest,
  normalizeResumeState,
  normalizeWatchlisted,
  clampSubtitleOffset,
  clampSubtitleSize,
  clampSubtitleVerticalPosition,
  normalizeSourceGroups,
  onlyAioStreamsSourceGroups,
  preferredEnglishAudioTrack,
  preferredEnglishSubtitleTrack,
  uniqueExtraSubtitleTracks,
  type ExtraSubtitleTrack,
} from "./normalize";
import {
  createVideoEngine,
  loadVideoEngineConstructor,
  NATIVE_ADDON_SUBTITLE_TITLE_PREFIX,
  type VideoEngine,
  type VideoEngineSession,
} from "./video-engine";
import {
  DEFAULT_SERVICE_ENDPOINT,
  serviceEndpointCandidates,
  initialRuntimeSnapshot,
} from "./snapshot";
import {
  normalizeNativePlaybackDuration,
  resolveInitialPlaybackDuration,
} from "./playback-state";
import {
  createRuntimeSnapshotWithSubtitlePreferences,
  hasStoredSubtitlePreferences,
  writeSubtitlePreferences,
} from "./subtitle-preferences";
import {
  enrichTmdbCatalogFilms,
  fetchTmdbDirectedMovies,
  fetchTmdbMovieDetails,
  fetchTmdbPopularMovies,
  fetchTmdbSearchMovies,
  fetchTmdbSearchPeople,
  NO_IMDB_SOURCES_MESSAGE,
  parseTmdbPublicId,
} from "./tmdb";
import type {
  PanoramaFilm,
  PanoramaFilmDetails,
  PanoramaSubtitleStyle,
  PlaybackStartMode,
  RuntimeListener,
  RuntimeSnapshot,
  ServiceStatus,
  StremioRuntime,
} from "./types";

export const PLAYBACK_PREPARATION_TIMEOUT_MS = 6 * 60_000;
const PREPARATION_SERVICE_POLL_MS = 3_000;
const PREPARATION_SERVICE_FAILURE_LIMIT = 3;
const PROGRESS_SYNC_INTERVAL_SECONDS = 10;
const SEEK_ACK_TIMEOUT_MS = 3_000;
// The selected source's link and opening are fetched once the details page has
// been open this long, so Play starts from data already on hand.
const SOURCE_WARM_DELAY_MS = 1_500;
// A source whose server refused its warm-up is flagged for this long.
const UNREACHABLE_SOURCE_MS = 10 * 60_000;
const UNREACHABLE_SOURCE_REASON = "The server for this source is not responding right now.";
const UNREACHABLE_PLAYBACK_ERROR = "The server for this source is not responding. Choose another source or try again later.";

// Only direct links an addon marks as instantly available are warmed. Asking a
// debrid service for an uncached item can start a download on the account.
export function warmableSourceUrl(stream: unknown): string | null {
  if (!stream || typeof stream !== "object") return null;
  const { url, name, title, description } = stream as Record<string, unknown>;
  if (typeof url !== "string" || !/^https?:\/\//i.test(url)) return null;
  const label = [name, title, description].filter((value) => typeof value === "string").join(" ");
  return label.includes("⚡") && !/[⏳⌛]/u.test(label) ? url : null;
}
const SEEK_ACK_TOLERANCE_SECONDS = 2;
const SERVICE_HEALTH_PATH = "/stats.json";
export const SERVICE_HEALTH_FRESHNESS_MS = 15_000;

function serviceHealthUrl(endpoint: string): string {
  return `${endpoint.replace(/\/$/, "")}${SERVICE_HEALTH_PATH}`;
}

function toFilmDetails(film: PanoramaFilm): PanoramaFilmDetails {
  if ("runtime" in film && "genres" in film && "logoUrl" in film) {
    const details = film as PanoramaFilmDetails;
    return {
      ...details,
      originalTitle: details.originalTitle ?? null,
      alternativeTitle: details.alternativeTitle ?? null,
      runtime: details.runtime ?? null,
      genres: details.genres ?? [],
      logoUrl: details.logoUrl ?? null,
    };
  }
  return { ...film, originalTitle: null, alternativeTitle: null, runtime: null, genres: [], logoUrl: null };
}

type CoreContextState = {
  profile?: {
    auth?: {
      user?: {
        email?: string;
      };
    } | null;
    settings?: {
      streamingServerUrl?: string;
      subtitlesSize?: number;
      subtitlesTextColor?: string;
      subtitlesBackgroundColor?: string;
      subtitlesOutlineColor?: string;
    };
  };
};

type InstalledAddonsState = {
  catalog?: unknown[];
};

type InstalledAddonEntry = {
  transportUrl?: unknown;
  manifest?: { id?: unknown; name?: unknown };
  addon?: {
    transportUrl?: unknown;
    manifest?: { id?: unknown; name?: unknown };
  };
};

type CoreMetaDetailsState = {
  libraryItem?: unknown;
  metaItem?: {
    addon?: { transportUrl?: string };
    content?: {
      type?: "Loading" | "Ready" | "Err";
      content?: { id: string; inLibrary?: boolean; [key: string]: unknown };
    };
  } | null;
  streams?: Array<{
    addon?: { transportUrl?: string };
    content?: { type?: "Loading" | "Ready" | "Err"; content?: unknown };
  }>;
  selected?: {
    metaPath?: { id?: string };
  } | null;
};

type CorePlayerState = {
  libraryItem?: {
    state?: { timeOffset?: number };
  } | null;
  selected?: {
    stream?: unknown;
  } | null;
  stream?: {
    type?: "Loading" | "Ready" | "Err";
    content?: unknown;
  } | null;
  subtitles?: unknown;
};

type ResourcePath = {
  resource: "meta" | "stream" | "subtitles";
  type: "movie";
  id: string;
  extra: never[];
};

type PrivatePlaybackTarget = {
  stream: unknown;
  streamTransportUrl: string;
  metaTransportUrl: string;
  streamPath: ResourcePath;
  metaPath: ResourcePath;
  subtitlesPath: ResourcePath;
  addonName: string;
};

export function isFreshServiceHealth(
  status: ServiceStatus,
  endpoint: string,
  checkedEndpoint: string | null,
  checkedAt: number,
  now: number,
): boolean {
  return status === "online" &&
    endpoint === checkedEndpoint &&
    now - checkedAt < SERVICE_HEALTH_FRESHNESS_MS;
}

type VideoTrack = {
  id?: unknown;
  label?: unknown;
  lang?: unknown;
  origin?: unknown;
};

function optionalAddonString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function mapVideoTrack(track: VideoTrack, index: number): {
  label: string;
  language: string | null;
  description: string | null;
} {
  const language = typeof track.lang === "string" && track.lang.trim() ? track.lang.trim() : null;
  const rawLabel = typeof track.label === "string" && track.label.trim() && !/^https?:/i.test(track.label.trim())
    ? track.label.trim()
    : null;
  let displayedLanguage = language;
  if (language && language.length <= 3) {
    try {
      displayedLanguage = new Intl.DisplayNames([language, "en"], { type: "language" }).of(language) ?? language;
    } catch {
      displayedLanguage = language;
    }
  }
  const label = displayedLanguage || rawLabel || `Track ${index + 1}`;
  const description = rawLabel && rawLabel !== label ? rawLabel : null;
  return { label, language, description };
}

type CoreEnvelope = {
  name: "NewState" | "CoreEvent";
  args: unknown;
};

export class StremioCoreRuntime implements StremioRuntime {
  private snapshot: RuntimeSnapshot = createRuntimeSnapshotWithSubtitlePreferences();
  private listeners = new Set<RuntimeListener>();
  private transport: CoreTransport | null = null;
  private worker: Worker | null = null;
  private initialized = false;
  private tmdbByPublicId = new Map<string, number>();
  private imdbByPublicId = new Map<string, string>();
  private detailRequest = 0;
  private watchlistRequest = 0;
  private catalogRequest = 0;
  private privateSources = new Map<string, PrivatePlaybackTarget>();
  private playerGeneration = 0;
  private video: VideoEngine | null = null;
  private videoSession: VideoEngineSession | null = null;
  private videoSessionPromise: Promise<void> | null = null;
  private videoContainer: HTMLElement | null = null;
  private fullscreenTarget: HTMLElement | null = null;
  private loadedPlayerGeneration = 0;
  private playerTarget: PrivatePlaybackTarget | null = null;
  private subtitleTargets = new Map<string, { origin: "embedded" | "addon"; engineId: string }>();
  private audioTargets = new Map<string, string>();
  private addonNamesByTransport = new Map<string, string>();
  private addonCatalogSignature: string | null = null;
  private addonSyncPromise: Promise<void> | null = null;
  private pendingExtraSubtitles: ExtraSubtitleTrack[] = [];
  private autoSelectEnglishAudio = true;
  private autoSelectEnglishSubtitle = true;
  private observedVideoProps: string[] = [];
  private preparationTimeout: ReturnType<typeof setTimeout> | null = null;
  private sourceWarmTimer: ReturnType<typeof setTimeout> | null = null;
  private unreachableSourceUrls = new Map<string, number>();
  private preparationServicePoll: ReturnType<typeof setInterval> | null = null;
  private preparationServiceFailures = 0;
  private preparedPlayback: {
    filmId: string;
    sourceId: string;
    target: PrivatePlaybackTarget;
    ready: Promise<boolean>;
  } | null = null;
  private lastServiceOnlineAt = 0;
  private lastServiceOnlineEndpoint: string | null = null;
  private lastProgressSyncTime = 0;
  private videoDevice = "HTMLVideo";
  private lastNonzeroPlaybackVolume = 1;
  private pendingSeek: { target: number; expiresAt: number } | null = null;
  private preparingTimeSample: number | null = null;
  private probedPlaybackDuration: number | null = null;

  getSnapshot = (): RuntimeSnapshot => this.snapshot;

  subscribe = (listener: RuntimeListener): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  initialize = async (): Promise<void> => {
    if (this.initialized) return;
    this.initialized = true;
    this.patch({ runtime: { status: "initializing", error: null } });

    try {
      const { transport, worker } = createCoreTransport(this.handleCoreEvent);
      this.transport = transport;
      this.worker = worker;
      await transport.init({ appVersion: "0.1.0", shellVersion: null });
      this.patch({ runtime: { status: "ready", error: null } });
      await Promise.all([this.refreshContext(), this.loadPopularMovies(), this.checkService()]);
    } catch (error) {
      this.patch({
        runtime: {
          status: "error",
          error: this.message(error, "Stremio core could not start."),
        },
      });
    }
  };

  login = async (email: string, password: string): Promise<void> => {
    if (!this.transport) throw new Error("Stremio core is not ready.");
    this.patch({
      account: { status: "authenticating", email: null, error: null },
    });
    await this.transport.dispatch({
      action: "Ctx",
      args: {
        action: "Authenticate",
        args: { type: "Login", email, password },
      },
    });
  };

  logout = async (): Promise<void> => {
    if (!this.transport) return;
    await this.transport.dispatch({
      action: "Ctx",
      args: { action: "Logout" },
    });
  };

  loadPopularMovies = async (): Promise<void> => {
    await this.loadCatalog("popular", null);
  };

  searchMovies = async (query: string): Promise<void> => {
    const normalizedQuery = query.trim();
    if (!normalizedQuery) {
      await this.clearSearch();
      return;
    }
    const request = ++this.catalogRequest;
    this.patch({
      catalog: {
        ...this.snapshot.catalog,
        mode: "search",
        query: normalizedQuery,
        requestId: request,
        status: "loading",
        page: { items: [], nextSkip: 1, hasMore: false },
        loadingMore: false,
        error: null,
      },
      people: {
        query: normalizedQuery,
        requestId: request,
        status: "loading",
        page: { items: [], nextSkip: 1, hasMore: false },
        loadingMore: false,
        error: null,
      },
    });

    const peoplePage = fetchTmdbSearchPeople(normalizedQuery, 1);
    await Promise.allSettled([
      this.loadMovieSearch(normalizedQuery, request, peoplePage),
      this.loadPeopleSearch(normalizedQuery, request, peoplePage),
    ]);
  };

  retryMovieSearch = async (): Promise<void> => {
    const query = this.snapshot.catalog.query;
    if (this.snapshot.catalog.mode !== "search" || !query) return;
    const request = this.catalogRequest;
    this.patch({
      catalog: {
        ...this.snapshot.catalog,
        status: "loading",
        page: { items: [], nextSkip: 1, hasMore: false },
        loadingMore: false,
        error: null,
      },
    });
    await this.loadMovieSearch(query, request);
  };

  retryPeopleSearch = async (): Promise<void> => {
    const query = this.snapshot.people.query;
    if (this.snapshot.catalog.mode !== "search" || !query) return;
    const request = this.catalogRequest;
    this.patch({
      people: {
        ...this.snapshot.people,
        status: "loading",
        page: { items: [], nextSkip: 1, hasMore: false },
        loadingMore: false,
        error: null,
      },
    });
    await this.loadPeopleSearch(query, request);
  };

  clearSearch = async (): Promise<void> => {
    await this.loadCatalog("popular", null);
  };

  private loadMovieSearch = async (
    query: string,
    request: number,
    peoplePage = fetchTmdbSearchPeople(query, 1),
  ): Promise<void> => {
    try {
      const [page, directedItems] = await Promise.all([
        fetchTmdbSearchMovies(query, 1),
        peoplePage
          .then((people) => fetchTmdbDirectedMovies(query, people.items))
          .catch(() => []),
      ]);
      if (request !== this.catalogRequest) return;
      const seen = new Set(directedItems.map((film) => film.id));
      const items = await enrichTmdbCatalogFilms([
        ...directedItems,
        ...page.items.filter((film) => !seen.has(film.id)),
      ]);
      if (request !== this.catalogRequest) return;
      this.rememberTmdbIds(items);
      this.patch({
        catalog: {
          ...this.snapshot.catalog,
          status: "ready",
          page: {
            items,
            nextSkip: page.page + 1,
            hasMore: page.page < page.totalPages,
          },
          loadingMore: false,
          error: null,
        },
      });
    } catch (error) {
      if (request !== this.catalogRequest) return;
      this.patch({
        catalog: {
          ...this.snapshot.catalog,
          status: "error",
          loadingMore: false,
          error: this.message(error, "Search results could not be loaded."),
        },
      });
    }
  };

  private loadPeopleSearch = async (
    query: string,
    request: number,
    peoplePage = fetchTmdbSearchPeople(query, 1),
  ): Promise<void> => {
    try {
      const page = await peoplePage;
      if (request !== this.catalogRequest) return;
      this.patch({
        people: {
          ...this.snapshot.people,
          status: "ready",
          page: {
            items: page.items,
            nextSkip: page.page + 1,
            hasMore: page.page < page.totalPages,
          },
          loadingMore: false,
          error: null,
        },
      });
    } catch (error) {
      if (request !== this.catalogRequest) return;
      this.patch({
        people: {
          ...this.snapshot.people,
          status: "error",
          loadingMore: false,
          error: this.message(error, "Cast and crew results could not be loaded."),
        },
      });
    }
  };

  private loadCatalog = async (mode: "popular" | "search", query: string | null): Promise<void> => {
    const request = ++this.catalogRequest;
    this.patch({
      catalog: {
        ...this.snapshot.catalog,
        mode,
        query,
        requestId: request,
        status: "loading",
        page: { items: [], nextSkip: 1, hasMore: false },
        loadingMore: false,
        error: null,
      },
      people: {
        query: null,
        requestId: request,
        status: "idle",
        page: { items: [], nextSkip: 0, hasMore: false },
        loadingMore: false,
        error: null,
      },
    });

    try {
      const page = mode === "search" && query
        ? await fetchTmdbSearchMovies(query, 1)
        : await fetchTmdbPopularMovies(1);
      if (request !== this.catalogRequest) return;
      const items = await enrichTmdbCatalogFilms(page.items);
      if (request !== this.catalogRequest) return;
      this.rememberTmdbIds(items);
      this.patch({
        catalog: {
          ...this.snapshot.catalog,
          status: "ready",
          page: {
            items,
            nextSkip: page.page + 1,
            hasMore: page.page < page.totalPages,
          },
          loadingMore: false,
          error: null,
        },
      });
    } catch (error) {
      if (request !== this.catalogRequest) return;
      this.patch({
        catalog: {
          ...this.snapshot.catalog,
          status: "error",
          loadingMore: false,
          error: this.message(error, mode === "search" ? "Search results could not be loaded." : "Popular films could not be loaded."),
        },
      });
    }
  };

  loadNextPage = async (): Promise<void> => {
    if (this.snapshot.catalog.loadingMore || !this.snapshot.catalog.page.hasMore) {
      return;
    }

    const request = this.catalogRequest;
    const nextPage = this.snapshot.catalog.page.nextSkip;
    this.patch({
      catalog: { ...this.snapshot.catalog, loadingMore: true, error: null },
    });

    try {
      const page = this.snapshot.catalog.mode === "search" && this.snapshot.catalog.query
        ? await fetchTmdbSearchMovies(this.snapshot.catalog.query, nextPage)
        : await fetchTmdbPopularMovies(nextPage);
      if (request !== this.catalogRequest) return;
      const nextItems = await enrichTmdbCatalogFilms(page.items);
      if (request !== this.catalogRequest) return;
      this.rememberTmdbIds(nextItems);
      const seen = new Set(this.snapshot.catalog.page.items.map((film) => film.id));
      const items = [
        ...this.snapshot.catalog.page.items,
        ...nextItems.filter((film) => !seen.has(film.id)),
      ];
      this.patch({
        catalog: {
          ...this.snapshot.catalog,
          status: "ready",
          page: {
            items,
            nextSkip: page.page + 1,
            hasMore: page.page < page.totalPages,
          },
          loadingMore: false,
          error: null,
        },
      });
    } catch (error) {
      if (request !== this.catalogRequest) return;
      this.patch({
        catalog: {
          ...this.snapshot.catalog,
          loadingMore: false,
          error: this.message(
            error,
            this.snapshot.catalog.mode === "search"
              ? "Search results could not be loaded."
              : "Popular films could not be loaded.",
          ),
        },
      });
    }
  };

  loadNextPeoplePage = async (): Promise<void> => {
    if (
      this.snapshot.catalog.mode !== "search" ||
      !this.snapshot.people.query ||
      this.snapshot.people.loadingMore ||
      !this.snapshot.people.page.hasMore
    ) {
      return;
    }

    const request = this.catalogRequest;
    const nextPage = this.snapshot.people.page.nextSkip;
    this.patch({
      people: { ...this.snapshot.people, loadingMore: true, error: null },
    });

    try {
      const page = await fetchTmdbSearchPeople(this.snapshot.people.query, nextPage);
      if (request !== this.catalogRequest) return;
      const seen = new Set(this.snapshot.people.page.items.map((person) => person.id));
      const items = [
        ...this.snapshot.people.page.items,
        ...page.items.filter((person) => !seen.has(person.id)),
      ];
      this.patch({
        people: {
          ...this.snapshot.people,
          status: "ready",
          page: {
            items,
            nextSkip: page.page + 1,
            hasMore: page.page < page.totalPages,
          },
          loadingMore: false,
          error: null,
        },
      });
    } catch (error) {
      if (request !== this.catalogRequest) return;
      this.patch({
        people: {
          ...this.snapshot.people,
          loadingMore: false,
          error: this.message(error, "Cast and crew results could not be loaded."),
        },
      });
    }
  };

  openFilmDetails = async (filmId: string): Promise<void> => {
    const request = ++this.detailRequest;
    const fallback = this.snapshot.catalog.page.items.find((film) => film.id === filmId);
    this.privateSources.clear();
    this.preparedPlayback = null;
    this.patch({
      details: {
        filmId,
        resume: { available: false, offset: 0, duration: null },
        watchlist: { available: false, saved: false, pending: false },
        metadata: {
          status: "loading",
          item: fallback ? toFilmDetails(fallback) : null,
          error: null,
        },
        sources: {
          status: this.canLoadSources() ? "loading" : "idle",
          groups: [],
          error: null,
        },
      },
    });

    try {
      const tmdbId = parseTmdbPublicId(filmId) ?? this.tmdbByPublicId.get(filmId);
      if (!tmdbId) throw new Error("Unable to load film details. Check your connection and try again.");
      const details = await fetchTmdbMovieDetails(tmdbId);
      if (request !== this.detailRequest) return;

      this.tmdbByPublicId.set(filmId, details.tmdbId);
      if (details.imdbId) this.imdbByPublicId.set(filmId, details.imdbId);
      else this.imdbByPublicId.delete(filmId);

      const item: PanoramaFilmDetails = {
        ...toFilmDetails(details),
        id: filmId,
      };
      const imdbId = details.imdbId;
      this.patch({
        details: {
          ...this.snapshot.details,
          metadata: { status: "ready", item, error: null },
          sources: !this.canLoadSources()
            ? { status: "idle", groups: [], error: null }
            : imdbId
              ? { status: "loading", groups: [], error: null }
              : { status: "ready", groups: [], error: NO_IMDB_SOURCES_MESSAGE },
        },
      });

      if (!imdbId || !this.canLoadSources() || !this.transport) return;

      await this.transport.dispatch(
        {
          action: "Load",
          args: {
            model: "MetaDetails",
            args: {
              metaPath: { resource: "meta", type: "movie", id: imdbId, extra: [] },
              streamPath: { resource: "stream", type: "movie", id: imdbId, extra: [] },
              guessStream: true,
            },
          },
        },
        "meta_details",
      );
      if (request === this.detailRequest) await this.refreshFilmDetails(request);
    } catch {
      if (request !== this.detailRequest) return;
      this.patch({
        details: {
          ...this.snapshot.details,
          metadata: {
            ...this.snapshot.details.metadata,
            status: "error",
            error: "Unable to load film details. Check your connection and try again.",
          },
          sources: this.canLoadSources()
            ? { status: "error", groups: [], error: "Unable to load sources." }
            : this.snapshot.details.sources,
        },
      });
    }
  };

  setWatchlisted = async (saved: boolean): Promise<void> => {
    const transport = this.transport;
    const previous = this.snapshot.details.watchlist;
    if (!transport || !previous.available || previous.pending || saved === previous.saved) return;
    const request = this.detailRequest;
    const mutation = ++this.watchlistRequest;
    const current = () => request === this.detailRequest && mutation === this.watchlistRequest;
    this.patch({
      details: { ...this.snapshot.details, watchlist: { available: true, saved, pending: true } },
    });
    try {
      const state = await transport.getState<CoreMetaDetailsState>("meta_details");
      if (!current()) return;
      const meta = state.metaItem?.content;
      const filmId = this.snapshot.details.filmId;
      const imdbId = filmId ? this.imdbByPublicId.get(filmId) : null;
      if (meta?.type !== "Ready" || !meta.content || meta.content.id !== imdbId) {
        throw new Error("Film metadata is not ready.");
      }
      await transport.dispatch({
        action: "Ctx",
        args: saved
          ? { action: "AddToLibrary", args: meta.content }
          : { action: "RemoveFromLibrary", args: meta.content.id },
      });
      if (!current()) return;
      this.patch({
        details: {
          ...this.snapshot.details,
          watchlist: { ...this.snapshot.details.watchlist, pending: false },
        },
      });
    } catch {
      if (!current()) return;
      this.patch({
        details: {
          ...this.snapshot.details,
          watchlist: { ...this.snapshot.details.watchlist, saved: previous.saved, pending: false },
        },
      });
    }
  };

  retryFilmDetails = async (): Promise<void> => {
    const filmId = this.snapshot.details.filmId;
    if (filmId) await this.openFilmDetails(filmId);
  };

  closeFilmDetails = async (): Promise<void> => {
    const request = ++this.detailRequest;
    await this.stopPlayback();
    if (request !== this.detailRequest) return;
    this.privateSources.clear();
    if (this.transport) {
      await this.transport.dispatch({ action: "Unload" }, "meta_details");
    }
    if (request !== this.detailRequest) return;
    this.patch({ details: structuredClone(initialRuntimeSnapshot.details) });
  };

  startPlayback = async (sourceId: string, mode: PlaybackStartMode = "restart"): Promise<void> => {
    const startTime = mode === "resume" && this.snapshot.details.resume.available
      ? this.snapshot.details.resume.offset
      : 0;
    await this.beginPlayback(sourceId, startTime);
  };

  preparePlayback = async (sourceId: string): Promise<void> => {
    if (!this.transport) return;
    const target = this.privateSources.get(sourceId);
    const filmId = this.snapshot.details.filmId;
    if (!target || !filmId) return;

    void loadVideoEngineConstructor().catch(() => undefined);
    this.scheduleSourceWarm(sourceId, filmId, target);
    const existing = this.preparedPlayback;
    if (existing?.sourceId === sourceId && existing.filmId === filmId && existing.target === target) {
      await existing.ready;
      return;
    }

    const ready = this.loadCorePlayer(target).then(
      () => true,
      () => false,
    );
    this.preparedPlayback = { filmId, sourceId, target, ready };
    await ready;
  };

  switchPlaybackSource = async (sourceId: string): Promise<void> => {
    this.flushPlaybackProgress();
    await this.beginPlayback(sourceId, this.snapshot.player.time);
  };

  private beginPlayback = async (sourceId: string, startTime: number): Promise<void> => {
    if (!this.transport) return;
    const target = this.privateSources.get(sourceId);
    const filmId = this.snapshot.details.filmId;
    if (!target || !filmId) return;

    window.panoramaDesktop?.recordPlaybackTiming?.("play-requested");

    const continuingSession = this.snapshot.player.status !== "idle" && this.snapshot.player.filmId === filmId;
    const sessionVolume = continuingSession ? this.snapshot.player.volume : 1;
    const sessionMuted = continuingSession ? this.snapshot.player.muted : false;
    if (!continuingSession) this.lastNonzeroPlaybackVolume = 1;
    else if (sessionVolume > 0) this.lastNonzeroPlaybackVolume = sessionVolume;

    const generation = ++this.playerGeneration;
    this.videoSessionPromise = null;
    this.playerTarget = target;
    this.loadedPlayerGeneration = 0;
    this.pendingSeek = null;
    this.preparingTimeSample = startTime;
    this.probedPlaybackDuration = null;
    this.lastProgressSyncTime = startTime;
    this.subtitleTargets.clear();
    this.audioTargets.clear();
    this.pendingExtraSubtitles = [];
    this.autoSelectEnglishAudio = true;
    this.autoSelectEnglishSubtitle = true;
    this.videoSession?.destroy();
    this.videoSession = null;
    this.video = null;
    const currentDuration = this.snapshot.player.filmId === filmId
      ? this.snapshot.player.duration
      : 0;
    const duration = resolveInitialPlaybackDuration(
      currentDuration,
      this.snapshot.details.resume.duration,
    );
    const previousSubtitles = this.snapshot.player.subtitles;
    this.patch({
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "preparing",
        stage: "checkingService",
        filmId,
        sourceId,
        title: this.snapshot.details.metadata.item?.name ?? null,
        time: startTime,
        duration,
        volume: sessionVolume,
        muted: sessionMuted,
        subtitles: {
          ...structuredClone(initialRuntimeSnapshot.player.subtitles),
          offset: previousSubtitles.offset,
          verticalPosition: previousSubtitles.verticalPosition,
          style: previousSubtitles.style,
        },
      },
    });

    this.startPreparationMonitor(generation);
    const cachedPreparation = this.preparedPlayback?.sourceId === sourceId &&
      this.preparedPlayback.filmId === filmId &&
      this.preparedPlayback.target === target
      ? this.preparedPlayback.ready
      : null;
    if (cachedPreparation) this.preparedPlayback = null;
    const coreReady = cachedPreparation
      ? cachedPreparation.then((ready) => ready ? true : this.loadCorePlayer(target).then(() => true))
      : this.loadCorePlayer(target).then(() => true);
    const endpoint = this.snapshot.service.endpoint;
    const serviceReady = isFreshServiceHealth(
      this.snapshot.service.status,
      endpoint,
      this.lastServiceOnlineEndpoint,
      this.lastServiceOnlineAt,
      Date.now(),
    )
      ? Promise.resolve<ServiceStatus>("online")
      : this.checkService();
    void serviceReady.then((status) => {
      if (
        status === "online" &&
        generation === this.playerGeneration &&
        this.snapshot.player.stage === "checkingService"
      ) {
        this.patch({ player: { ...this.snapshot.player, stage: "resolvingSource" } });
      }
    });
    try {
      const [serviceStatus] = await Promise.all([
        serviceReady,
        coreReady,
        loadVideoEngineConstructor(),
      ]);
      if (generation !== this.playerGeneration || serviceStatus !== "online") return;
      window.panoramaDesktop?.recordPlaybackTiming?.("preparation-ready");
    } catch {
      if (generation === this.playerGeneration) this.patchPlayerError();
      return;
    }
    if (generation === this.playerGeneration && this.videoContainer) {
      await this.attachPlayer(this.videoContainer);
    } else if (generation === this.playerGeneration) {
      await this.refreshPlayer(generation);
    }
  };

  private loadCorePlayer = async (target: PrivatePlaybackTarget): Promise<void> => {
    if (!this.transport) throw new Error("Stremio core is not ready.");
    await this.transport.dispatch(
      {
        action: "Load",
        args: {
          model: "Player",
          args: {
            stream: target.stream,
            streamRequest: { base: target.streamTransportUrl, path: target.streamPath },
            metaRequest: { base: target.metaTransportUrl, path: target.metaPath },
            subtitlesPath: target.subtitlesPath,
          },
        },
      },
      "player",
    );
  };

  retryPlayback = async (): Promise<void> => {
    const sourceId = this.snapshot.player.sourceId;
    if (sourceId) await this.beginPlayback(sourceId, this.snapshot.player.time);
  };

  stopPlayback = async (): Promise<void> => {
    this.flushPlaybackProgress();
    ++this.playerGeneration;
    this.videoSessionPromise = null;
    this.clearPreparationMonitor();
    if (this.fullscreenTarget && document.fullscreenElement === this.fullscreenTarget) {
      try {
        await document.exitFullscreen();
      } catch {}
    }
    this.clearFullscreenTarget();
    this.loadedPlayerGeneration = 0;
    this.playerTarget = null;
    this.preparedPlayback = null;
    this.subtitleTargets.clear();
    this.audioTargets.clear();
    this.pendingExtraSubtitles = [];
    this.lastNonzeroPlaybackVolume = 1;
    this.pendingSeek = null;
    this.probedPlaybackDuration = null;
    this.videoSession?.destroy();
    this.videoSession = null;
    this.video = null;
    this.videoContainer = null;
    this.observedVideoProps = [];
    if (this.transport) {
      try {
        await this.transport.dispatch({ action: "Unload" }, "player");
      } catch {}
    }
    this.patch({ player: createRuntimeSnapshotWithSubtitlePreferences().player });
  };

  attachPlayer = async (container: HTMLElement): Promise<void> => {
    const generation = this.playerGeneration;
    if (this.videoContainer !== container) this.videoSessionPromise = null;
    this.videoContainer = container;
    let pending: Promise<void> | null = null;
    try {
      if (!this.video) {
        pending = this.videoSessionPromise ??= createVideoEngine(container).then((session) => {
          if (generation !== this.playerGeneration || this.videoContainer !== container || this.videoSessionPromise !== pending) {
            session.destroy();
            return;
          }
          this.videoSession = session;
          this.video = session.engine;
          this.videoDevice = session.device;
          this.bindVideoEvents(this.video, generation);
        });
        await pending;
      }
      if (generation !== this.playerGeneration || this.videoContainer !== container) return;
      await this.refreshPlayer(generation);
    } catch {
      if (generation !== this.playerGeneration || this.videoContainer !== container || (pending && this.videoSessionPromise !== pending)) return;
      this.videoSession?.destroy();
      this.videoSession = null;
      this.video = null;
      this.patchPlayerError();
    } finally {
      if (pending && this.videoSessionPromise === pending) this.videoSessionPromise = null;
    }
  };

  detachPlayer = (): void => {
    this.videoContainer = null;
  };

  setPlaybackPaused = (paused: boolean): void => {
    this.setVideoProp("paused", paused);
    this.patch({ player: { ...this.snapshot.player, paused } });
    this.dispatchPlayerAction("PausedChanged", { paused });
    this.flushPlaybackProgress();
  };

  seekPlayback = (time: number): void => {
    const next = Math.max(0, time);
    window.panoramaDesktop?.recordPlaybackTiming?.("seek-requested");
    this.pendingSeek = { target: next, expiresAt: Date.now() + SEEK_ACK_TIMEOUT_MS };
    this.setVideoProp("time", next * 1000);
    this.patch({
      player: {
        ...this.snapshot.player,
        time: next,
        duration: Math.max(this.snapshot.player.duration, next),
      },
    });
    this.dispatchPlayerAction("Seek", this.progressPayload(next));
    this.lastProgressSyncTime = next;
  };

  setPlaybackVolume = (volume: number): void => {
    const next = Math.min(2, Math.max(0, volume));
    if (next > 0) {
      this.lastNonzeroPlaybackVolume = next;
      this.setVideoProp("muted", false);
      this.setVideoProp("volume", Math.round(next * 100));
      this.patch({ player: { ...this.snapshot.player, volume: next, muted: false } });
      return;
    }
    this.setVideoProp("volume", 0);
    this.setVideoProp("muted", true);
    this.patch({ player: { ...this.snapshot.player, volume: 0, muted: true } });
  };

  setPlaybackMuted = (muted: boolean): void => {
    const restoredVolume = this.snapshot.player.volume > 0
      ? this.snapshot.player.volume
      : this.lastNonzeroPlaybackVolume;
    if (muted) {
      this.setVideoProp("muted", true);
      this.patch({ player: { ...this.snapshot.player, muted: true } });
      return;
    }
    this.setVideoProp("muted", false);
    this.setVideoProp("volume", Math.round(restoredVolume * 100));
    this.patch({ player: { ...this.snapshot.player, volume: restoredVolume, muted: false } });
  };

  setPlaybackFullscreen = (fullscreen: boolean, target: HTMLElement): void => {
    this.setFullscreenTarget(target);
    if (fullscreen) {
      if (document.fullscreenElement === target) {
        this.handleFullscreenChange();
        return;
      }
      if (typeof target.requestFullscreen !== "function") return;
      void target.requestFullscreen().catch(() => this.handleFullscreenChange());
      return;
    }
    if (document.fullscreenElement === target && typeof document.exitFullscreen === "function") {
      void document.exitFullscreen().catch(() => this.handleFullscreenChange());
    } else {
      this.handleFullscreenChange();
    }
  };

  selectAudioTrack = (trackId: string): void => {
    this.autoSelectEnglishAudio = false;
    this.applyAudioSelection(trackId);
  };

  private applyAudioSelection = (trackId: string): void => {
    const engineId = this.audioTargets.get(trackId);
    if (!engineId) return;
    this.setVideoProp("selectedAudioTrackId", engineId);
    this.patch({
      player: {
        ...this.snapshot.player,
        audio: { ...this.snapshot.player.audio, selectedId: trackId, error: null },
      },
    });
  };

  selectSubtitle = (trackId: string | null): void => {
    this.autoSelectEnglishSubtitle = false;
    this.applySubtitleSelection(trackId);
  };

  private applySubtitleSelection = (trackId: string | null): void => {
    if (trackId === null) {
      this.setVideoProp("selectedSubtitlesTrackId", null);
      this.setVideoProp("selectedExtraSubtitlesTrackId", null);
      this.patch({
        player: {
          ...this.snapshot.player,
          subtitles: { ...this.snapshot.player.subtitles, selectedId: null, pendingId: null, error: null },
        },
      });
      return;
    }
    const target = this.subtitleTargets.get(trackId);
    if (!target) return;
    this.patch({
      player: {
        ...this.snapshot.player,
        subtitles: { ...this.snapshot.player.subtitles, status: "loading", pendingId: trackId, error: null },
      },
    });
    if (target.origin === "embedded") {
      this.setVideoProp("selectedExtraSubtitlesTrackId", null);
      this.setVideoProp("selectedSubtitlesTrackId", target.engineId);
    } else {
      this.setVideoProp("selectedSubtitlesTrackId", null);
      this.setVideoProp("selectedExtraSubtitlesTrackId", target.engineId);
    }
    this.applySubtitleDelay(this.snapshot.player.subtitles.offset);
    this.applySubtitleVerticalPosition(this.snapshot.player.subtitles.verticalPosition);
  };

  setSubtitleStyle = (style: Partial<PanoramaSubtitleStyle>): void => {
    const next = updateSubtitleAppearance(this.snapshot.player.subtitles.style, style);
    this.patch({
      player: {
        ...this.snapshot.player,
        subtitles: { ...this.snapshot.player.subtitles, style: next },
      },
    });
    this.persistSubtitlePreferences(next, this.snapshot.player.subtitles.verticalPosition);
    this.applySubtitleStyle(next);
  };

  setSubtitleOffset = (offset: number): void => {
    const next = clampSubtitleOffset(offset);
    this.patch({
      player: {
        ...this.snapshot.player,
        subtitles: { ...this.snapshot.player.subtitles, offset: next },
      },
    });
    this.applySubtitleDelay(next);
  };

  setSubtitleVerticalPosition = (position: number): void => {
    const next = clampSubtitleVerticalPosition(position);
    this.patch({
      player: {
        ...this.snapshot.player,
        subtitles: { ...this.snapshot.player.subtitles, verticalPosition: next },
      },
    });
    this.persistSubtitlePreferences(this.snapshot.player.subtitles.style, next);
    this.applySubtitleVerticalPosition(next);
  };

  flushPlaybackProgress = (): void => {
    const player = this.snapshot.player;
    if (
      this.snapshot.account.status !== "signedIn" ||
      player.status === "idle" ||
      !Number.isFinite(player.time) ||
      !Number.isFinite(player.duration) ||
      player.duration <= 0
    ) {
      return;
    }
    this.dispatchPlayerAction("TimeChanged", this.progressPayload(player.time));
    this.lastProgressSyncTime = player.time;
    if (this.snapshot.details.filmId === player.filmId) {
      this.patch({
        details: {
          ...this.snapshot.details,
          resume: {
            available: player.time >= 30 && player.duration - player.time >= 60,
            offset: player.time,
            duration: player.duration,
          },
        },
      });
    }
  };

  private serviceDiscoveryGeneration = 0;
  private serviceDiscoveryController: AbortController | null = null;

  checkService = async (): Promise<ServiceStatus> => {
    const generation = ++this.serviceDiscoveryGeneration;
    this.serviceDiscoveryController?.abort();
    const controller = new AbortController();
    this.serviceDiscoveryController = controller;
    const context = await this.getContextSafely();
    if (generation !== this.serviceDiscoveryGeneration) return this.snapshot.service.status;
    const configuredEndpoint = context?.profile?.settings?.streamingServerUrl;
    const endpoints = serviceEndpointCandidates(configuredEndpoint);
    const endpoint = endpoints[0] ?? DEFAULT_SERVICE_ENDPOINT;

    this.patch({ service: { status: "checking", endpoint } });

    const onlineEndpoint = await discoverService(endpoints, this.lastServiceOnlineEndpoint, async (candidate, signal) => {
      try { return (await fetch(serviceHealthUrl(candidate), { signal })).ok; } catch { return false; }
    }, controller.signal);
    if (generation !== this.serviceDiscoveryGeneration || controller.signal.aborted) return this.snapshot.service.status;
    this.serviceDiscoveryController = null;
    const status: ServiceStatus = onlineEndpoint ? "online" : "offline";
    const resolvedEndpoint = onlineEndpoint ?? endpoint;
    if (onlineEndpoint) {
      this.lastServiceOnlineAt = Date.now();
      this.lastServiceOnlineEndpoint = onlineEndpoint;
    }
    this.patch({ service: { status, endpoint: resolvedEndpoint } });
    if (status === "offline" && this.snapshot.player.status !== "idle") this.handleServiceLoss();
    return status;
  };

  private scheduleSourceWarm = (sourceId: string, filmId: string, target: PrivatePlaybackTarget): void => {
    if (this.sourceWarmTimer) clearTimeout(this.sourceWarmTimer);
    this.sourceWarmTimer = null;
    const url = warmableSourceUrl(target.stream);
    if (!url || !window.panoramaDesktop?.warmMedia) return;
    this.sourceWarmTimer = setTimeout(() => {
      this.sourceWarmTimer = null;
      if (
        this.snapshot.details.filmId !== filmId ||
        this.preparedPlayback?.sourceId !== sourceId ||
        this.snapshot.player.status !== "idle"
      ) return;
      void window.panoramaDesktop?.warmMedia?.(url).then((result) => {
        if (result.unreachable) this.markSourceUnreachable(sourceId, url);
      }).catch(() => undefined);
    }, SOURCE_WARM_DELAY_MS);
  };

  private sourceRecentlyUnreachable = (url: string | null): boolean => {
    const at = url ? this.unreachableSourceUrls.get(url) : undefined;
    return at !== undefined && Date.now() - at < UNREACHABLE_SOURCE_MS;
  };

  private markSourceUnreachable = (sourceId: string, url: string): void => {
    this.unreachableSourceUrls.set(url, Date.now());
    const sources = this.snapshot.details.sources;
    this.patch({
      details: {
        ...this.snapshot.details,
        sources: {
          ...sources,
          groups: sources.groups.map((group) => ({
            ...group,
            items: group.items.map((item) => item.id === sourceId && !item.unavailableReason
              ? { ...item, unavailableReason: UNREACHABLE_SOURCE_REASON }
              : item),
          })),
        },
      },
    });
  };

  // Replaces the generic playback error when the source's server refused the
  // connection, which choosing another source or waiting can resolve.
  private explainPlayerError = (generation: number): void => {
    const read = window.panoramaDesktop?.getPlaybackDiagnostics;
    if (!read) return;
    void read().then((diagnostics) => {
      if (
        generation !== this.playerGeneration ||
        this.snapshot.player.status !== "error" ||
        !diagnostics?.sourceUnreachable
      ) return;
      this.patch({ player: { ...this.snapshot.player, error: UNREACHABLE_PLAYBACK_ERROR } });
    }).catch(() => undefined);
  };

  destroy = (): void => {
    this.serviceDiscoveryGeneration += 1;
    this.serviceDiscoveryController?.abort();
    if (this.sourceWarmTimer) clearTimeout(this.sourceWarmTimer);
    this.sourceWarmTimer = null;
    void this.stopPlayback();
    this.worker?.terminate();
    this.worker = null;
    this.transport = null;
    this.listeners.clear();
    this.tmdbByPublicId.clear();
    this.imdbByPublicId.clear();
    this.privateSources.clear();
    window.onCoreEvent = null;
  };

  private rememberTmdbIds = (films: PanoramaFilm[]): void => {
    for (const film of films) {
      const tmdbId = parseTmdbPublicId(film.id);
      if (tmdbId) this.tmdbByPublicId.set(film.id, tmdbId);
    }
  };

  private handleCoreEvent = (envelope: CoreEnvelope): void => {
    if (envelope.name === "NewState" && Array.isArray(envelope.args)) {
      const models = envelope.args.filter((item): item is string => typeof item === "string");
      if (models.includes("ctx")) void this.refreshContext();
      if (models.includes("installed_addons")) void this.refreshAddons();
      if (models.includes("streaming_server")) void this.checkService();
      if (models.includes("meta_details")) void this.refreshFilmDetails(this.detailRequest);
      if (models.includes("player")) void this.refreshPlayer(this.playerGeneration);
      return;
    }

    if (envelope.name !== "CoreEvent" || !envelope.args || typeof envelope.args !== "object") {
      return;
    }

    const coreEvent = envelope.args as {
      event?: string;
      args?: { source?: { event?: string }; error?: unknown };
    };

    if (coreEvent.event === "UserAuthenticated") {
      void this.refreshContext();
    }

    if (
      coreEvent.event === "Error" &&
      coreEvent.args?.source?.event === "UserAuthenticated"
    ) {
      this.patch({
        account: {
          status: "error",
          email: null,
          error: this.message(coreEvent.args.error, "Sign in failed."),
        },
      });
    }
  };

  private refreshContext = async (): Promise<void> => {
    const context = await this.getContextSafely();
    const email = context?.profile?.auth?.user?.email;
    const settings = context?.profile?.settings;
    if (this.snapshot.player.status === "idle" && settings && !hasStoredSubtitlePreferences()) {
      const currentStyle = this.snapshot.player.subtitles.style;
      const color = (value: unknown, fallback: string) =>
        typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value) ? value : fallback;
      this.patch({
        player: {
          ...this.snapshot.player,
          subtitles: {
            ...this.snapshot.player.subtitles,
            style: {
              ...currentStyle,
              ...normalizeSubtitleAppearance({ ...currentStyle, fontSizePx: undefined, size: clampSubtitleSize(settings.subtitlesSize ?? currentStyle.size) }),
              textColor: color(settings.subtitlesTextColor, currentStyle.textColor),
              backgroundColor: color(settings.subtitlesBackgroundColor, currentStyle.backgroundColor),
              outlineColor: color(settings.subtitlesOutlineColor, currentStyle.outlineColor),
            },
          },
        },
      });
      this.persistSubtitlePreferences(
        this.snapshot.player.subtitles.style,
        this.snapshot.player.subtitles.verticalPosition,
      );
    }

    if (email) {
      const accountChanged = this.snapshot.account.status !== "signedIn" || this.snapshot.account.email !== email;
      if (accountChanged) this.addonCatalogSignature = null;
      this.patch({ account: { status: "signedIn", email, error: null } });
      if (accountChanged || this.snapshot.addons.status === "idle" || this.snapshot.addons.status === "error") {
        await this.syncAddons();
      }
    } else {
      this.addonCatalogSignature = null;
      this.patch({
        account: { status: "loggedOut", email: null, error: null },
        addons: { status: "idle", count: 0, error: null },
      });
      if (this.snapshot.details.filmId) {
        await this.openFilmDetails(this.snapshot.details.filmId);
      }
    }
  };

  syncAddons = async (): Promise<void> => {
    const transport = this.transport;
    if (!transport || this.snapshot.account.status !== "signedIn") return;
    if (this.addonSyncPromise) return this.addonSyncPromise;
    this.addonSyncPromise = (async () => {
      this.patch({ addons: { ...this.snapshot.addons, status: "syncing", error: null } });
      try {
        await transport.dispatch(
          {
            action: "Load",
            args: {
              model: "InstalledAddonsWithFilters",
              args: { request: { type: "movie" } },
            },
          },
          "installed_addons",
        );
        await this.refreshAddons();
      } catch (error) {
        this.patch({
          addons: {
            status: "error",
            count: 0,
            error: this.message(error, "Addon sync failed."),
          },
        });
      }
    })();
    try {
      await this.addonSyncPromise;
    } finally {
      this.addonSyncPromise = null;
    }
  };

  private refreshAddons = async (): Promise<void> => {
    if (!this.transport) return;
    try {
      const state = await this.transport.getState<InstalledAddonsState>("installed_addons");
      const catalog = Array.isArray(state.catalog) ? state.catalog : [];
      const addonNamesByTransport = new Map<string, string>();
      const signature = JSON.stringify(catalog.map((entry) => {
        if (!entry || typeof entry !== "object") return [null, null, null];
        const addon = entry as InstalledAddonEntry;
        return [
          optionalAddonString(addon.transportUrl) ?? optionalAddonString(addon.addon?.transportUrl),
          optionalAddonString(addon.manifest?.id) ?? optionalAddonString(addon.addon?.manifest?.id),
          optionalAddonString(addon.manifest?.name) ?? optionalAddonString(addon.addon?.manifest?.name),
        ];
      }));
      for (const entry of catalog) {
        if (!entry || typeof entry !== "object") continue;
        const addon = entry as InstalledAddonEntry;
        const transportUrl = optionalAddonString(addon.transportUrl) ?? optionalAddonString(addon.addon?.transportUrl);
        const name = optionalAddonString(addon.manifest?.name) ?? optionalAddonString(addon.addon?.manifest?.name);
        if (transportUrl && name) addonNamesByTransport.set(transportUrl, name);
      }
      const catalogChanged = signature !== this.addonCatalogSignature;
      this.addonCatalogSignature = signature;
      this.addonNamesByTransport = addonNamesByTransport;
      this.patch({
        addons: {
          status: "ready",
          count: catalog.length,
          error: null,
        },
      });
      if (catalogChanged && this.snapshot.details.filmId) {
        await this.openFilmDetails(this.snapshot.details.filmId);
      }
    } catch (error) {
      this.patch({
        addons: {
          status: "error",
          count: 0,
          error: this.message(error, "Addon sync failed."),
        },
      });
    }
  };

  private refreshFilmDetails = async (request: number): Promise<void> => {
    if (!this.transport || request !== this.detailRequest) return;
    try {
      const state = await this.transport.getState<CoreMetaDetailsState>("meta_details");
      const filmId = this.snapshot.details.filmId;
      const imdbId = filmId ? this.imdbByPublicId.get(filmId) ?? null : null;
      if (!isCurrentFilmRequest(request, this.detailRequest, imdbId, state.selected?.metaPath?.id)) {
        return;
      }

      const metadata = this.snapshot.details.metadata;
      const streamId = imdbId;
      const rawGroups = onlyAioStreamsSourceGroups(state.streams) as NonNullable<
        CoreMetaDetailsState["streams"]
      >;
      const groups = normalizeSourceGroups(rawGroups);
      groups.forEach((group, groupIndex) => {
        const raw = rawGroups[groupIndex];
        const rawItems = Array.isArray(raw?.content?.content) ? raw.content.content : [];
        group.items.forEach((source, sourceIndex) => {
          if (source.playbackSupport !== "internal" || !streamId) return;
          const streamTransportUrl = raw?.addon?.transportUrl;
          const metaTransportUrl = state.metaItem?.addon?.transportUrl;
          if (!streamTransportUrl || !metaTransportUrl) {
            source.playbackSupport = "unsupported";
            source.unavailableReason = "This source is missing the transport information required for playback.";
            return;
          }
          if (!source.unavailableReason && this.sourceRecentlyUnreachable(warmableSourceUrl(rawItems[sourceIndex]))) {
            source.unavailableReason = UNREACHABLE_SOURCE_REASON;
          }
          this.privateSources.set(source.id, {
            stream: rawItems[sourceIndex],
            streamTransportUrl,
            metaTransportUrl,
            streamPath: { resource: "stream", type: "movie", id: streamId, extra: [] },
            metaPath: { resource: "meta", type: "movie", id: streamId, extra: [] },
            subtitlesPath: { resource: "subtitles", type: "movie", id: streamId, extra: [] },
            addonName: group.addonName,
          });
        });
      });
      const hasLoading = groups.some((group) => group.status === "loading");
      const hasError = groups.some((group) => group.status === "error");
      const sources = this.canLoadSources()
        ? {
            status: hasLoading ? "loading" as const : hasError && groups.every((group) => group.status === "error") ? "error" as const : "ready" as const,
            groups,
            error: hasError && groups.every((group) => group.status === "error") ? "Unable to load sources." : null,
          }
        : { status: "idle" as const, groups: [], error: null };

      const resume = this.snapshot.account.status === "signedIn"
        ? normalizeResumeState(state.libraryItem)
        : { available: false, offset: 0, duration: null };
      const signedIn = this.snapshot.account.status === "signedIn";
      const saved = signedIn && normalizeWatchlisted(state.metaItem, state.libraryItem);
      const previous = this.snapshot.details.watchlist;
      const pending = signedIn && previous.pending && saved !== previous.saved;
      const watchlist = {
        available: signedIn && state.metaItem?.content?.type === "Ready",
        saved: pending ? previous.saved : saved,
        pending,
      };
      this.patch({ details: { filmId, resume, watchlist, metadata, sources } });
    } catch {
      if (request !== this.detailRequest) return;
      this.patch({
        details: {
          ...this.snapshot.details,
          sources: this.canLoadSources()
            ? { status: "error", groups: [], error: "Unable to load sources." }
            : this.snapshot.details.sources,
        },
      });
    }
  };

  private refreshPlayer = async (generation: number): Promise<void> => {
    if (!this.transport || generation !== this.playerGeneration || !this.snapshot.player.sourceId) return;
    try {
      const state = await this.transport.getState<CorePlayerState>("player");
      if (generation !== this.playerGeneration) return;
      if (state.stream?.type === "Err") {
        this.patchPlayerError();
        return;
      }
      if (state.stream?.type !== "Ready" || !state.stream.content) return;
      if (this.video && this.videoContainer && this.loadedPlayerGeneration !== generation) {
        this.patch({ player: { ...this.snapshot.player, stage: "loadingVideo" } });
        this.loadVideo(state.stream.content, state.subtitles, generation, state.selected?.stream);
        return;
      }
      if (this.video && this.loadedPlayerGeneration === generation) {
        this.syncExtraSubtitles(state.stream.content, state.subtitles, state.selected?.stream);
      }
    } catch {
      if (generation === this.playerGeneration) this.patchPlayerError();
    }
  };

  private extraSubtitleTracks = (stream: unknown, subtitles: unknown, selectedStream?: unknown): ExtraSubtitleTrack[] => {
    const origin = this.playerTarget?.addonName ?? "Addon";
    return uniqueExtraSubtitleTracks([
      ...extraSubtitleTracksFromStream(this.playerTarget?.stream, origin, this.addonNamesByTransport),
      ...extraSubtitleTracksFromStream(selectedStream, origin, this.addonNamesByTransport),
      ...extraSubtitleTracksFromStream(stream, origin, this.addonNamesByTransport),
      ...extraSubtitleTracksFromCore(subtitles, this.addonNamesByTransport),
    ]);
  };

  private syncExtraSubtitles = (stream: unknown, subtitles: unknown, selectedStream?: unknown): void => {
    this.pendingExtraSubtitles = this.extraSubtitleTracks(stream, subtitles, selectedStream);
    this.addEngineExtraSubtitles(this.pendingExtraSubtitles);
    this.publishAddonSubtitleTracks([]);
  };

  private addEngineExtraSubtitles = (tracks: ExtraSubtitleTrack[]): void => {
    if (!this.video || tracks.length === 0) return;
    try {
      this.video.dispatch({
        type: "command",
        commandName: "addExtraSubtitlesTracks",
        commandArgs: { tracks },
      });
    } catch {
      this.patchSubtitleError();
    }
  };

  private publishAddonSubtitleTracks = (engineTracks: unknown[]): void => {
    const fromEngine = engineTracks.flatMap((entry, index) => {
      if (!entry || typeof entry !== "object") return [];
      const track = entry as VideoTrack;
      const engineId = typeof track.id === "string" ? track.id : `addon-${index + 1}`;
      const mapped = mapVideoTrack(track, index);
      return [{
        engineId,
        label: mapped.label,
        language: mapped.language,
        sourceLabel: extraSubtitleSourceLabel(track.origin, this.playerTarget?.addonName ?? null, this.addonNamesByTransport),
      }];
    });
    const pendingExtras = this.pendingExtraSubtitles.filter(
      (pending) => !fromEngine.some((track) => track.engineId === pending.id),
    );
    const combined = [
      ...fromEngine,
      ...pendingExtras.map((track, index) => {
        const mapped = mapVideoTrack({ id: track.id, lang: track.lang, label: track.label, origin: track.origin }, index);
        return {
          engineId: track.id,
          label: mapped.label,
          language: mapped.language,
          sourceLabel: extraSubtitleSourceLabel(track.origin, this.playerTarget?.addonName ?? null, this.addonNamesByTransport),
        };
      }),
    ];
    this.replaceSubtitleOriginTracks("addon", combined);
  };

  private loadVideo = (stream: unknown, subtitles: unknown, generation: number, selectedStream?: unknown): void => {
    if (!this.video || !this.videoContainer || !this.playerTarget) return;
    this.loadedPlayerGeneration = generation;
    const origin = this.playerTarget.addonName;
    this.pendingExtraSubtitles = this.extraSubtitleTracks(stream, subtitles, selectedStream);
    const streamTracks = uniqueExtraSubtitleTracks([
      ...extraSubtitleTracksFromStream(this.playerTarget.stream, origin, this.addonNamesByTransport),
      ...extraSubtitleTracksFromStream(selectedStream, origin, this.addonNamesByTransport),
      ...extraSubtitleTracksFromStream(stream, origin, this.addonNamesByTransport),
    ]);
    const resolvedStream =
      stream && typeof stream === "object" && !Array.isArray(stream)
        ? { ...stream, ...(streamTracks.length > 0 ? { subtitles: streamTracks } : {}) }
        : stream;
    window.panoramaDesktop?.recordPlaybackTiming?.("video-load-requested");
    this.video.dispatch(
      {
        type: "command",
        commandName: "load",
        commandArgs: {
          stream: resolvedStream,
          autoplay: true,
          time: this.snapshot.player.time * 1000,
          streamingServerURL: this.snapshot.service.endpoint,
        },
      },
      { containerElement: this.videoContainer },
    );
    this.addEngineExtraSubtitles(this.pendingExtraSubtitles);
    this.publishAddonSubtitleTracks([]);
    try {
      this.applySubtitleStyle(this.snapshot.player.subtitles.style);
      this.applySubtitleDelay(this.snapshot.player.subtitles.offset);
      this.applySubtitleVerticalPosition(this.snapshot.player.subtitles.verticalPosition);
    } catch {
      this.patchSubtitleError();
    }
  };

  private patchSubtitleError = (): void => {
    this.patch({
      player: {
        ...this.snapshot.player,
        subtitles: {
          ...this.snapshot.player.subtitles,
          status: "error",
          pendingId: null,
          error: "Unable to load subtitles. Try another track.",
        },
      },
    });
  };

  private bindVideoEvents = (video: VideoEngine, generation: number): void => {
    const current = () => generation === this.playerGeneration;
    video.on("subtitleRenderingMode", (value) => {
      if (generation !== this.playerGeneration || !value || typeof value !== "object") return;
      const mode = value as { mode?: unknown; limitation?: unknown };
      if (!["custom", "native", "unknown"].includes(String(mode.mode))) return;
      this.patch({ player: { ...this.snapshot.player, subtitles: { ...this.snapshot.player.subtitles, renderingMode: mode.mode as "custom" | "native" | "unknown", appearanceLimitation: typeof mode.limitation === "string" ? mode.limitation : null } } });
    });
    video.on("implementationChanged", (...args) => {
      if (!current()) return;
      const manifest = args[0] as { props?: unknown; name?: unknown } | undefined;
      if (typeof manifest?.name === "string" && manifest.name.trim()) {
        this.videoDevice = manifest.name;
      }
      this.observedVideoProps = Array.isArray(manifest?.props)
        ? manifest.props.filter((prop): prop is string => typeof prop === "string")
        : [];
      for (const propName of this.observedVideoProps) {
        video.dispatch({ type: "observeProp", propName });
      }
    });
    const handleProp = (...args: unknown[]) => {
      if (!current()) return;
      this.handleVideoProp(String(args[0] ?? ""), args[1]);
    };
    video.on("propValue", handleProp);
    video.on("propChanged", handleProp);
    video.on("nativePlaybackActive", () => {
      if (
        current() &&
        this.loadedPlayerGeneration === generation &&
        this.snapshot.player.stage === "loadingVideo" &&
        (this.snapshot.player.status === "preparing" || this.snapshot.player.status === "buffering")
      ) {
        this.completePlaybackPreparation();
      }
    });
    video.on("ended", () => {
      if (!current()) return;
      this.clearPreparationMonitor();
      this.dispatchPlayerAction("Ended");
      this.patch({
        details: {
          ...this.snapshot.details,
          resume: { available: false, offset: 0, duration: this.snapshot.player.duration || null },
        },
        player: { ...this.snapshot.player, status: "ended", stage: null, paused: true, buffering: false },
      });
    });
    video.on("error", (...args) => {
      if (!current()) return;
      const error = args[0] as { critical?: unknown } | undefined;
      if (error?.critical === false && this.snapshot.player.subtitles.pendingId) {
        this.patchSubtitleError();
      } else if (error?.critical !== false) {
        this.patchPlayerError();
        this.explainPlayerError(generation);
      }
    });
    video.on("subtitlesTrackLoaded", (...args) => this.confirmSubtitle(args[0], "embedded", generation));
    video.on("extraSubtitlesTrackLoaded", (...args) => this.confirmSubtitle(args[0], "addon", generation));
    video.on("extraSubtitlesTrackError", () => {
      if (current() && this.snapshot.player.subtitles.pendingId) this.patchSubtitleError();
    });
  };

  private handleVideoProp = (propName: string, value: unknown): void => {
    const player = this.snapshot.player;
    if (propName === "loaded" && value === true) {
      this.completePlaybackPreparation();
      return;
    }
    if (propName === "paused" && typeof value === "boolean") {
      if (player.paused !== value) this.dispatchPlayerAction("PausedChanged", { paused: value });
      this.patch({ player: { ...player, paused: value } });
      return;
    }
    if (propName === "time" && typeof value === "number") {
      const next = Math.max(0, value / 1000);
      if (this.pendingSeek) {
        if (Math.abs(next - this.pendingSeek.target) <= SEEK_ACK_TOLERANCE_SECONDS) {
          this.pendingSeek = null;
        } else if (Date.now() < this.pendingSeek.expiresAt) {
          return;
        } else {
          this.pendingSeek = null;
        }
      }
      if (
        player.status === "preparing" &&
        player.stage === "loadingVideo" &&
        this.videoDevice.includes("ShellVideo")
      ) {
        const previous = this.preparingTimeSample;
        this.preparingTimeSample = next;
        if (previous === null || Math.abs(next - previous) < 0.05) return;
        this.completePlaybackPreparation();
      }
      const currentPlayer = this.snapshot.player;
      if (!["ready", "buffering", "ended"].includes(currentPlayer.status)) return;
      this.patch({
        player: {
          ...currentPlayer,
          time: next,
          duration: currentPlayer.duration > 0
            ? Math.max(currentPlayer.duration, next)
            : 0,
        },
      });
      if (Math.abs(next - this.lastProgressSyncTime) >= PROGRESS_SYNC_INTERVAL_SECONDS) {
        this.flushPlaybackProgress();
      }
      return;
    }
    if (propName === "duration") {
      const nativeDuration = normalizeNativePlaybackDuration(value);
      const next = this.probedPlaybackDuration ?? nativeDuration;
      if (next !== null) {
        this.patch({
          player: {
            ...player,
            duration: Math.max(player.time, next),
          },
        });
      }
      return;
    }
    if (propName === "buffering" && typeof value === "boolean") {
      if (!value && player.status === "buffering" && player.stage !== null) {
        this.completePlaybackPreparation();
        return;
      }
      this.patch({
        player: {
          ...player,
          buffering: value,
          status: value ? "buffering" : player.status === "buffering" ? "ready" : player.status,
        },
      });
      return;
    }
    if (propName === "volume" && typeof value === "number") {
      if (this.videoSession?.device === "ShellVideo") return;
      this.patch({ player: { ...player, volume: Math.min(2, Math.max(0, value / 100)) } });
      return;
    }
    if (propName === "muted" && typeof value === "boolean") {
      if (this.videoSession?.device === "ShellVideo") return;
      this.patch({ player: { ...player, muted: value } });
      return;
    }
    if (propName === "fullscreen" && typeof value === "boolean") {
      this.patch({ player: { ...player, fullscreen: value } });
      return;
    }
    if (propName === "audioTracks" && Array.isArray(value)) {
      this.updateAudioTracks(value);
      return;
    }
    if (propName === "selectedAudioTrackId") {
      this.confirmAudioTrack(value);
      return;
    }
    if (propName === "subtitlesTracks" && Array.isArray(value)) {
      this.updateSubtitleTracks(value, "embedded");
      return;
    }
    if (propName === "extraSubtitlesTracks" && Array.isArray(value)) {
      this.publishAddonSubtitleTracks(value);
      return;
    }
    if (propName === "stream") {
      if (value != null) {
        const generation = this.playerGeneration;
        this.addEngineExtraSubtitles(this.pendingExtraSubtitles);
        queueMicrotask(() => {
          if (generation === this.playerGeneration && this.snapshot.player.status !== "idle") {
            this.applyPlaybackAudioState();
          }
        });
      }
      return;
    }
    if (propName === "videoParams" && value && typeof value === "object") {
      const durationMs = (value as { durationMs?: unknown }).durationMs;
      const probedDuration = typeof durationMs === "number" && Number.isFinite(durationMs) && durationMs > 0
        ? durationMs / 1000
        : null;
      if (probedDuration !== null) this.probedPlaybackDuration = probedDuration;
      const player = this.snapshot.player;
      this.patch({
        player: {
          ...player,
          trackDiscoveryReady: true,
          duration: probedDuration === null
            ? player.duration
            : Math.max(player.time, probedDuration),
        },
      });
      this.dispatchVideoParams(value);
      this.tryAutoSelectEnglishSubtitle();
    }
  };

  private updateAudioTracks = (tracks: unknown[]): void => {
    this.audioTargets.clear();
    const nextTracks = tracks.flatMap((entry, index) => {
      if (!entry || typeof entry !== "object") return [];
      const track = entry as VideoTrack;
      const engineId = track.id == null ? `audio-${index + 1}` : String(track.id);
      const publicId = `audio-${index + 1}`;
      this.audioTargets.set(publicId, engineId);
      const mapped = mapVideoTrack(track, index);
      return [{ id: publicId, label: mapped.label, language: mapped.language, description: mapped.description }];
    });
    const selectedStillPresent = nextTracks.some((track) => track.id === this.snapshot.player.audio.selectedId);
    this.patch({
      player: {
        ...this.snapshot.player,
        audio: {
          status: "ready",
          tracks: nextTracks,
          selectedId: selectedStillPresent ? this.snapshot.player.audio.selectedId : null,
          error: null,
        },
      },
    });
    this.tryAutoSelectEnglishAudio();
  };

  private confirmAudioTrack = (value: unknown): void => {
    const engineId = value == null ? null : String(value);
    const selectedId = engineId
      ? [...this.audioTargets.entries()].find(([, id]) => id === engineId)?.[0] ?? null
      : null;
    if (selectedId === this.snapshot.player.audio.selectedId) return;
    this.patch({
      player: {
        ...this.snapshot.player,
        audio: { ...this.snapshot.player.audio, selectedId, error: null },
      },
    });
    this.tryAutoSelectEnglishAudio();
  };

  private tryAutoSelectEnglishAudio = (): void => {
    if (!this.autoSelectEnglishAudio) return;
    const track = preferredEnglishAudioTrack(this.snapshot.player.audio.tracks);
    if (!track || !this.audioTargets.has(track.id) || track.id === this.snapshot.player.audio.selectedId) return;
    this.applyAudioSelection(track.id);
  };

  private updateSubtitleTracks = (tracks: unknown[], origin: "embedded" | "addon"): void => {
    const fallbackAddonName = this.playerTarget?.addonName ?? null;
    const nextTracks = tracks.flatMap((entry, index) => {
      if (!entry || typeof entry !== "object") return [];
      const track = entry as VideoTrack;
      if (
        origin === "embedded" && typeof track.label === "string" &&
        track.label.startsWith(NATIVE_ADDON_SUBTITLE_TITLE_PREFIX)
      ) return [];
      const engineId = typeof track.id === "string" ? track.id : `${origin}-${index + 1}`;
      const mapped = mapVideoTrack(track, index);
      const sourceLabel =
        origin === "embedded"
          ? embeddedSubtitleSourceLabel(index, mapped.description)
          : extraSubtitleSourceLabel(track.origin, fallbackAddonName, this.addonNamesByTransport);
      return [{ engineId, label: mapped.label, language: mapped.language, sourceLabel }];
    });
    this.replaceSubtitleOriginTracks(origin, nextTracks);
  };

  private replaceSubtitleOriginTracks = (
    origin: "embedded" | "addon",
    tracks: Array<{ engineId: string; label: string; language: string | null; sourceLabel: string }>,
  ): void => {
    const engineIds = new Set(tracks.map((track) => track.engineId));
    for (const [publicId, target] of [...this.subtitleTargets]) {
      if (target.origin === origin && !engineIds.has(target.engineId)) this.subtitleTargets.delete(publicId);
    }
    const usedIds = new Set(this.subtitleTargets.keys());
    const nextTracks = tracks.map((track, index) => {
      const existing = [...this.subtitleTargets.entries()].find(
        ([, target]) => target.origin === origin && target.engineId === track.engineId,
      );
      let publicId = existing?.[0];
      if (!publicId) {
        let nextIndex = index + 1;
        publicId = `subtitle-${origin}-${nextIndex}`;
        while (usedIds.has(publicId)) {
          nextIndex += 1;
          publicId = `subtitle-${origin}-${nextIndex}`;
        }
        this.subtitleTargets.set(publicId, { origin, engineId: track.engineId });
        usedIds.add(publicId);
      }
      return {
        id: publicId,
        label: track.label,
        language: track.language,
        origin,
        sourceLabel: track.sourceLabel,
      };
    });
    const otherTracks = this.snapshot.player.subtitles.tracks.filter((track) => track.origin !== origin);
    this.patch({
      player: {
        ...this.snapshot.player,
        subtitles: {
          ...this.snapshot.player.subtitles,
          status: "ready",
          tracks: origin === "embedded" ? [...nextTracks, ...otherTracks] : [...otherTracks, ...nextTracks],
        },
      },
    });
    this.tryAutoSelectEnglishSubtitle();
  };

  private tryAutoSelectEnglishSubtitle = (): void => {
    if (
      !this.autoSelectEnglishSubtitle ||
      !this.snapshot.player.trackDiscoveryReady ||
      this.snapshot.player.status !== "ready"
    ) return;
    const track = preferredEnglishSubtitleTrack(this.snapshot.player.subtitles.tracks);
    if (!track || !this.subtitleTargets.has(track.id)) return;
    if (
      track.id === this.snapshot.player.subtitles.selectedId ||
      track.id === this.snapshot.player.subtitles.pendingId
    ) return;
    this.applySubtitleSelection(track.id);
  };

  private confirmSubtitle = (value: unknown, origin: "embedded" | "addon", generation: number): void => {
    if (generation !== this.playerGeneration || !value) return;
    const engineId = typeof value === "string"
      ? value
      : typeof value === "object" ? (value as VideoTrack).id : null;
    const selected = [...this.subtitleTargets.entries()].find(
      ([, target]) => target.origin === origin && target.engineId === engineId,
    );
    if (!selected) return;
    this.applySubtitleStyle(this.snapshot.player.subtitles.style);
    this.applySubtitleDelay(this.snapshot.player.subtitles.offset);
    this.applySubtitleVerticalPosition(this.snapshot.player.subtitles.verticalPosition);
    this.patch({
      player: {
        ...this.snapshot.player,
        subtitles: {
          ...this.snapshot.player.subtitles,
          selectedId: selected[0],
          status: "ready",
          pendingId: null,
          error: null,
        },
      },
    });
  };

  private applySubtitleStyle = (style: PanoramaSubtitleStyle): void => {
    this.setVideoProp("subtitleAppearance", style);
    for (const prefix of ["subtitles", "extraSubtitles"]) {
      this.setVideoProp(`${prefix}Size`, style.size);
      this.setVideoProp(`${prefix}TextColor`, style.textColor);
      this.setVideoProp(`${prefix}BackgroundColor`, style.backgroundColor);
      this.setVideoProp(`${prefix}OutlineColor`, "rgba(0, 0, 0, 0)");
      this.setVideoProp(`${prefix}PaddingX`, style.paddingX);
      this.setVideoProp(`${prefix}PaddingY`, style.paddingY);
      this.setVideoProp(`${prefix}FontWeight`, style.fontWeight);
    }
  };

  private persistSubtitlePreferences = (
    style: PanoramaSubtitleStyle,
    verticalPosition: number,
  ): void => {
    writeSubtitlePreferences({ style, verticalPosition });
  };

  private applySubtitleDelay = (offset: number): void => {
    this.setVideoProp("subtitlesDelay", offset * 1000);
    this.setVideoProp("extraSubtitlesDelay", offset * 1000);
  };

  private applySubtitleVerticalPosition = (position: number): void => {
    this.setVideoProp("subtitleVerticalPosition", position);
    this.setVideoProp("subtitlesOffset", position);
    this.setVideoProp("extraSubtitlesOffset", position);
  };

  private setVideoProp = (propName: string, propValue: unknown): void => {
    this.video?.dispatch({ type: "setProp", propName, propValue });
  };

  private applyPlaybackAudioState = (): void => {
    const { volume, muted } = this.snapshot.player;
    const restoredVolume = volume > 0 ? volume : this.lastNonzeroPlaybackVolume;
    if (muted) {
      this.setVideoProp("volume", Math.round(restoredVolume * 100));
      this.setVideoProp("muted", true);
      return;
    }
    this.setVideoProp("muted", false);
    this.setVideoProp("volume", Math.round(restoredVolume * 100));
  };

  // Playback starts as soon as video is ready; the English subtitle is chosen
  // from the tracks known at that point (embedded tracks are listed once the
  // file opens) and from addon tracks as they arrive, without holding playback.
  private completePlaybackPreparation = (): void => {
    this.clearPreparationMonitor();
    this.preparingTimeSample = null;
    this.addEngineExtraSubtitles(this.pendingExtraSubtitles);
    this.applyPlaybackAudioState();
    this.patch({
      player: {
        ...this.snapshot.player,
        status: "ready",
        stage: null,
        buffering: false,
        error: null,
        trackDiscoveryReady: true,
      },
    });
    this.tryAutoSelectEnglishSubtitle();
  };

  private setFullscreenTarget = (target: HTMLElement): void => {
    if (this.fullscreenTarget === target) return;
    document.removeEventListener("fullscreenchange", this.handleFullscreenChange);
    this.fullscreenTarget = target;
    document.addEventListener("fullscreenchange", this.handleFullscreenChange);
  };

  private clearFullscreenTarget = (): void => {
    document.removeEventListener("fullscreenchange", this.handleFullscreenChange);
    this.fullscreenTarget = null;
  };

  private handleFullscreenChange = (): void => {
    const fullscreen = this.fullscreenTarget !== null && document.fullscreenElement === this.fullscreenTarget;
    if (this.snapshot.player.fullscreen !== fullscreen) {
      this.patch({ player: { ...this.snapshot.player, fullscreen } });
    }
  };

  private progressPayload = (time: number): { time: number; duration: number; device: string } => ({
    time: Math.max(0, Math.round(time * 1000)),
    duration: Math.max(0, Math.round(this.snapshot.player.duration * 1000)),
    device: this.videoDevice,
  });

  private dispatchPlayerAction = (action: string, args?: unknown): void => {
    if (!this.transport || this.snapshot.account.status !== "signedIn") return;
    void this.transport.dispatch(
      {
        action: "Player",
        args: { action, ...(args === undefined ? {} : { args }) },
      },
      "player",
    ).catch(() => undefined);
  };

  private dispatchVideoParams = (videoParams: unknown): void => {
    if (!this.transport || !this.snapshot.player.sourceId) return;
    void this.transport.dispatch(
      {
        action: "Player",
        args: { action: "VideoParamsChanged", args: { videoParams } },
      },
      "player",
    ).catch(() => undefined);
  };

  private patchPlayerError = (): void => {
    this.clearPreparationMonitor();
    this.patch({
      player: {
        ...this.snapshot.player,
        status: "error",
        stage: null,
        paused: true,
        buffering: false,
        error: "Unable to play this source. Choose another source or try again.",
      },
    });
  };

  private handleServiceLoss = (): void => {
    this.flushPlaybackProgress();
    this.clearPreparationMonitor();
    this.video?.dispatch({ type: "command", commandName: "unload" });
    this.patch({
      player: {
        ...this.snapshot.player,
        status: "error",
        stage: null,
        paused: true,
        buffering: false,
        error: "Stremio Service is offline.",
      },
    });
  };

  private startPreparationMonitor = (generation: number): void => {
    this.clearPreparationMonitor();
    this.preparationServiceFailures = 0;
    this.preparationTimeout = setTimeout(() => {
      if (generation !== this.playerGeneration || this.snapshot.player.stage === null) return;
      this.video?.dispatch({ type: "command", commandName: "unload" });
      this.clearPreparationMonitor();
      this.patch({
        player: {
          ...this.snapshot.player,
          status: "error",
          stage: null,
          paused: true,
          buffering: false,
          error: "Playback is taking longer than expected. Try again or choose another source.",
        },
      });
    }, PLAYBACK_PREPARATION_TIMEOUT_MS);
    this.preparationServicePoll = setInterval(() => {
      void this.checkPreparationService(generation);
    }, PREPARATION_SERVICE_POLL_MS);
  };

  private checkPreparationService = async (generation: number): Promise<void> => {
    if (generation !== this.playerGeneration || this.snapshot.player.stage === null) return;
    const endpoint = this.snapshot.service.endpoint;
    try {
      const response = await fetch(serviceHealthUrl(endpoint), {
        signal: AbortSignal.timeout(2500),
      });
      if (generation !== this.playerGeneration || this.snapshot.player.stage === null) return;
      if (response.ok) {
        this.preparationServiceFailures = 0;
        return;
      }
      this.handlePreparationServiceFailure(endpoint);
    } catch {
      if (generation === this.playerGeneration && this.snapshot.player.stage !== null) {
        this.handlePreparationServiceFailure(endpoint);
      }
    }
  };

  private handlePreparationServiceFailure = (endpoint: string): void => {
    this.preparationServiceFailures += 1;
    if (this.preparationServiceFailures < PREPARATION_SERVICE_FAILURE_LIMIT) return;
    this.patch({ service: { status: "offline", endpoint } });
    this.handleServiceLoss();
  };

  private clearPreparationMonitor = (): void => {
    if (this.preparationTimeout) clearTimeout(this.preparationTimeout);
    if (this.preparationServicePoll) clearInterval(this.preparationServicePoll);
    this.preparationTimeout = null;
    this.preparationServicePoll = null;
    this.preparationServiceFailures = 0;
  };

  private canLoadSources = (): boolean =>
    this.snapshot.account.status === "signedIn" && this.snapshot.addons.status === "ready";

  private getContextSafely = async (): Promise<CoreContextState | null> => {
    if (!this.transport) return null;
    try {
      return await this.transport.getState<CoreContextState>("ctx");
    } catch {
      return null;
    }
  };

  private patch = (patch: Partial<RuntimeSnapshot>): void => {
    this.snapshot = { ...this.snapshot, ...patch };
    this.listeners.forEach((listener) => listener());
  };

  private message = (error: unknown, fallback: string): string => {
    if (error instanceof Error && error.message.trim()) return error.message;
    if (typeof error === "string" && error.trim()) return error;
    return fallback;
  };
}
