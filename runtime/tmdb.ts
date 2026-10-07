import type { PanoramaFilm, PanoramaFilmDetails, PanoramaPerson } from "./types";

const TMDB_API = "https://api.themoviedb.org/3";
const TMDB_IMAGE = "https://image.tmdb.org/t/p";
export const TMDB_FILM_PREFIX = "tmdb:";
export const TMDB_PERSON_PREFIX = "tmdb-person:";
export const NO_IMDB_SOURCES_MESSAGE =
  "This film has no IMDb identifier, so Stremio addons cannot load sources.";

type TmdbListMovie = {
  id?: unknown;
  title?: unknown;
  name?: unknown;
  overview?: unknown;
  release_date?: unknown;
  poster_path?: unknown;
  backdrop_path?: unknown;
  vote_average?: unknown;
  vote_count?: unknown;
  original_title?: unknown;
  origin_country?: unknown;
  production_countries?: unknown;
};

type TmdbPaged = {
  page?: unknown;
  total_pages?: unknown;
  results?: unknown;
};

type TmdbPerson = {
  id?: unknown;
  name?: unknown;
  known_for_department?: unknown;
  profile_path?: unknown;
};

type TmdbCrew = {
  id?: unknown;
  job?: unknown;
  name?: unknown;
};

type TmdbPersonMovieCredits = {
  crew?: unknown;
};

type TmdbAlternativeTitle = {
  iso_3166_1?: unknown;
  title?: unknown;
  type?: unknown;
};

type TmdbImage = {
  file_path?: unknown;
  iso_639_1?: unknown;
};

type TmdbDetails = TmdbListMovie & {
  runtime?: unknown;
  genres?: unknown;
  credits?: { crew?: unknown };
  external_ids?: { imdb_id?: unknown };
  alternative_titles?: { titles?: unknown };
  images?: { logos?: unknown };
};

export type TmdbCatalogPage = {
  items: PanoramaFilm[];
  page: number;
  totalPages: number;
};

export type TmdbPeoplePage = {
  items: PanoramaPerson[];
  page: number;
  totalPages: number;
};

export type TmdbMovieDetails = PanoramaFilmDetails & {
  tmdbId: number;
  imdbId: string | null;
};

const detailsCache = new Map<number, TmdbMovieDetails>();

export function clearTmdbDetailsCache(): void {
  detailsCache.clear();
}

function optionalString(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : null;
}

function optionalNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function tmdbPublicId(tmdbId: number): string {
  return `${TMDB_FILM_PREFIX}${tmdbId}`;
}

export function tmdbPersonPublicId(tmdbId: number): string {
  return `${TMDB_PERSON_PREFIX}${tmdbId}`;
}

function parseTmdbPersonPublicId(id: string): number | null {
  if (!id.startsWith(TMDB_PERSON_PREFIX)) return null;
  const value = Number(id.slice(TMDB_PERSON_PREFIX.length));
  return Number.isInteger(value) && value > 0 ? value : null;
}

export function parseTmdbPublicId(id: string): number | null {
  if (!id.startsWith(TMDB_FILM_PREFIX)) return null;
  const value = Number(id.slice(TMDB_FILM_PREFIX.length));
  return Number.isInteger(value) && value > 0 ? value : null;
}

export function tmdbImageUrl(path: unknown, size: "w500" | "w1280"): string | null {
  const value = optionalString(path);
  if (!value || !value.startsWith("/")) return null;
  return `${TMDB_IMAGE}/${size}${value}`;
}

function tmdbApiKey(): string {
  const key = process.env.NEXT_PUBLIC_TMDB_API_KEY?.trim() ?? "";
  if (!key) throw new Error("TMDB API key is missing.");
  return key;
}

function filmTitle(movie: TmdbListMovie): string | null {
  return optionalString(movie.title) ?? optionalString(movie.name);
}

function filmYear(releaseDate: unknown): string | null {
  const value = optionalString(releaseDate);
  const year = value?.match(/^(?:19|20)\d{2}/)?.[0];
  return year ?? null;
}

function filmRating(voteAverage: unknown): number | null {
  const value = optionalNumber(voteAverage);
  if (value === null || value <= 0) return null;
  return Math.min(10, value);
}

function filmRatingCount(voteCount: unknown): number | null {
  const value = optionalNumber(voteCount);
  if (value === null || value <= 0) return null;
  return Math.floor(value);
}

function filmDirector(crew: unknown): string | null {
  if (!Array.isArray(crew)) return null;
  const names = crew.flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const member = entry as TmdbCrew;
    if (optionalString(member.job) !== "Director") return [];
    const name = optionalString(member.name);
    return name ? [name] : [];
  });
  return names.length > 0 ? [...new Set(names)].join(", ") : null;
}

function filmRuntime(runtime: unknown): string | null {
  const minutes = optionalNumber(runtime);
  if (minutes === null || minutes <= 0) return null;
  return `${Math.round(minutes)} min`;
}

