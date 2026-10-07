import type {
  PanoramaAudioTrack,
  PanoramaFilm,
  PanoramaFilmDetails,
  PanoramaResumeState,
  PanoramaSourceGroup,
  PanoramaSource,
  PanoramaSubtitleTrack,
  PlaybackSupport,
  SourceAudioChannels,
  SourceQuality,
  SourceGroupStatus,
} from "./types";

type CatalogMeta = {
  id?: unknown;
  type?: unknown;
  name?: unknown;
  releaseInfo?: unknown;
  released?: unknown;
  director?: unknown;
  imdbRating?: unknown;
  links?: unknown;
  poster?: unknown;
  background?: unknown;
  description?: unknown;
  runtime?: unknown;
  genres?: unknown;
  logo?: unknown;
};

type CoreAddon = {
  manifest?: { id?: unknown; name?: unknown };
};

type CoreSourceGroup = {
  addon?: CoreAddon;
  content?: {
    type?: "Loading" | "Ready" | "Err";
    content?: unknown;
  };
};

type CoreStream = {
  url?: unknown;
  infoHash?: unknown;
  ytId?: unknown;
  externalUrl?: unknown;
};

function optionalString(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : null;
}

export function parseFilmDirector(director: unknown, links: unknown): string | null {
  if (typeof director === "string") {
    const name = optionalString(director);
    if (name) return name;
  }

  if (Array.isArray(director)) {
    const names = director.flatMap((entry) => {
      const name = optionalString(entry);
      return name ? [name] : [];
    });
    if (names.length > 0) return [...new Set(names)].join(", ");
  }

  if (!Array.isArray(links)) return null;
  const names = links.flatMap((link) => {
    if (!link || typeof link !== "object") return [];
    const category = String((link as { category?: unknown }).category).toLowerCase();
    if (category !== "director" && category !== "directors") return [];
    const name = optionalString((link as { name?: unknown }).name);
    return name ? [name] : [];
  });

  return names.length > 0 ? [...new Set(names)].join(", ") : null;
}

export function parseFilmRating(imdbRating: unknown, links: unknown): number | null {
  const directRating = optionalString(imdbRating);
  if (directRating) {
    const value = Number(directRating);
    if (Number.isFinite(value) && value > 0) return Math.min(10, value);
  }

  if (!Array.isArray(links)) return null;
  const imdbLink = links.find(
    (link) =>
      link &&
      typeof link === "object" &&
      (link as { category?: unknown }).category === "imdb",
  ) as { name?: unknown } | undefined;

  const linked = Number(optionalString(imdbLink?.name));
  return Number.isFinite(linked) && linked > 0 ? Math.min(10, linked) : null;
}

export function parseFilmYear(releaseInfo: unknown, released: unknown): string | null {
  const releaseText = optionalString(releaseInfo);
  const yearMatch = releaseText?.match(/(?:19|20)\d{2}/);
  if (yearMatch) return yearMatch[0];

  const date = released instanceof Date ? released : new Date(String(released ?? ""));
  return Number.isNaN(date.getTime()) ? null : String(date.getUTCFullYear());
}

export function normalizeFilm(meta: CatalogMeta): PanoramaFilm | null {
  const id = optionalString(meta.id);
  const name = optionalString(meta.name);
  const type = optionalString(meta.type);

  if (!id || !name || type !== "movie") return null;

  return {
    id,
    type: "movie",
    name,
    year: parseFilmYear(meta.releaseInfo, meta.released),
    director: parseFilmDirector(meta.director, meta.links),
    originCountry: null,
    rating: parseFilmRating(meta.imdbRating, meta.links),
    ratingCount: null,
    posterUrl: optionalString(meta.poster),
    landscapeUrl: optionalString(meta.background),
    logoUrl: optionalString(meta.logo),
    description: optionalString(meta.description),
  };
}

export function applyDirectorNames(
  films: PanoramaFilm[],
  names: Map<string, string>,
): PanoramaFilm[] {
  return films.map((film) =>
    film.director ? film : { ...film, director: names.get(film.id) ?? null },
  );
}

