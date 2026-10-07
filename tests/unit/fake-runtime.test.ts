import { describe, expect, it, vi } from "vitest";
import { FakeRuntime } from "@/runtime/fake-runtime";

describe("runtime state transitions", () => {
  it("moves through catalog, account, addon, and pagination states", async () => {
    const runtime = new FakeRuntime();
    const listener = vi.fn();
    runtime.subscribe(listener);

    await runtime.initialize();
    expect(runtime.getSnapshot().catalog.page.items).toHaveLength(9);
    expect(runtime.getSnapshot().service.status).toBe("offline");

    await runtime.login("viewer@example.com", "password");
    expect(runtime.getSnapshot().account).toMatchObject({
      status: "signedIn",
      email: "viewer@example.com",
    });
    expect(runtime.getSnapshot().addons).toMatchObject({ status: "ready", count: 4 });

    await runtime.loadNextPage();
    expect(runtime.getSnapshot().catalog.page.items).toHaveLength(18);
    expect(runtime.getSnapshot().catalog.page.hasMore).toBe(false);
    expect(listener).toHaveBeenCalled();
  });

  it("reports staged playback preparation and clears it when video is ready", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();
    await runtime.login("viewer@example.com", "password");
    expect(await runtime.checkService()).toBe("online");
    await runtime.openFilmDetails("tmdb:100");
    await expect(runtime.preparePlayback("source-1-1")).resolves.toBeUndefined();
    expect(runtime.getSnapshot().player.status).toBe("idle");
    await runtime.startPlayback("source-1-1");

    expect(runtime.getSnapshot().player).toMatchObject({
      status: "preparing",
      stage: "resolvingSource",
    });

    await runtime.attachPlayer();
    expect(runtime.getSnapshot().player).toMatchObject({
      status: "ready",
      stage: null,
    });
    expect(runtime.getSnapshot().player.audio).toMatchObject({
      status: "ready",
      selectedId: "audio-1",
    });
    expect(runtime.getSnapshot().player.audio.tracks.map((track) => track.label)).toEqual(["English", "Français"]);
    expect(runtime.getSnapshot().player.subtitles.tracks.map((track) => track.sourceLabel)).toEqual([
      "Embedded 1",
      "OpenSubtitles v3",
      "Fixture addon",
      "OpenSubtitles v3",
    ]);
    expect(runtime.getSnapshot().player.subtitles.selectedId).toBe("subtitle-embedded-1");
    runtime.selectAudioTrack("audio-2");
    expect(runtime.getSnapshot().player.audio.selectedId).toBe("audio-2");
  });

  it("keeps volume within a playback session and resets the next session to 100%", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();
    await runtime.login("viewer@example.com", "password");
    await runtime.openFilmDetails("tmdb:100");
    await runtime.startPlayback("source-1-1");

    runtime.setPlaybackVolume(1.65);
    await runtime.switchPlaybackSource("source-1-1");
    expect(runtime.getSnapshot().player).toMatchObject({ volume: 1.65, muted: false });

    await runtime.retryPlayback();
    expect(runtime.getSnapshot().player).toMatchObject({ volume: 1.65, muted: false });

    await runtime.stopPlayback();
    await runtime.startPlayback("source-1-1");
    expect(runtime.getSnapshot().player).toMatchObject({ volume: 1, muted: false });
  });

  it("switches between full-catalog search and popular results", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();
    const popularRequest = runtime.getSnapshot().catalog.requestId;

    await runtime.searchMovies("  Aftersun  ");
    expect(runtime.getSnapshot().catalog).toMatchObject({
      mode: "search",
      query: "Aftersun",
      requestId: popularRequest + 1,
    });
    expect(runtime.getSnapshot().catalog.page.items.every((film) => film.name === "Aftersun")).toBe(true);

    await runtime.loadNextPage();
    expect(runtime.getSnapshot().catalog.page.hasMore).toBe(false);
    await runtime.clearSearch();
    expect(runtime.getSnapshot().catalog).toMatchObject({ mode: "popular", query: null });
    expect(runtime.getSnapshot().catalog.page.items).toHaveLength(9);
  });

  it("matches films by a director's full name but not a partial name", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();

    await runtime.searchMovies("Charlotte Wells");
    expect(runtime.getSnapshot().catalog.page.items).not.toHaveLength(0);
    expect(runtime.getSnapshot().catalog.page.items.every((film) => film.director === "Charlotte Wells")).toBe(true);

    await runtime.searchMovies("Charlotte");
    expect(runtime.getSnapshot().catalog.page.items).toHaveLength(0);
  });

  it("keeps cast and crew results and pagination independent from films", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();

    await runtime.searchMovies("Godard");
    expect(runtime.getSnapshot().catalog.page.items).toHaveLength(0);
    expect(runtime.getSnapshot().people).toMatchObject({
      query: "Godard",
      status: "ready",
      requestId: runtime.getSnapshot().catalog.requestId,
    });
    expect(runtime.getSnapshot().people.page.items).toHaveLength(6);

    await runtime.loadNextPeoplePage();
    expect(runtime.getSnapshot().people.page.items).toHaveLength(12);
    expect(runtime.getSnapshot().people.page.hasMore).toBe(false);
    expect(runtime.getSnapshot().catalog.page.items).toHaveLength(0);

    await runtime.clearSearch();
    expect(runtime.getSnapshot().people).toMatchObject({ query: null, status: "idle" });
    expect(runtime.getSnapshot().people.page.items).toHaveLength(0);
  });

  it("keeps signed-in progress for resume and supports restarting", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();
    await runtime.login("viewer@example.com", "password");
    await runtime.openFilmDetails("tmdb:100");
    await runtime.startPlayback("source-1-1", "restart");
    await runtime.attachPlayer();
    runtime.seekPlayback(2520);
    await runtime.stopPlayback();
    await runtime.openFilmDetails("tmdb:100");

    expect(runtime.getSnapshot().details.resume).toEqual({ available: true, offset: 2520, duration: 6120 });
    await runtime.startPlayback("source-1-1", "resume");
    expect(runtime.getSnapshot().player.time).toBe(2520);
    await runtime.startPlayback("source-1-1", "restart");
    expect(runtime.getSnapshot().player.time).toBe(0);
  });
});


