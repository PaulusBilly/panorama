import { updateSubtitleAppearance } from "./subtitle-appearance";
import { initialRuntimeSnapshot } from "./snapshot";
import {
  createRuntimeSnapshotWithSubtitlePreferences,
  writeSubtitlePreferences,
} from "./subtitle-preferences";
import {
  clampSubtitleVerticalPosition,
} from "./normalize";
import type {
  PanoramaSubtitleStyle,
  RuntimeListener,
  RuntimeSnapshot,
  ServiceStatus,
  StremioRuntime,
  PlaybackStartMode,
} from "./types";

const fixtureFilms = Array.from({ length: 9 }, (_, index) => ({
  id: `tmdb:${100 + index}`,
  type: "movie" as const,
  name: ["Aftersun", "Perfect Days", "Past Lives"][index % 3],
  year: String(2022 + (index % 3)),
  director: ["Charlotte Wells", "Wim Wenders", "Celine Song"][index % 3],
  originCountry: [null, "Japan", null][index % 3],
  rating: [7.6, 7.9, 7.8][index % 3],
  ratingCount: [2485, 1824, 3201][index % 3],
  posterUrl: null,
  landscapeUrl: null,
  logoUrl: null,
  description: null,
}));

const fixturePeople = [
  { id: "tmdb-person:1", name: "Jean-Luc Godard", department: "Directing", profileUrl: null },
  { id: "tmdb-person:2", name: "Agnès Godard", department: "Camera", profileUrl: null },
  { id: "tmdb-person:3", name: "Thierry Godard", department: "Acting", profileUrl: null },
  { id: "tmdb-person:4", name: "Monique Godard", department: "Acting", profileUrl: null },
  { id: "tmdb-person:5", name: "Coletta Godard", department: null, profileUrl: null },
  { id: "tmdb-person:6", name: "Alain Godard", department: "Production", profileUrl: null },
];

function searchFixtureFilms(query: string) {
  const normalizedQuery = query.toLocaleLowerCase();
  return fixtureFilms.filter((film) => (
    film.name.toLocaleLowerCase().includes(normalizedQuery) ||
    film.director?.toLocaleLowerCase() === normalizedQuery
  ));
}

export class FakeRuntime implements StremioRuntime {
  private snapshot: RuntimeSnapshot = createRuntimeSnapshotWithSubtitlePreferences();
  private listeners = new Set<RuntimeListener>();
  private resumeByFilm = new Map<string, { offset: number; duration: number | null }>();
  private watchlistedFilms = new Set<string>();
  private fullscreenTarget: HTMLElement | null = null;
  private lastNonzeroPlaybackVolume = 1;

  getSnapshot = (): RuntimeSnapshot => this.snapshot;
  subscribe = (listener: RuntimeListener): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  initialize = async (): Promise<void> => {
    this.set({
      ...this.snapshot,
      runtime: { status: "ready", error: null },
      service: { ...this.snapshot.service, status: "offline" },
      catalog: {
        mode: "popular",
        query: null,
        requestId: this.snapshot.catalog.requestId + 1,
        status: "ready",
        page: { items: fixtureFilms, nextSkip: 9, hasMore: true },
        loadingMore: false,
        error: null,
      },
      people: {
        query: null,
        requestId: this.snapshot.catalog.requestId + 1,
        status: "idle",
        page: { items: [], nextSkip: 0, hasMore: false },
        loadingMore: false,
        error: null,
      },
    });
  };

  login = async (email: string, password: string): Promise<void> => {
    void password;
    this.set({
      ...this.snapshot,
      account: { status: "signedIn", email, error: null },
      addons: { status: "ready", count: 4, error: null },
    });
    if (this.snapshot.details.filmId) {
      await this.openFilmDetails(this.snapshot.details.filmId);
    }
  };

  logout = async (): Promise<void> => {
    this.set({
      ...this.snapshot,
      account: { status: "loggedOut", email: null, error: null },
      addons: { status: "idle", count: 0, error: null },
    });
    if (this.snapshot.details.filmId) {
      await this.openFilmDetails(this.snapshot.details.filmId);
    }
  };