export function directorNamesFromCatalog(items: unknown): Map<string, string> {
  const names = new Map<string, string>();
  for (const film of normalizeFilms(items)) {
    if (film.director) names.set(film.id, film.director);
  }
  return names;
}

export function normalizeFilms(items: unknown): PanoramaFilm[] {
  if (!Array.isArray(items)) return [];

  const seen = new Set<string>();
  return items.reduce<PanoramaFilm[]>((films, item) => {
    if (!item || typeof item !== "object") return films;
    const film = normalizeFilm(item as CatalogMeta);
    if (!film || seen.has(film.id)) return films;
    seen.add(film.id);
    films.push(film);
    return films;
  }, []);
}

export function normalizeFilmDetails(
  meta: unknown,
  fallback?: PanoramaFilm,
): PanoramaFilmDetails | null {
  const normalized = meta && typeof meta === "object" ? normalizeFilm(meta as CatalogMeta) : null;
  const film = normalized ?? fallback ?? null;
  if (!film) return null;

  const record = meta && typeof meta === "object" ? (meta as CatalogMeta) : {};
  const genres = Array.isArray(record.genres)
    ? record.genres.flatMap((genre) => {
        const value = optionalString(genre);
        return value ? [value] : [];
      })
    : [];

  return {
    ...film,
    originalTitle: null,
    alternativeTitle: null,
    runtime: optionalString(record.runtime),
    genres,
    logoUrl: optionalString(record.logo),
  };
}

export function parseSourceQuality(...labels: unknown[]): SourceQuality | null {
  const value = labels.flatMap((label) => typeof label === "string" ? [label] : []).join(" ");
  if (/\b(?:2160p|4k|uhd)\b/i.test(value)) return "4k";
  if (/\b(?:1080p|720p|hd)\b/i.test(value)) return "hd";
  return null;
}

export function parseSourceAudioChannels(...labels: unknown[]): SourceAudioChannels | null {
  const value = labels.flatMap((label) => typeof label === "string" ? [label] : []).join(" ");
  return /(?:^|[^\d])5\.1(?![\d.])/i.test(value) ? "5.1" : null;
}

// Prefers a playable source without a known problem, such as a server that
// did not respond to its warm-up; otherwise the first playable source.
export function firstPlayableSource(groups: PanoramaSourceGroup[]): PanoramaSource | null {
  const playable = groups.flatMap((group) => group.items.filter((item) => item.playbackSupport === "internal"));
  return playable.find((item) => !item.unavailableReason) ?? playable[0] ?? null;
}

export function normalizeWatchlisted(metaItem: unknown, libraryItem: unknown): boolean {
  const meta = metaItem && typeof metaItem === "object"
    ? metaItem as { content?: { type?: unknown; content?: { inLibrary?: unknown } | null } | null }
    : null;
  if (meta?.content?.type === "Ready") {
    return meta.content.content?.inLibrary === true;
  }
  const item = libraryItem && typeof libraryItem === "object"
    ? libraryItem as { removed?: unknown; temp?: unknown }
    : null;
  return item !== null && item.removed !== true && item.temp !== true;
}

export function normalizeResumeState(libraryItem: unknown): PanoramaResumeState {
  const state = libraryItem && typeof libraryItem === "object"
    ? (libraryItem as { state?: { timeOffset?: unknown; duration?: unknown } }).state
    : null;
  const offsetMs = typeof state?.timeOffset === "number" && Number.isFinite(state.timeOffset)
    ? Math.max(0, state.timeOffset)
    : 0;
  const durationMs = typeof state?.duration === "number" && Number.isFinite(state.duration)
    ? Math.max(0, state.duration)
    : null;
  const offset = offsetMs / 1000;
  const duration = durationMs === null ? null : durationMs / 1000;

  return {
    available: offset >= 30 && (duration === null || duration - offset >= 60),
    offset,
    duration,
  };
}

/** Stremio's built-in local-files addon; not useful as a Panorama source tab. */
export function isLocalFilesAddon(addonId: string, addonName?: string | null): boolean {
  if (addonId === "org.stremio.local") return true;
  const name = (addonName ?? "").trim().toLowerCase();
  return name.includes("local files") && name.includes("without catalog");
}

