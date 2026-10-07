export type DiscordPlayback = {
  filmId: string;
  title: string;
  year?: string | null;
  directorTmdbId?: number | null;
  director: string | null;
  artwork: string | null;
  time: number;
  duration: number;
  state: "playing" | "paused" | "buffering";
};

export type DiscordSettings = { enabled: boolean; available: boolean };

export function discordArtwork(...urls: Array<string | null | undefined>): string | null {
  for (const value of urls) {
    if (!value || value.length > 2048) continue;
    try {
      const url = new URL(value);
      if (url.protocol === "https:" && url.hostname === "image.tmdb.org" && !url.port && !url.username && !url.password && url.pathname.startsWith("/t/p/") && !url.search && !url.hash) return url.href;
    } catch {}
  }
  return null;
}

export function parseDiscordPlayback(value: unknown): DiscordPlayback | null {
  if (value === null) return null;
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid Discord playback");
  const input = value as Record<string, unknown>;
  if (Object.keys(input).length !== 7 + Number(Object.hasOwn(input, "year")) + Number(Object.hasOwn(input, "directorTmdbId")) || typeof input.filmId !== "string" || input.filmId.length > 128 || !input.filmId ||
    typeof input.title !== "string" || !input.title.trim() || input.title.length > 512 ||
    (input.year !== undefined && input.year !== null && (typeof input.year !== "string" || !/^\d{4}$/.test(input.year))) ||
    (input.directorTmdbId !== undefined && input.directorTmdbId !== null && (typeof input.directorTmdbId !== "number" || !Number.isSafeInteger(input.directorTmdbId) || input.directorTmdbId <= 0)) ||
    (input.director !== null && (typeof input.director !== "string" || !input.director.trim() || input.director.length > 512)) ||
    (input.artwork !== null && (typeof input.artwork !== "string" || !discordArtwork(input.artwork))) ||
    typeof input.time !== "number" || !Number.isFinite(input.time) || input.time < 0 || input.time > 1e8 ||
    typeof input.duration !== "number" || !Number.isFinite(input.duration) || input.duration < 0 || input.duration > 1e8 ||
    !["playing", "paused", "buffering"].includes(input.state as string)) throw new Error("Invalid Discord playback");
  return { ...(Object.hasOwn(input, "directorTmdbId") ? { directorTmdbId: input.directorTmdbId as number | null } : {}), filmId: input.filmId, title: input.title.trim(), ...(Object.hasOwn(input, "year") ? { year: input.year as string | null } : {}), director: typeof input.director === "string" ? input.director.trim() : null, artwork: input.artwork as string | null, time: input.time, duration: input.duration, state: input.state as DiscordPlayback["state"] };
}
