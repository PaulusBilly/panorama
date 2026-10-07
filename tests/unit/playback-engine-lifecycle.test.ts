import { afterEach, describe, expect, it, vi } from "vitest";
import { StremioCoreRuntime } from "../../runtime/stremio-core-runtime";
import type { VideoEngineSession } from "../../runtime/video-engine";

const mocks = vi.hoisted(() => ({ create: vi.fn() }));
vi.mock("../../runtime/video-engine", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../runtime/video-engine")>(),
  createVideoEngine: mocks.create,
}));

function session() {
  return { device: "ShellVideo" as const, engine: { on: vi.fn(), dispatch: vi.fn(), destroy: vi.fn() }, destroy: vi.fn() };
}

function deferred() {
  let resolve!: (value: VideoEngineSession) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<VideoEngineSession>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

afterEach(() => mocks.create.mockReset());

describe("playback engine lifecycle", () => {
  it("keeps concurrent player attachments on the engine that receives load and controls", async () => {
    const runtime = new StremioCoreRuntime();
    const internals = runtime as unknown as { refreshPlayer(): Promise<void>; video: VideoEngineSession["engine"] | null };
    let loaded = false;
    vi.spyOn(internals, "refreshPlayer").mockImplementation(async () => {
      if (!loaded) {
        loaded = true;
        internals.video?.dispatch({ type: "command", commandName: "load" });
      }
    });
    const first = deferred();
    const second = deferred();
    mocks.create.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const container = document.createElement("div");
    const mount = runtime.attachPlayer(container);
    const startup = runtime.attachPlayer(container);
    const playing = session();
    first.resolve(playing);
    await mount;
    const replacement = session();
    second.resolve(replacement);
    await startup;

    runtime.setPlaybackPaused(true);
    runtime.seekPlayback(45);
    await runtime.stopPlayback();

    expect(mocks.create).toHaveBeenCalledTimes(1);
    expect(playing.engine.dispatch).toHaveBeenCalledWith({ type: "setProp", propName: "paused", propValue: true });
    expect(playing.engine.dispatch).toHaveBeenCalledWith({ type: "setProp", propName: "time", propValue: 45_000 });
    expect(playing.destroy).toHaveBeenCalledOnce();
  });

  it("discards engine creation that finishes after the player closes", async () => {
    const runtime = new StremioCoreRuntime();
    const pending = deferred();
    mocks.create.mockReturnValue(pending.promise);
    const attachment = runtime.attachPlayer(document.createElement("div"));
    await runtime.stopPlayback();
    const late = session();
    pending.resolve(late);
    await attachment;
    runtime.setPlaybackPaused(false);

    expect(late.destroy).toHaveBeenCalledOnce();
    expect(late.engine.on).not.toHaveBeenCalled();
    expect(late.engine.dispatch).not.toHaveBeenCalled();
  });

  it.each(["resolve", "reject"])("keeps a remounted player after an earlier attachment %s", async (result) => {
    const runtime = new StremioCoreRuntime();
    const first = deferred();
    const second = deferred();
    mocks.create.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const container = document.createElement("div");
    const detached = runtime.attachPlayer(container);
    runtime.detachPlayer();
    const remounted = runtime.attachPlayer(container);
    const current = session();
    second.resolve(current);
    await remounted;
    const stale = session();
    if (result === "resolve") first.resolve(stale);
    else first.reject(new Error("Detached engine failed"));
    await detached;
    runtime.setPlaybackPaused(true);

    expect(stale.destroy).toHaveBeenCalledTimes(result === "resolve" ? 1 : 0);
    expect(current.destroy).not.toHaveBeenCalled();
    expect(current.engine.dispatch).toHaveBeenCalledWith({ type: "setProp", propName: "paused", propValue: true });
    await runtime.stopPlayback();
  });

  it("ignores a stale creation failure after another playback starts", async () => {
    const runtime = new StremioCoreRuntime();
    const first = deferred();
    const second = deferred();
    mocks.create.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const container = document.createElement("div");
    const old = runtime.attachPlayer(container);
    await runtime.stopPlayback();
    const replacement = runtime.attachPlayer(container);
    const current = session();
    second.resolve(current);
    await replacement;
    first.reject(new Error("Previous engine failed"));
    await old;
    runtime.setPlaybackPaused(true);

    expect(current.destroy).not.toHaveBeenCalled();
    expect(current.engine.dispatch).toHaveBeenCalledWith({ type: "setProp", propName: "paused", propValue: true });
    expect(runtime.getSnapshot().player.status).toBe("idle");
    await runtime.stopPlayback();
  });
});