export function isAioStreamsAddon(addonId: string, addonName?: string | null): boolean {
  const normalizedId = addonId.trim().toLowerCase().replace(/[^a-z0-9]/g, "");
  const normalizedName = (addonName ?? "").trim().toLowerCase().replace(/[^a-z0-9]/g, "");
  return normalizedId.includes("aiostreams") || normalizedName === "aiostreams";
}

function sourceGroupAddon(entry: unknown, fallbackIndex: number): { addonId: string; addonName: string } {
  const group = entry && typeof entry === "object" ? (entry as CoreSourceGroup) : {};
  return {
    addonId: optionalString(group.addon?.manifest?.id) ?? `addon-${fallbackIndex + 1}`,
    addonName: optionalString(group.addon?.manifest?.name) ?? "Unknown addon",
  };
}

/** Drop the local-files addon before normalizing so UI and private stream maps stay aligned. */
export function withoutLocalFilesSourceGroups(groups: unknown): unknown[] {
  if (!Array.isArray(groups)) return [];
  return groups.filter((entry, index) => {
    const { addonId, addonName } = sourceGroupAddon(entry, index);
    return !isLocalFilesAddon(addonId, addonName);
  });
}

export function onlyAioStreamsSourceGroups(groups: unknown): unknown[] {
  if (!Array.isArray(groups)) return [];
  return groups.filter((entry, index) => {
    const { addonId, addonName } = sourceGroupAddon(entry, index);
    return isAioStreamsAddon(addonId, addonName);
  });
}

export function normalizeSourceGroups(groups: unknown): PanoramaSourceGroup[] {
  const visible = withoutLocalFilesSourceGroups(groups);

  return visible.map((entry, groupIndex) => {
    const group = entry && typeof entry === "object" ? (entry as CoreSourceGroup) : {};
    const { addonId, addonName } = sourceGroupAddon(entry, groupIndex);
    const loadableType = group.content?.type;
    const status: SourceGroupStatus =
      loadableType === "Err" ? "error" : loadableType === "Ready" ? "ready" : "loading";
    const rawItems = status === "ready" && Array.isArray(group.content?.content)
      ? group.content.content
      : [];
    const items = rawItems.map((item, sourceIndex) => {
      const source = item && typeof item === "object"
        ? (item as CoreStream & { name?: unknown; description?: unknown })
        : {};
      const playbackSupport = normalizePlaybackSupport(source);
      return {
        id: `source-${groupIndex + 1}-${sourceIndex + 1}`,
        addonId,
        addonName,
        name: optionalString(source.name) ?? `Source ${sourceIndex + 1}`,
        description: optionalString(source.description),
        quality: parseSourceQuality(source.name, source.description),
        audioChannels: parseSourceAudioChannels(source.name, source.description),
        playbackSupport,
        unavailableReason:
          playbackSupport === "external"
            ? "This source opens in an external player and is unavailable in Panorama."
            : playbackSupport === "unsupported"
              ? "This source format is not supported for in-app playback."
              : null,
      };
    });

    return {
      id: `source-group-${groupIndex + 1}`,
      addonId,
      addonName,
      status,
      items,
      error: status === "error" ? `Unable to load sources from ${addonName}.` : null,
    };
  });
}

export function normalizePlaybackSupport(source: CoreStream): PlaybackSupport {
  if (optionalString(source.url) || optionalString(source.infoHash) || optionalString(source.ytId)) {
    return "internal";
  }
  if (optionalString(source.externalUrl)) return "external";
  return "unsupported";
}

export function clampSubtitleSize(value: number): number {
  return Math.min(300, Math.max(50, Math.round(value)));
}

export function clampSubtitleOffset(value: number): number {
  return Math.min(10, Math.max(-10, Math.round(value * 2) / 2));
}

export function clampSubtitleVerticalPosition(value: number): number {
  return Math.min(100, Math.max(0, Math.round(value)));
}

export function clampSubtitlePaddingX(value: number): number {
  return Math.min(64, Math.max(0, Math.round(value)));
}

export function clampSubtitlePaddingY(value: number): number {
  return Math.min(64, Math.max(0, Math.round(value)));
}

export type SubtitleLanguageId = "en" | "id";