function filmGenres(genres: unknown): string[] {
  if (!Array.isArray(genres)) return [];
  return genres.flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const name = optionalString((entry as { name?: unknown }).name);
    return name ? [name] : [];
  });
}

function filmImdbId(imdbId: unknown): string | null {
  const value = optionalString(imdbId);
  return value && /^tt\d+$/.test(value) ? value : null;
}

function regionName(code: string): string {
  try {
    return new Intl.DisplayNames(["en"], { type: "region" }).of(code) ?? code;
  } catch {
    return code;
  }
}

function filmOriginCountryCodes(originCountry: unknown, productionCountries?: unknown): string[] {
  const codes = [
    ...(Array.isArray(originCountry) ? originCountry : []),
    ...(Array.isArray(productionCountries)
      ? productionCountries.map((entry) => (
        entry && typeof entry === "object" ? (entry as { iso_3166_1?: unknown }).iso_3166_1 : null
      ))
      : []),
  ].flatMap((code) => {
    const value = optionalString(code)?.toUpperCase();
    return value && /^[A-Z]{2}$/.test(value) ? [value] : [];
  });
  return [...new Set(codes)];
}

export function filmOriginCountry(originCountry: unknown, productionCountries?: unknown): string | null {
  const unique = filmOriginCountryCodes(originCountry, productionCountries);
  if (unique.length === 0 || unique.includes("US")) return null;
  return regionName(unique[0] ?? "");
}

function filmAlternativeTitle(
  alternativeTitles: unknown,
  originCountry: unknown,
  productionCountries: unknown,
): string | null {
  if (!Array.isArray(alternativeTitles)) return null;
  const originCodes = new Set(filmOriginCountryCodes(originCountry, productionCountries));
  for (const entry of alternativeTitles) {
    if (!entry || typeof entry !== "object") continue;
    const title = entry as TmdbAlternativeTitle;
    const countryCode = optionalString(title.iso_3166_1)?.toUpperCase();
    const type = optionalString(title.type)?.toLowerCase();
    if (!countryCode || !originCodes.has(countryCode) || type !== "full onscreen title") continue;
    const value = optionalString(title.title);
    if (value) return value;
  }
  return null;
}

function filmLogoUrl(logos: unknown): string | null {
  if (!Array.isArray(logos)) return null;
  const valid = logos.flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const image = entry as TmdbImage;
    const url = tmdbImageUrl(image.file_path, "w500");
    return url ? [{ language: optionalString(image.iso_639_1), url }] : [];
  });
  return valid.find((image) => image.language === "en")?.url
    ?? valid.find((image) => image.language === null)?.url
    ?? valid[0]?.url
    ?? null;
}

export function normalizeTmdbListMovie(movie: TmdbListMovie): PanoramaFilm | null {
  const tmdbId = optionalNumber(movie.id);
  const name = filmTitle(movie);
  if (tmdbId === null || tmdbId <= 0 || !Number.isInteger(tmdbId) || !name) return null;

  return {
    id: tmdbPublicId(tmdbId),
    type: "movie",
    name,
    year: filmYear(movie.release_date),
    director: null,
    originCountry: filmOriginCountry(movie.origin_country, movie.production_countries),
    rating: filmRating(movie.vote_average),
    ratingCount: filmRatingCount(movie.vote_count),
    posterUrl: tmdbImageUrl(movie.poster_path, "w500"),
    landscapeUrl: tmdbImageUrl(movie.backdrop_path, "w1280"),
    logoUrl: null,
    description: optionalString(movie.overview),
  };
}

export function normalizeTmdbPerson(person: TmdbPerson): PanoramaPerson | null {
  const tmdbId = optionalNumber(person.id);
  const name = optionalString(person.name);
  if (tmdbId === null || tmdbId <= 0 || !Number.isInteger(tmdbId) || !name) return null;

  return {
    id: tmdbPersonPublicId(tmdbId),
    name,
    department: optionalString(person.known_for_department),
    profileUrl: tmdbImageUrl(person.profile_path, "w500"),
  };
}

export function normalizeTmdbMovieDetails(movie: TmdbDetails): TmdbMovieDetails | null {
  const film = normalizeTmdbListMovie(movie);
  if (!film) return null;
  const tmdbId = parseTmdbPublicId(film.id);
  if (tmdbId === null) return null;

  return {
    ...film,
    originalTitle: optionalString(movie.original_title),
    alternativeTitle: filmAlternativeTitle(
      movie.alternative_titles?.titles,
      movie.origin_country,
      movie.production_countries,
    ),
    director: filmDirector(movie.credits?.crew),
    directorTmdbId: Array.isArray(movie.credits?.crew) ? movie.credits.crew.flatMap((entry: TmdbCrew) => {
      const id = optionalNumber(entry?.id);
      return optionalString(entry?.job) === "Director" && optionalString(entry.name) && id !== null && Number.isSafeInteger(id) && id > 0 ? [id] : [];
    })[0] ?? null : null,
    originCountry: filmOriginCountry(movie.origin_country, movie.production_countries),
    runtime: filmRuntime(movie.runtime),
    genres: filmGenres(movie.genres),
    logoUrl: filmLogoUrl(movie.images?.logos),
    tmdbId,
    imdbId: filmImdbId(movie.external_ids?.imdb_id),
  };
}

