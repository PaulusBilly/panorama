import { describe, expect, it, vi } from "vitest";
import { StremioCoreRuntime } from "@/runtime/stremio-core-runtime";
import { initialRuntimeSnapshot } from "@/runtime/snapshot";
import type { CoreTransport } from "@/runtime/core-transport";

type RuntimeInternals = {
  snapshot: typeof initialRuntimeSnapshot;
  transport: CoreTransport;
  detailRequest: number;
  imdbByPublicId: Map<string, string>;
  refreshFilmDetails(request: number): Promise<void>;
};

function setup(saved = false) {
  const runtime = new StremioCoreRuntime();
  const internals = runtime as unknown as RuntimeInternals;
  const meta = { id: "tt123", type: "movie", name: "A Film", poster: "poster.jpg", inLibrary: saved };
  const state = {
    selected: { metaPath: { id: meta.id } },
    metaItem: { content: { type: "Ready", content: meta } },
    libraryItem: null,
    streams: [],
  };
  const dispatch = vi.fn().mockResolvedValue(undefined);
  internals.transport = { init: vi.fn(), getState: vi.fn().mockResolvedValue(state), dispatch };
  internals.imdbByPublicId.set("tmdb:100", meta.id);
  internals.snapshot = structuredClone(initialRuntimeSnapshot);
  internals.snapshot.account = { status: "signedIn", email: "viewer@example.com", error: null };
  internals.snapshot.details.filmId = "tmdb:100";
  internals.snapshot.details.watchlist = { available: true, saved, pending: false };
  return { runtime, internals, meta, state, dispatch };
}

function deferred() {
  let resolve!: () => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<void>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

describe("core runtime watchlist", () => {
  it("dispatches the Ready meta item for add and its id for remove", async () => {
    const { runtime, meta, dispatch } = setup();
    await runtime.setWatchlisted(true);
    expect(dispatch).toHaveBeenLastCalledWith({ action: "Ctx", args: { action: "AddToLibrary", args: meta } });
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: true, saved: true, pending: false });
    await runtime.setWatchlisted(false);
    expect(dispatch).toHaveBeenLastCalledWith({ action: "Ctx", args: { action: "RemoveFromLibrary", args: meta.id } });
    expect(runtime.getSnapshot().details.watchlist.saved).toBe(false);
  });

  it.each([false, true])("reverts a rejected toggle from saved=%s", async (saved) => {
    const { runtime, dispatch } = setup(saved);
    dispatch.mockRejectedValue(new Error("dispatch failed"));
    await runtime.setWatchlisted(!saved);
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: true, saved, pending: false });
  });

  it("keeps the optimistic value during stale refreshes and clears pending on confirmation", async () => {
    const { runtime, internals, meta, dispatch } = setup();
    const operation = deferred();
    dispatch.mockReturnValue(operation.promise);
    const toggle = runtime.setWatchlisted(true);
    await vi.waitFor(() => expect(dispatch).toHaveBeenCalledOnce());
    await internals.refreshFilmDetails(internals.detailRequest);
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: true, saved: true, pending: true });
    await runtime.setWatchlisted(false);
    expect(dispatch).toHaveBeenCalledOnce();
    meta.inLibrary = true;
    await internals.refreshFilmDetails(internals.detailRequest);
    expect(runtime.getSnapshot().details.watchlist.pending).toBe(false);
    operation.resolve();
    await toggle;
  });

  it.each([false, true])("ignores a late dispatch result after the film changes (failure=%s)", async (failure) => {
    const { runtime, internals, dispatch } = setup();
    const operation = deferred();
    dispatch.mockReturnValue(operation.promise);
    const toggle = runtime.setWatchlisted(true);
    await vi.waitFor(() => expect(dispatch).toHaveBeenCalledOnce());
    internals.detailRequest += 1;
    internals.snapshot.details = { ...structuredClone(initialRuntimeSnapshot.details), filmId: "tmdb:101" };
    if (failure) operation.reject(new Error("late failure"));
    else operation.resolve();
    await toggle;
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: false, saved: false, pending: false });
  });

  it("skips unavailable, unchanged and pending toggles", async () => {
    const { runtime, internals, dispatch } = setup();
    await runtime.setWatchlisted(false);
    internals.snapshot.details.watchlist.available = false;
    await runtime.setWatchlisted(true);
    internals.snapshot.details.watchlist = { available: true, saved: false, pending: true };
    await runtime.setWatchlisted(true);
    expect(dispatch).not.toHaveBeenCalled();
  });

  it("requires signed-in Ready metadata for availability and uses library fallback", async () => {
    const { runtime, internals } = setup();
    internals.transport.getState = vi.fn().mockResolvedValue({
      selected: { metaPath: { id: "tt123" } },
      metaItem: { content: { type: "Loading" } },
      libraryItem: { removed: false, temp: false },
    });
    await internals.refreshFilmDetails(internals.detailRequest);
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: false, saved: true, pending: false });
    internals.snapshot.account.status = "loggedOut";
    await internals.refreshFilmDetails(internals.detailRequest);
    expect(runtime.getSnapshot().details.watchlist).toEqual({ available: false, saved: false, pending: false });
  });
});
