import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  fetchDirectedMovies: vi.fn(),
  enrichFilms: vi.fn(),
  fetchMovies: vi.fn(),
  fetchPeople: vi.fn(),
}));

vi.mock("@/runtime/tmdb", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/runtime/tmdb")>(),
  fetchTmdbDirectedMovies: mocks.fetchDirectedMovies,
  enrichTmdbCatalogFilms: mocks.enrichFilms,
  fetchTmdbSearchMovies: mocks.fetchMovies,
  fetchTmdbSearchPeople: mocks.fetchPeople,
}));

import { StremioCoreRuntime } from "@/runtime/stremio-core-runtime";
import type { PanoramaFilm, PanoramaPerson } from "@/runtime/types";

const film: PanoramaFilm = {
  id: "tmdb:1",
  type: "movie",
  name: "Godard Mon Amour",
  year: "2017",
  director: null,
  originCountry: "France",
  rating: null,
  ratingCount: null,
  posterUrl: null,
  landscapeUrl: null,
  logoUrl: null,
  description: null,
};

const person: PanoramaPerson = {
  id: "tmdb-person:240",
  name: "Jean-Luc Godard",
  department: "Directing",
  profileUrl: null,
};

const directedFilm: PanoramaFilm = {
  ...film,
  id: "tmdb:2",
  name: "In a Lonely Place",
  year: "1950",
  director: "Nicholas Ray",
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("search runtime", () => {
  beforeEach(() => {
    mocks.enrichFilms.mockReset();
    mocks.fetchMovies.mockReset();
    mocks.fetchPeople.mockReset();
    mocks.enrichFilms.mockImplementation(async (items: PanoramaFilm[]) => items);
    mocks.fetchDirectedMovies.mockResolvedValue([]);
    mocks.fetchMovies.mockResolvedValue({ items: [film], page: 1, totalPages: 1 });
    mocks.fetchPeople.mockResolvedValue({ items: [person], page: 1, totalPages: 1 });
  });

  it("loads films and people concurrently under one request generation", async () => {
    const runtime = new StremioCoreRuntime();
    await runtime.searchMovies("  Godard  ");

    expect(mocks.fetchMovies).toHaveBeenCalledWith("Godard", 1);
    expect(mocks.fetchPeople).toHaveBeenCalledWith("Godard", 1);
    expect(runtime.getSnapshot().catalog).toMatchObject({ mode: "search", query: "Godard", status: "ready" });
    expect(runtime.getSnapshot().people).toMatchObject({
      query: "Godard",
      requestId: runtime.getSnapshot().catalog.requestId,
      status: "ready",
    });
  });

  it("places exact-director films before title matches without duplicates", async () => {
    mocks.fetchDirectedMovies.mockResolvedValue([directedFilm, film]);
    const runtime = new StremioCoreRuntime();

    await runtime.searchMovies("Nicholas Ray");

    expect(runtime.getSnapshot().catalog.page.items).toEqual([directedFilm, film]);
  });

  it("keeps people usable when the movie endpoint fails", async () => {
    mocks.fetchMovies.mockRejectedValue(new Error("movie failure"));
    const runtime = new StremioCoreRuntime();
    await runtime.searchMovies("Godard");

    expect(runtime.getSnapshot().catalog).toMatchObject({ status: "error", error: "movie failure" });
    expect(runtime.getSnapshot().people).toMatchObject({ status: "ready", error: null });
    expect(runtime.getSnapshot().people.page.items).toEqual([person]);
  });

  it("rejects late film and people responses from an older query", async () => {
    const firstMovies = deferred<{ items: PanoramaFilm[]; page: number; totalPages: number }>();
    const firstPeople = deferred<{ items: PanoramaPerson[]; page: number; totalPages: number }>();
    mocks.fetchMovies.mockImplementation((query: string) => query === "First" ? firstMovies.promise : Promise.resolve({ items: [film], page: 1, totalPages: 1 }));
    mocks.fetchPeople.mockImplementation((query: string) => query === "First" ? firstPeople.promise : Promise.resolve({ items: [person], page: 1, totalPages: 1 }));
    const runtime = new StremioCoreRuntime();

    const firstSearch = runtime.searchMovies("First");
    await runtime.searchMovies("Second");
    firstMovies.resolve({ items: [{ ...film, name: "Stale film" }], page: 1, totalPages: 1 });
    firstPeople.resolve({ items: [{ ...person, name: "Stale person" }], page: 1, totalPages: 1 });
    await firstSearch;

    expect(runtime.getSnapshot().catalog.query).toBe("Second");
    expect(runtime.getSnapshot().catalog.page.items[0]?.name).toBe("Godard Mon Amour");
    expect(runtime.getSnapshot().people.page.items[0]?.name).toBe("Jean-Luc Godard");
  });

  it("deduplicates people across independently loaded pages", async () => {
    mocks.fetchPeople
      .mockResolvedValueOnce({ items: [person], page: 1, totalPages: 2 })
      .mockResolvedValueOnce({ items: [person, { ...person, id: "tmdb-person:241", name: "Agnès Godard" }], page: 2, totalPages: 2 });
    const runtime = new StremioCoreRuntime();
    await runtime.searchMovies("Godard");
    await runtime.loadNextPeoplePage();

    expect(runtime.getSnapshot().people.page.items.map((item) => item.name)).toEqual([
      "Jean-Luc Godard",
      "Agnès Godard",
    ]);
    expect(runtime.getSnapshot().catalog.page.items).toEqual([film]);
  });
});