async function tmdbGet<T>(path: string, params: Record<string, string> = {}): Promise<T> {
  const url = new URL(`${TMDB_API}${path}`);
  url.searchParams.set("api_key", tmdbApiKey());
  for (const [name, value] of Object.entries(params)) url.searchParams.set(name, value);
  const response = await fetch(url, { signal: AbortSignal.timeout(8000) });
  if (!response.ok) throw new Error(`TMDB request failed (${response.status}).`);
  return await response.json() as T;
}

function catalogPageFromPayload(payload: TmdbPaged): TmdbCatalogPage {
  const results = Array.isArray(payload.results) ? payload.results : [];
  const items = results.flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const film = normalizeTmdbListMovie(entry as TmdbListMovie);
    return film ? [film] : [];
  });
  const page = optionalNumber(payload.page) ?? 1;
  const totalPages = optionalNumber(payload.total_pages) ?? 1;
  return { items, page, totalPages };
}

function peoplePageFromPayload(payload: TmdbPaged): TmdbPeoplePage {
  const results = Array.isArray(payload.results) ? payload.results : [];
  const seen = new Set<string>();
  const items = results.flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const person = normalizeTmdbPerson(entry as TmdbPerson);
    if (!person || seen.has(person.id)) return [];
    seen.add(person.id);
    return [person];
  });
  const page = optionalNumber(payload.page) ?? 1;
  const totalPages = optionalNumber(payload.total_pages) ?? 1;
  return { items, page, totalPages };
}

export async function fetchTmdbPopularMovies(page: number): Promise<TmdbCatalogPage> {
  return catalogPageFromPayload(await tmdbGet<TmdbPaged>("/movie/popular", { page: String(page) }));
}

export async function fetchTmdbSearchMovies(query: string, page: number): Promise<TmdbCatalogPage> {
  return catalogPageFromPayload(await tmdbGet<TmdbPaged>("/search/movie", {
    query,
    page: String(page),
    include_adult: "false",
  }));
}

export async function fetchTmdbSearchPeople(query: string, page: number): Promise<TmdbPeoplePage> {
  return peoplePageFromPayload(await tmdbGet<TmdbPaged>("/search/person", {
    query,
    page: String(page),
    include_adult: "false",
  }));
}

export async function fetchTmdbDirectedMovies(
  query: string,
  people: PanoramaPerson[],
): Promise<PanoramaFilm[]> {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const directors = people.filter((person) => (
    person.department === "Directing" &&
    person.name.toLocaleLowerCase() === normalizedQuery &&
    parseTmdbPersonPublicId(person.id) !== null
  ));
  const credits = await Promise.all(directors.map(async (director) => ({
    director,
    payload: await tmdbGet<TmdbPersonMovieCredits>(
      `/person/${parseTmdbPersonPublicId(director.id)}/movie_credits`,
      {},
    ),
  })));
  const seen = new Set<string>();
  return credits.flatMap(({ director, payload }) => (
    Array.isArray(payload.crew) ? payload.crew : []
  ).flatMap((entry) => {
    if (!entry || typeof entry !== "object" || optionalString((entry as TmdbCrew).job) !== "Director") return [];
    const film = normalizeTmdbListMovie(entry as TmdbListMovie);
    if (!film || seen.has(film.id)) return [];
    seen.add(film.id);
    return [{ ...film, director: director.name }];
  }));
}

export async function fetchTmdbMovieDetails(tmdbId: number): Promise<TmdbMovieDetails> {
  const cached = detailsCache.get(tmdbId);
  if (cached) return cached;
  const details = normalizeTmdbMovieDetails(
    await tmdbGet<TmdbDetails>(`/movie/${tmdbId}`, {
      append_to_response: "credits,external_ids,alternative_titles,images",
      include_image_language: "en,null",
    }),
  );
  if (!details) throw new Error("Unable to load film details. Check your connection and try again.");
  detailsCache.set(tmdbId, details);
  return details;
}

export async function enrichTmdbCatalogFilms(films: PanoramaFilm[]): Promise<PanoramaFilm[]> {
  return Promise.all(films.map(async (film) => {
    const tmdbId = parseTmdbPublicId(film.id);
    if (!tmdbId) return film;
    try {
      const details = await fetchTmdbMovieDetails(tmdbId);
      return {
        ...film,
        director: details.director,
        originCountry: details.originCountry,
        logoUrl: details.logoUrl,
      };
    } catch {
      return film;
    }
  }));
}
