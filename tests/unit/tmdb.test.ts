import { afterEach, describe, expect, it, vi } from "vitest";
import {
  clearTmdbDetailsCache,
  enrichTmdbCatalogFilms,
  fetchTmdbDirectedMovies,
  fetchTmdbMovieDetails,
  fetchTmdbPopularMovies,
  fetchTmdbSearchMovies,
  fetchTmdbSearchPeople,
  filmOriginCountry,
  normalizeTmdbListMovie,
  normalizeTmdbMovieDetails,
  normalizeTmdbPerson,
  parseTmdbPublicId,
  tmdbImageUrl,
  tmdbPublicId,
  tmdbPersonPublicId,
} from "@/runtime/tmdb";

describe("TMDB normalization", () => {
  it("retains the director person ID for Discord links", () => {
    const movie = { id: 157336, title: "Interstellar", credits: { crew: [null, { id: 1, name: "Someone", job: "Producer" }, { id: 525, name: "Christopher Nolan", job: "Director" }] } };
    expect(normalizeTmdbMovieDetails(movie)).toMatchObject({ director: "Christopher Nolan", directorTmdbId: 525 });
    expect(normalizeTmdbMovieDetails({ ...movie, credits: { crew: [{ id: -1, name: "Christopher Nolan", job: "Director" }] } })?.directorTmdbId).toBeNull();
  });
  it("maps list movies to Panorama films with tmdb public ids", () => {
    expect(normalizeTmdbListMovie({
      id: 550,
      title: "Fight Club",
      overview: "An insomniac office worker...",
      release_date: "1999-10-15",
      poster_path: "/poster.jpg",
      backdrop_path: "/back.jpg",
      vote_average: 8.433,
      vote_count: 2485,
      original_title: "Fight Club",
    })).toEqual({
      id: "tmdb:550",
      type: "movie",
      name: "Fight Club",
      year: "1999",
      director: null,
      originCountry: null,
      rating: 8.433,
      ratingCount: 2485,
      posterUrl: "https://image.tmdb.org/t/p/w500/poster.jpg",
      landscapeUrl: "https://image.tmdb.org/t/p/w1280/back.jpg",
      logoUrl: null,
      description: "An insomniac office worker...",
    });
  });

  it("rejects incomplete list movies and missing image paths", () => {
    expect(normalizeTmdbListMovie({ id: 1 })).toBeNull();
    expect(normalizeTmdbListMovie({ title: "No id" })).toBeNull();
    expect(tmdbImageUrl(null, "w500")).toBeNull();
    expect(tmdbImageUrl("poster.jpg", "w500")).toBeNull();
    expect(parseTmdbPublicId("tmdb:550")).toBe(550);
    expect(parseTmdbPublicId("tt0137523")).toBeNull();
    expect(tmdbPublicId(550)).toBe("tmdb:550");
  });

  it("maps people with stable ids, departments, and profile images", () => {
    expect(normalizeTmdbPerson({
      id: 240,
      name: "Jean-Luc Godard",
      known_for_department: "Directing",
      profile_path: "/godard.jpg",
    })).toEqual({
      id: "tmdb-person:240",
      name: "Jean-Luc Godard",
      department: "Directing",
      profileUrl: "https://image.tmdb.org/t/p/w500/godard.jpg",
    });
    expect(normalizeTmdbPerson({ id: 241, name: "Coletta Godard" })).toEqual({
      id: "tmdb-person:241",
      name: "Coletta Godard",
      department: null,
      profileUrl: null,
    });
    expect(normalizeTmdbPerson({ id: 1 })).toBeNull();
    expect(normalizeTmdbPerson({ name: "No id" })).toBeNull();
    expect(tmdbPersonPublicId(240)).toBe("tmdb-person:240");
  });

  it("fills details from credits and keeps films without an IMDb id", () => {
    expect(normalizeTmdbMovieDetails({
      id: 11,
      title: "Untitled",
      original_title: "Titre original",
      release_date: "2024-01-01",
      runtime: 102,
      genres: [{ name: "Drama" }],
      credits: { crew: [{ job: "Writer", name: "A Writer" }, { job: "Director", name: "Celine Song" }] },
      external_ids: { imdb_id: null },
      images: {
        logos: [
          { file_path: "/untagged-logo.png", iso_639_1: null },
          { file_path: "/english-logo.png", iso_639_1: "en" },
        ],
      },
    })).toMatchObject({
      id: "tmdb:11",
      director: "Celine Song",
      runtime: "102 min",
      genres: ["Drama"],
      tmdbId: 11,
      imdbId: null,
      originalTitle: "Titre original",
      logoUrl: "https://image.tmdb.org/t/p/w500/english-logo.png",
    });
  });

  it("selects the origin-country full onscreen alternative title", () => {
    expect(normalizeTmdbMovieDetails({
      id: 85927,
      title: "Masculin Féminin",
      original_title: "Masculin féminin",
      origin_country: ["FR"],
      alternative_titles: {
        titles: [
          { iso_3166_1: "FR", title: "Masculin, féminin", type: "short title" },
          { iso_3166_1: "FR", title: "Masculin féminin: 15 faits précis", type: "Full Onscreen Title" },
          { iso_3166_1: "DE", title: "Masculin - Feminin oder: Die Kinder von Coca Cola", type: "West Germany Title" },
        ],
      },
    })?.alternativeTitle).toBe("Masculin féminin: 15 faits précis");
  });

  it("resolves a valid IMDb id from TMDB external ids", () => {
    expect(normalizeTmdbMovieDetails({
      id: 550,
      title: "Fight Club",
      credits: { crew: [{ job: "Director", name: "David Fincher" }] },
      external_ids: { imdb_id: "tt0137523" },
    })?.imdbId).toBe("tt0137523");
    expect(normalizeTmdbMovieDetails({
      id: 550,
      title: "Fight Club",
      external_ids: { imdb_id: "not-imdb" },
    })?.imdbId).toBeNull();
  });

  it("keeps US origin countries hidden and names foreign ones", () => {
    expect(filmOriginCountry(["US"])).toBeNull();
    expect(filmOriginCountry(["US", "GB"])).toBeNull();
    expect(filmOriginCountry(["FR"])).toBe("France");
    expect(filmOriginCountry(null, [{ iso_3166_1: "JP" }])).toBe("Japan");
  });
});