  loadPopularMovies = async (): Promise<void> => this.initialize();

  searchMovies = async (query: string): Promise<void> => {
    const normalizedQuery = query.trim();
    if (!normalizedQuery) {
      await this.clearSearch();
      return;
    }
    const items = searchFixtureFilms(normalizedQuery);
    const people = fixturePeople.filter((person) =>
      person.name.toLocaleLowerCase().includes(normalizedQuery.toLocaleLowerCase()),
    );
    const requestId = this.snapshot.catalog.requestId + 1;
    this.set({
      ...this.snapshot,
      catalog: {
        mode: "search",
        query: normalizedQuery,
        requestId,
        status: "ready",
        page: { items, nextSkip: items.length, hasMore: items.length > 0 },
        loadingMore: false,
        error: null,
      },
      people: {
        query: normalizedQuery,
        requestId,
        status: "ready",
        page: { items: people, nextSkip: people.length, hasMore: people.length > 0 },
        loadingMore: false,
        error: null,
      },
    });
  };

  retryMovieSearch = async (): Promise<void> => {
    const query = this.snapshot.catalog.query;
    if (!query) return;
    const items = searchFixtureFilms(query);
    this.set({
      ...this.snapshot,
      catalog: {
        ...this.snapshot.catalog,
        status: "ready",
        page: { items, nextSkip: items.length, hasMore: items.length > 0 },
        loadingMore: false,
        error: null,
      },
    });
  };

  retryPeopleSearch = async (): Promise<void> => {
    const query = this.snapshot.people.query;
    if (!query) return;
    const items = fixturePeople.filter((person) =>
      person.name.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
    );
    this.set({
      ...this.snapshot,
      people: {
        ...this.snapshot.people,
        status: "ready",
        page: { items, nextSkip: items.length, hasMore: items.length > 0 },
        loadingMore: false,
        error: null,
      },
    });
  };

  clearSearch = async (): Promise<void> => {
    this.set({
      ...this.snapshot,
      catalog: {
        mode: "popular",
        query: null,
        requestId: this.snapshot.catalog.requestId + 1,
        status: "ready",
        page: { items: fixtureFilms, nextSkip: fixtureFilms.length, hasMore: true },
        loadingMore: false,
        error: null,
      },
      people: {
        query: null,
        requestId: this.snapshot.catalog.requestId + 1,
        status: "idle",
        page: { items: [], nextSkip: 0, hasMore: false },
        loadingMore: false,
        error: null,
      },
    });
  };

  loadNextPage = async (): Promise<void> => {
    const source = this.snapshot.catalog.mode === "search" && this.snapshot.catalog.query
      ? searchFixtureFilms(this.snapshot.catalog.query)
      : fixtureFilms;
    const next = source.map((film, index) => ({
      ...film,
      id: `${film.id}-next-${index}`,
    }));
    const items = [...this.snapshot.catalog.page.items, ...next];
    this.set({
      ...this.snapshot,
      catalog: {
        ...this.snapshot.catalog,
        status: "ready",
        page: { items, nextSkip: items.length, hasMore: false },
        loadingMore: false,
        error: null,
      },
    });
  };

  loadNextPeoplePage = async (): Promise<void> => {
    if (!this.snapshot.people.query || !this.snapshot.people.page.hasMore) return;
    const next = this.snapshot.people.page.items.map((person, index) => ({
      ...person,
      id: `${person.id}-next-${index}`,
    }));
    const items = [...this.snapshot.people.page.items, ...next];
    this.set({
      ...this.snapshot,
      people: {
        ...this.snapshot.people,
        status: "ready",
        page: { items, nextSkip: items.length, hasMore: false },
        loadingMore: false,
        error: null,
      },
    });
  };