export function subtitleLanguageId(language: string | null, label?: string | null): SubtitleLanguageId | null {
  const code = (language ?? "").trim().toLowerCase().split(/[-_]/)[0];
  if (code === "en" || code === "eng") return "en";
  if (code === "id" || code === "ind") return "id";
  const text = `${language ?? ""} ${label ?? ""}`.toLowerCase();
  if (/\bindonesian\b|\bbahasa\b/.test(text)) return "id";
  if (/\benglish\b/.test(text)) return "en";
  return null;
}

export function subtitleLanguageName(id: SubtitleLanguageId): "English" | "Indonesian" {
  return id === "id" ? "Indonesian" : "English";
}

export function preferredEnglishSubtitleTrack(
  tracks: PanoramaSubtitleTrack[],
): PanoramaSubtitleTrack | null {
  return tracks.find(
    (track) => subtitleLanguageId(track.language, track.label) === "en",
  ) ?? null;
}

export function preferredEnglishAudioTrack(
  tracks: PanoramaAudioTrack[],
): PanoramaAudioTrack | null {
  return tracks.find(
    (track) => subtitleLanguageId(track.language, track.label) === "en",
  ) ?? null;
}

export function embeddedSubtitleSourceLabel(index: number, description: string | null): string {
  return `Embedded ${index + 1}${description ? ` · ${description}` : ""}`;
}

export type ExtraSubtitleTrack = {
  id: string;
  url: string;
  lang: string;
  label: string;
  origin: string;
};

const GENERIC_EXTRA_ORIGINS = new Set(["exclusive", "embedded in stream", "addon", "embedded"]);

type AddonSubtitle = {
  id?: unknown;
  url?: unknown;
  lang?: unknown;
  language?: unknown;
  label?: unknown;
  origin?: unknown;
};

type CoreSubtitleGroup = {
  addon?: {
    transportUrl?: unknown;
    manifest?: { name?: unknown };
  };
  request?: { base?: unknown };
  content?: {
    type?: "Loading" | "Ready" | "Err";
    content?: unknown;
  };
};

function optionalHref(value: unknown): string | null {
  const direct = optionalString(value);
  if (direct) return direct;
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return optionalString((value as { href?: unknown }).href);
}

function optionalTrackId(value: unknown): string | null {
  const named = optionalString(value);
  if (named) return named;
  return typeof value === "number" && Number.isFinite(value) ? String(value) : null;
}

function streamRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object") return null;
  if (Array.isArray(value)) {
    for (const entry of value) {
      const nested = streamRecord(entry);
      if (nested && Array.isArray(nested.subtitles)) return nested;
    }
    return null;
  }
  const record = value as Record<string, unknown>;
  if (Array.isArray(record.subtitles)) return record;
  const nestedStream = record.stream;
  if (nestedStream && nestedStream !== value) return streamRecord(nestedStream);
  return record;
}

function readySubtitleEntries(value: unknown): unknown[] {
  if (!value || typeof value !== "object") return [];
  if (Array.isArray(value)) return value;
  const record = value as Record<string, unknown>;
  if (Array.isArray(record.subtitles)) return record.subtitles;
  if (Array.isArray(record.Ready)) return record.Ready;
  if (record.type && record.type !== "Ready") return [];
  if (record.content !== undefined) return readySubtitleEntries(record.content);
  return [];
}

function isAddonSubtitle(value: unknown): value is AddonSubtitle {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const track = value as AddonSubtitle;
  return Boolean(optionalHref(track.url) && (optionalString(track.lang) || optionalString(track.language)));
}

function extraTrackLabel(label: unknown, lang: string): string {
  const named = optionalString(label);
  if (named && !/^https?:/i.test(named)) return named;
  return lang;
}

function extraTrackFromAddon(
  track: AddonSubtitle,
  origin: string,
  originByTransport?: Map<string, string>,
): ExtraSubtitleTrack | null {
  const url = optionalHref(track.url);
  const lang = optionalString(track.lang) ?? optionalString(track.language);
  if (!url || !lang) return null;
  const trackOrigin = extraSubtitleSourceLabel(track.origin, origin, originByTransport);
  return {
    id: optionalTrackId(track.id) ?? `${trackOrigin}:${url}`,
    url,
    lang,
    label: extraTrackLabel(track.label, lang),
    origin: trackOrigin,
  };
}