describe("fake runtime watchlist", () => {
  it("saves and removes films independently and persists across reopening", async () => {
    const runtime = new FakeRuntime();
    await runtime.login("viewer@example.com", "password");
    await runtime.openFilmDetails("tmdb:100");
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: true, saved: false, pending: false });
    await runtime.setWatchlisted(true);
    expect(runtime.getSnapshot().details.watchlist.saved).toBe(true);
    await runtime.openFilmDetails("tmdb:101");
    expect(runtime.getSnapshot().details.watchlist.saved).toBe(false);
    await runtime.closeFilmDetails();
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: false, saved: false, pending: false });
    await runtime.openFilmDetails("tmdb:100");
    expect(runtime.getSnapshot().details.watchlist.saved).toBe(true);
    await runtime.setWatchlisted(false);
    expect(runtime.getSnapshot().details.watchlist.saved).toBe(false);
  });

  it("disables signed-out toggles and restores saved films after login", async () => {
    const runtime = new FakeRuntime();
    await runtime.openFilmDetails("tmdb:100");
    await runtime.setWatchlisted(true);
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: false, saved: false, pending: false });
    await runtime.login("viewer@example.com", "password");
    expect(runtime.getSnapshot().details.watchlist.available).toBe(true);
    await runtime.setWatchlisted(true);
    await runtime.logout();
    await runtime.setWatchlisted(false);
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: false, saved: false, pending: false });
    await runtime.login("viewer@example.com", "password");
    expect(runtime.getSnapshot().details.watchlist.saved).toBe(true);
    runtime.setStateForTest({ account: { status: "loggedOut", email: null, error: null } });
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: false, saved: false, pending: false });
    runtime.setStateForTest({ account: { status: "signedIn", email: "viewer@example.com", error: null } });
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: true, saved: true, pending: false });
  });
});