  openFilmDetails = async (filmId: string): Promise<void> => {
    const film = this.snapshot.catalog.page.items.find((item) => item.id === filmId)
      ?? fixtureFilms.find((item) => item.id === filmId);
    if (!film) return;
    const signedIn = this.snapshot.account.status === "signedIn";
    const savedResume = signedIn ? this.resumeByFilm.get(filmId) : null;
    this.set({
      ...this.snapshot,
      details: {
        filmId,
        watchlist: { available: signedIn, saved: signedIn && this.watchlistedFilms.has(filmId), pending: false },
        resume: savedResume
          ? {
              available: savedResume.offset >= 30 && (savedResume.duration === null || savedResume.duration - savedResume.offset >= 60),
              offset: savedResume.offset,
              duration: savedResume.duration,
            }
          : { available: false, offset: 0, duration: null },
        metadata: {
          status: "ready",
          item: {
            ...film,
            originalTitle: film.originCountry ? "パーフェクト・デイズ" : film.name,
            alternativeTitle: null,
            description: "A quiet, observant portrait shaped by memory and time.",
            runtime: "102 min",
            genres: ["Drama"],
            logoUrl: null,
          },
          error: null,
        },
        sources: signedIn
          ? {
              status: "ready",
              groups: [
                {
                  id: "source-group-1",
                  addonId: "com.stremio.aiostreams",
                  addonName: "AIOStreams",
                  status: "ready",
                  error: null,
                  items: [
                    {
                      id: "source-1-1",
                      addonId: "com.stremio.aiostreams",
                      addonName: "AIOStreams",
                      name: "1080p",
                      description: "Reliable source · English · 5.1",
                      quality: "hd",
                      audioChannels: "5.1",
                      playbackSupport: "internal",
                      unavailableReason: null,
                    },
                    {
                      id: "source-1-2",
                      addonId: "com.stremio.aiostreams",
                      addonName: "AIOStreams",
                      name: "External listing",
                      description: "Opens outside Panorama",
                      quality: null,
                      audioChannels: null,
                      playbackSupport: "external",
                      unavailableReason: "This source opens in an external player and is unavailable in Panorama.",
                    },
                  ],
                },
              ],
              error: null,
            }
          : { status: "idle", groups: [], error: null },
      },
    });
  };

  setWatchlisted = async (saved: boolean): Promise<void> => {
    const { filmId, watchlist } = this.snapshot.details;
    if (!filmId || !watchlist.available || watchlist.pending || saved === watchlist.saved) return;
    if (saved) this.watchlistedFilms.add(filmId);
    else this.watchlistedFilms.delete(filmId);
    this.set({
      ...this.snapshot,
      details: { ...this.snapshot.details, watchlist: { available: true, saved, pending: false } },
    });
  };

  retryFilmDetails = async (): Promise<void> => {
    if (this.snapshot.details.filmId) {
      await this.openFilmDetails(this.snapshot.details.filmId);
    }
  };

  closeFilmDetails = async (): Promise<void> => {
    this.set({ ...this.snapshot, details: structuredClone(initialRuntimeSnapshot.details) });
  };

  syncAddons = async (): Promise<void> => {
    if (this.snapshot.account.status !== "signedIn") return;
    this.set({
      ...this.snapshot,
      addons: { status: "ready", count: 4, error: null },
    });
  };

  checkService = async (): Promise<ServiceStatus> => {
    this.set({ ...this.snapshot, service: { ...this.snapshot.service, status: "online" } });
    return "online";
  };

  preparePlayback = async (sourceId: string): Promise<void> => {
    void sourceId;
  };