export function extraSubtitleSourceLabel(
  origin: unknown,
  fallbackAddonName?: string | null,
  originByTransport?: Map<string, string>,
): string {
  const name = optionalString(origin);
  if (name && originByTransport?.has(name)) return originByTransport.get(name) ?? name;
  if (!name || GENERIC_EXTRA_ORIGINS.has(name.toLowerCase()) || /^https?:/i.test(name)) {
    return optionalString(fallbackAddonName) ?? "Addon";
  }
  return name;
}

export function extraSubtitleTracksFromStream(
  stream: unknown,
  origin: string,
  originByTransport?: Map<string, string>,
): ExtraSubtitleTrack[] {
  const record = streamRecord(stream);
  const subtitles = record?.subtitles;
  if (!Array.isArray(subtitles)) return [];
  return uniqueExtraSubtitleTracks(subtitles.flatMap((entry) => {
    if (!isAddonSubtitle(entry)) return [];
    const track = extraTrackFromAddon(entry, origin, originByTransport);
    return track ? [track] : [];
  }));
}

export function extraSubtitleTracksFromCore(
  subtitles: unknown,
  originByTransport?: Map<string, string>,
): ExtraSubtitleTrack[] {
  if (!Array.isArray(subtitles)) {
    if (subtitles && typeof subtitles === "object" && !Array.isArray(subtitles)) {
      const record = subtitles as { content?: unknown; subtitles?: unknown; request?: unknown; addon?: unknown };
      if (Array.isArray(record.content)) return extraSubtitleTracksFromCore(record.content, originByTransport);
      if (Array.isArray(record.subtitles)) return extraSubtitleTracksFromCore(record.subtitles, originByTransport);
      if (record.content || record.request || record.addon) {
        return extraSubtitleTracksFromCore([subtitles], originByTransport);
      }
    }
    return [];
  }

  return uniqueExtraSubtitleTracks(subtitles.flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    if (Array.isArray(entry)) {
      return extraSubtitleTracksFromCore(entry, originByTransport);
    }
    if (isAddonSubtitle(entry)) {
      const track = extraTrackFromAddon(
        entry,
        extraSubtitleSourceLabel(entry.origin, "Addon", originByTransport),
        originByTransport,
      );
      return track ? [track] : [];
    }

    const group = entry as CoreSubtitleGroup & { Ready?: unknown };
    const loadableType = group.content?.type;
    if (loadableType && loadableType !== "Ready") return [];
    const tracks = readySubtitleEntries(group.content ?? group.Ready);
    const transport = optionalString(group.addon?.transportUrl) ?? optionalString(group.request?.base);
    const origin = extraSubtitleSourceLabel(
      optionalString(group.addon?.manifest?.name) ?? transport,
      "Addon",
      originByTransport,
    );
    return tracks.flatMap((track) => {
      if (isAddonSubtitle(track)) {
        const mapped = extraTrackFromAddon(track, origin, originByTransport);
        return mapped ? [mapped] : [];
      }
      if (track && typeof track === "object" && !Array.isArray(track) && ("content" in track || "request" in track || "addon" in track)) {
        return extraSubtitleTracksFromCore([track], originByTransport);
      }
      return [];
    });
  }));
}

export function uniqueExtraSubtitleTracks(tracks: ExtraSubtitleTrack[]): ExtraSubtitleTrack[] {
  const seenUrls = new Set<string>();
  const seenIds = new Set<string>();
  const unique: ExtraSubtitleTrack[] = [];
  for (const track of tracks) {
    if (seenUrls.has(track.url)) continue;
    seenUrls.add(track.url);
    const id = track.id && !seenIds.has(track.id) ? track.id : `${track.origin}:${track.url}`;
    seenIds.add(id);
    unique.push({ ...track, id });
  }
  return unique;
}

export function isCurrentFilmRequest(
  request: number,
  currentRequest: number,
  requestedFilmId: string | null,
  responseFilmId: string | undefined,
): boolean {
  return request === currentRequest && Boolean(requestedFilmId) && requestedFilmId === responseFilmId;
}