describe("TMDB fetch", () => {
  afterEach(() => {
    clearTmdbDetailsCache();
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
  });

  it("throws when the public API key is missing", async () => {
    vi.stubEnv("NEXT_PUBLIC_TMDB_API_KEY", "");
    await expect(fetchTmdbPopularMovies(1)).rejects.toThrow("TMDB API key is missing.");
  });

  it("requests popular movies with the API key", async () => {
    vi.stubEnv("NEXT_PUBLIC_TMDB_API_KEY", "test-key");
    const fetchMock = vi.fn(async () => ({
      ok: true,
      json: async () => ({
        page: 1,
        total_pages: 2,
        results: [{ id: 550, title: "Fight Club", release_date: "1999-10-15" }],
      }),
    }));
    vi.stubGlobal("fetch", fetchMock);

    const page = await fetchTmdbPopularMovies(1);
    expect(page.items).toHaveLength(1);
    expect(page.items[0]?.id).toBe("tmdb:550");
    expect(page.page).toBe(1);
    expect(page.totalPages).toBe(2);
    expect(String(fetchMock.mock.calls)).toContain("/movie/popular");
    expect(String(fetchMock.mock.calls)).toContain("api_key=test-key");
  });

  it("searches movies and loads details with credits", async () => {
    vi.stubEnv("NEXT_PUBLIC_TMDB_API_KEY", "test-key");
    const fetchMock = vi.fn(async (input: URL | string) => {
      const url = String(input);
      if (url.includes("/search/movie")) {
        return {
          ok: true,
          json: async () => ({ page: 1, total_pages: 1, results: [{ id: 550, title: "Fight Club" }] }),
        };
      }
      return {
        ok: true,
        json: async () => ({
          id: 550,
          title: "Fight Club",
          runtime: 139,
          credits: { crew: [{ job: "Director", name: "David Fincher" }, { job: "Director", name: "Someone Else" }] },
          external_ids: { imdb_id: "tt0137523" },
        }),
      };
    });
    vi.stubGlobal("fetch", fetchMock);

    const search = await fetchTmdbSearchMovies("Fight Club", 1);
    expect(search.items[0]?.name).toBe("Fight Club");
    const details = await fetchTmdbMovieDetails(550);
    expect(details.director).toBe("David Fincher, Someone Else");
    expect(details.imdbId).toBe("tt0137523");
    expect(String(fetchMock.mock.calls)).toContain("append_to_response=credits%2Cexternal_ids%2Calternative_titles%2Cimages");
    expect(String(fetchMock.mock.calls)).toContain("include_image_language=en%2Cnull");
    expect(String(fetchMock.mock.calls)).not.toContain("videos");
  });

  it("searches people and removes duplicate or malformed entries", async () => {
    vi.stubEnv("NEXT_PUBLIC_TMDB_API_KEY", "test-key");
    const fetchMock = vi.fn(async () => ({
      ok: true,
      json: async () => ({
        page: 2,
        total_pages: 4,
        results: [
          { id: 240, name: "Jean-Luc Godard", known_for_department: "Directing", profile_path: "/godard.jpg" },
          { id: 240, name: "Jean-Luc Godard", known_for_department: "Directing" },
          { id: 241 },
        ],
      }),
    }));
    vi.stubGlobal("fetch", fetchMock);

    const page = await fetchTmdbSearchPeople("Godard", 2);
    expect(page).toMatchObject({ page: 2, totalPages: 4 });
    expect(page.items).toHaveLength(1);
    expect(page.items[0]?.name).toBe("Jean-Luc Godard");
    expect(String(fetchMock.mock.calls)).toContain("/search/person");
    expect(String(fetchMock.mock.calls)).toContain("include_adult=false");
  });

  it("loads only exact-name movie directing credits and removes duplicates", async () => {
    vi.stubEnv("NEXT_PUBLIC_TMDB_API_KEY", "test-key");
    const fetchMock = vi.fn(async () => ({
      ok: true,
      json: async () => ({
        id: 10,
        crew: [
          { id: 100, title: "In a Lonely Place", release_date: "1950-05-17", job: "Director" },
          { id: 101, title: "Johnny Guitar", release_date: "1954-05-27", job: "Producer" },
          { id: 100, title: "In a Lonely Place", release_date: "1950-05-17", job: "Director" },
        ],
      }),
    }));
    vi.stubGlobal("fetch", fetchMock);

    const films = await fetchTmdbDirectedMovies("nicholas ray", [
      { id: "tmdb-person:10", name: "Nicholas Ray", department: "Directing", profileUrl: null },
      { id: "tmdb-person:11", name: "Nicholas Ray", department: "Acting", profileUrl: null },
      { id: "tmdb-person:12", name: "Nicholas Ray Jr.", department: "Directing", profileUrl: null },
    ]);

    expect(films).toEqual([expect.objectContaining({
      id: "tmdb:100",
      name: "In a Lonely Place",
      director: "Nicholas Ray",
    })]);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(String(fetchMock.mock.calls)).toContain("/person/10/movie_credits");
  });

  it("does not load directing credits for a partial person name", async () => {
    vi.stubEnv("NEXT_PUBLIC_TMDB_API_KEY", "test-key");
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    const films = await fetchTmdbDirectedMovies("Nicholas", [
      { id: "tmdb-person:10", name: "Nicholas Ray", department: "Directing", profileUrl: null },
    ]);

    expect(films).toEqual([]);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("fills catalog directors and foreign origin from details", async () => {
    vi.stubEnv("NEXT_PUBLIC_TMDB_API_KEY", "test-key");
    vi.stubGlobal("fetch", vi.fn(async () => ({
      ok: true,
      json: async () => ({
        id: 129,
        title: "Spirited Away",
        origin_country: ["JP"],
        credits: { crew: [{ job: "Director", name: "Hayao Miyazaki" }] },
      external_ids: { imdb_id: "tt0245429" },
      images: { logos: [{ file_path: "/spirited-away-logo.png", iso_639_1: "en" }] },
      }),
    })));

    const items = await enrichTmdbCatalogFilms([{
      id: "tmdb:129",
      type: "movie",
      name: "Spirited Away",
      year: "2001",
      director: null,
      originCountry: null,
      rating: null,
      ratingCount: null,
      posterUrl: null,
      landscapeUrl: null,
      logoUrl: null,
      description: null,
    }]);
    expect(items[0]?.director).toBe("Hayao Miyazaki");
    expect(items[0]?.originCountry).toBe("Japan");
    expect(items[0]?.logoUrl).toBe("https://image.tmdb.org/t/p/w500/spirited-away-logo.png");
  });
});