  startPlayback = async (sourceId: string, mode: PlaybackStartMode = "restart"): Promise<void> => {
    const source = this.snapshot.details.sources.groups
      .flatMap((group) => group.items)
      .find((item) => item.id === sourceId);
    if (!source || source.playbackSupport !== "internal") return;
    const subtitlePreferences = this.snapshot.player.subtitles;
    this.set({
      ...this.snapshot,
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "preparing",
        stage: "resolvingSource",
        filmId: this.snapshot.details.filmId,
        sourceId,
        title: this.snapshot.details.metadata.item?.name ?? null,
        time: mode === "resume" ? this.snapshot.details.resume.offset : 0,
        subtitles: {
          ...structuredClone(initialRuntimeSnapshot.player.subtitles),
          verticalPosition: subtitlePreferences.verticalPosition,
          style: subtitlePreferences.style,
        },
      },
    });
  };

  switchPlaybackSource = async (sourceId: string): Promise<void> => {
    const time = this.snapshot.player.time;
    const { volume, muted } = this.snapshot.player;
    await this.startPlayback(sourceId);
    this.set({ ...this.snapshot, player: { ...this.snapshot.player, time, volume, muted } });
  };

  retryPlayback = async (): Promise<void> => {
    const sourceId = this.snapshot.player.sourceId;
    if (!sourceId) return;
    const { time, volume, muted } = this.snapshot.player;
    await this.startPlayback(sourceId);
    this.set({ ...this.snapshot, player: { ...this.snapshot.player, time, volume, muted } });
  };

  stopPlayback = async (): Promise<void> => {
    this.flushPlaybackProgress();
    if (this.fullscreenTarget && document.fullscreenElement === this.fullscreenTarget) {
      try {
        await document.exitFullscreen();
      } catch {}
    }
    document.removeEventListener("fullscreenchange", this.handleFullscreenChange);
    this.fullscreenTarget = null;
    this.lastNonzeroPlaybackVolume = 1;
    this.set({ ...this.snapshot, player: createRuntimeSnapshotWithSubtitlePreferences().player });
  };

  attachPlayer = async (): Promise<void> => {
    this.set({
      ...this.snapshot,
      player: {
        ...this.snapshot.player,
        status: "ready",
        stage: null,
        trackDiscoveryReady: true,
        paused: false,
        duration: 6120,
        audio: {
          status: "ready",
          tracks: [
            { id: "audio-1", label: "English", language: "en", description: null },
            { id: "audio-2", label: "Français", language: "fr", description: "Commentary with film historian" },
          ],
          selectedId: "audio-1",
          error: null,
        },
        subtitles: {
          ...this.snapshot.player.subtitles,
          status: "ready",
          tracks: [
            { id: "subtitle-embedded-1", label: "English", language: "en", origin: "embedded", sourceLabel: "Embedded 1" },
            { id: "subtitle-addon-1", label: "English", language: "en", origin: "addon", sourceLabel: "OpenSubtitles v3" },
            { id: "subtitle-addon-2", label: "Bahasa Indonesia", language: "id", origin: "addon", sourceLabel: "Fixture addon" },
            { id: "subtitle-addon-3", label: "Bulgarian", language: "bg", origin: "addon", sourceLabel: "OpenSubtitles v3" },
          ],
          selectedId: "subtitle-embedded-1",
        },
      },
    });
  };

  detachPlayer = (): void => undefined;
  setPlaybackPaused = (paused: boolean): void => {
    this.set({ ...this.snapshot, player: { ...this.snapshot.player, paused } });
  };
  seekPlayback = (time: number): void => {
    const next = Math.max(0, time);
    this.set({
      ...this.snapshot,
      player: {
        ...this.snapshot.player,
        time: next,
        duration: Math.max(this.snapshot.player.duration, next),
      },
    });
  };
  setPlaybackVolume = (volume: number): void => {
    const next = Math.min(2, Math.max(0, volume));
    if (next > 0) this.lastNonzeroPlaybackVolume = next;
    this.set({ ...this.snapshot, player: { ...this.snapshot.player, volume: next, muted: next === 0 } });
  };
  setPlaybackMuted = (muted: boolean): void => {
    const volume = muted || this.snapshot.player.volume > 0
      ? this.snapshot.player.volume
      : this.lastNonzeroPlaybackVolume;
    this.set({ ...this.snapshot, player: { ...this.snapshot.player, volume, muted } });
  };
  setPlaybackFullscreen = (fullscreen: boolean, target: HTMLElement): void => {
    if (this.fullscreenTarget !== target) {
      document.removeEventListener("fullscreenchange", this.handleFullscreenChange);
      this.fullscreenTarget = target;
      document.addEventListener("fullscreenchange", this.handleFullscreenChange);
    }
    if (fullscreen && typeof target.requestFullscreen === "function") {
      void target.requestFullscreen().catch(() => this.handleFullscreenChange());
    } else if (!fullscreen && document.fullscreenElement === target && typeof document.exitFullscreen === "function") {
      void document.exitFullscreen().catch(() => this.handleFullscreenChange());
    } else {
      this.set({ ...this.snapshot, player: { ...this.snapshot.player, fullscreen } });
    }
  };
  selectAudioTrack = (trackId: string): void => {
    if (!this.snapshot.player.audio.tracks.some((track) => track.id === trackId)) return;
    this.set({
      ...this.snapshot,
      player: {
        ...this.snapshot.player,
        audio: { ...this.snapshot.player.audio, selectedId: trackId, error: null },
      },
    });
  };
  selectSubtitle = (trackId: string | null): void => {
    this.set({
      ...this.snapshot,
      player: {
        ...this.snapshot.player,
        subtitles: { ...this.snapshot.player.subtitles, selectedId: trackId, pendingId: null, error: null },
      },
    });
  };
  setSubtitleStyle = (style: Partial<PanoramaSubtitleStyle>): void => {
    const next = updateSubtitleAppearance(this.snapshot.player.subtitles.style, style);
    this.set({
      ...this.snapshot,
      player: {
        ...this.snapshot.player,
        subtitles: {
          ...this.snapshot.player.subtitles,
          style: next,
        },
      },
    });
    writeSubtitlePreferences({ style: next, verticalPosition: this.snapshot.player.subtitles.verticalPosition });
  };
  setSubtitleOffset = (offset: number): void => {
    this.set({
      ...this.snapshot,
      player: {
        ...this.snapshot.player,
        subtitles: { ...this.snapshot.player.subtitles, offset: Math.min(10, Math.max(-10, offset)) },
      },
    });
  };
  setSubtitleVerticalPosition = (position: number): void => {
    const verticalPosition = clampSubtitleVerticalPosition(position);
    this.set({
      ...this.snapshot,
      player: {
        ...this.snapshot.player,
        subtitles: {
          ...this.snapshot.player.subtitles,
          verticalPosition,
        },
      },
    });
    writeSubtitlePreferences({ style: this.snapshot.player.subtitles.style, verticalPosition });
  };
  flushPlaybackProgress = (): void => {
    const { filmId, time, duration } = this.snapshot.player;
    if (this.snapshot.account.status === "signedIn" && filmId && time >= 30) {
      this.resumeByFilm.set(filmId, { offset: time, duration: duration || null });
      if (this.snapshot.details.filmId === filmId) {
        this.set({
          ...this.snapshot,
          details: {
            ...this.snapshot.details,
            resume: {
              available: duration <= 0 || duration - time >= 60,
              offset: time,
              duration: duration || null,
            },
          },
        });
      }
    }
  };
  destroy = (): void => {
    document.removeEventListener("fullscreenchange", this.handleFullscreenChange);
    this.fullscreenTarget = null;
    this.listeners.clear();
  };

  setStateForTest(patch: Partial<RuntimeSnapshot>): void {
    this.set({ ...this.snapshot, ...patch });
  }

  private set(snapshot: RuntimeSnapshot): void {
    const { filmId, metadata } = snapshot.details;
    const available = snapshot.account.status === "signedIn" && filmId !== null && metadata.status === "ready";
    this.snapshot = {
      ...snapshot,
      details: {
        ...snapshot.details,
        watchlist: {
          available,
          saved: snapshot.account.status === "signedIn" && filmId !== null && this.watchlistedFilms.has(filmId),
          pending: false,
        },
      },
    };
    this.listeners.forEach((listener) => listener());
  }

  private handleFullscreenChange = (): void => {
    const fullscreen = this.fullscreenTarget !== null && document.fullscreenElement === this.fullscreenTarget;
    if (this.snapshot.player.fullscreen !== fullscreen) {
      this.set({ ...this.snapshot, player: { ...this.snapshot.player, fullscreen } });
    }
  };
}
