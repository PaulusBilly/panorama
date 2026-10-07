import { afterEach, describe, expect, it, vi } from "vitest";

const nativeDispatch = vi.hoisted(() => vi.fn());

vi.mock("@stremio/stremio-video", () => ({
  default: class { on() {} dispatch = nativeDispatch; destroy() {} },
}));

afterEach(() => {
  vi.useRealTimers();
  delete window.panoramaDesktop;
});

async function setup(platform: "win32" | "darwin") {
  vi.useFakeTimers();
  const send = vi.fn();
  const listeners = new Map<string, (payload: unknown) => void>();
  const unsubscribe = vi.fn();
  window.panoramaDesktop = {
    getCapabilities: async () => ({
      platform, architecture: platform === "win32" ? "x64" : "arm64",
      nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" }, appVersion: "0.1.0",
    }),
    openExternal: vi.fn(),
    mpv: { send, on: (channel, listener) => { listeners.set(channel, listener); return () => unsubscribe(); }, setVideoSurface: vi.fn() },
  };
  const { createVideoEngine } = await import("../../runtime/video-engine");
  const session = await createVideoEngine(document.createElement("div"));
  const loaded = vi.fn();
  const failed = vi.fn();
  session.engine.on("extraSubtitlesTrackLoaded", loaded);
  session.engine.on("extraSubtitlesTrackError", failed);
  const add = () => session.engine.dispatch({ type: "command", commandName: "addExtraSubtitlesTracks", commandArgs: {
    tracks: [{ id: "en", url: "https://subtitles.example/en.srt", lang: "en", label: "English", origin: "Test" }],
  } });
  const select = (id: string | null = "en") => session.engine.dispatch({ type: "setProp", propName: "selectedExtraSubtitlesTrackId", propValue: id });
  const title = () => send.mock.calls.filter(([channel]) => channel === "mpv-command").at(-1)![1][3] as string;
  const complete = (name: string, id = 7) => listeners.get("mpv-prop-change")?.({ name: "track-list", data: [{ id, type: "sub", title: name }] });
  add();
  return { session, send, loaded, failed, unsubscribe, add, select, title, complete, listeners };
}

describe.each(["win32", "darwin"] as const)("%s asynchronous subtitles", (platform) => {
  it("holds ShellVideo startup pause until the runtime releases it", async () => {
    const t = await setup(platform);
    t.session.engine.dispatch({ type: "command", commandName: "load", commandArgs: { stream: { url: "http://127.0.0.1:11474/test" }, autoplay: false } });
    const transport = nativeDispatch.mock.calls.at(-1)![1].shellTransport;
    transport.send("mpv-set-prop", ["pause", false]);
    expect(t.send).toHaveBeenLastCalledWith("mpv-set-prop", ["pause", true]);
    t.session.engine.dispatch({ type: "setProp", propName: "paused", propValue: false });
    transport.send("mpv-set-prop", ["pause", false]);
    expect(t.send).toHaveBeenLastCalledWith("mpv-set-prop", ["pause", false]);
    t.session.destroy();
  });

  it("confirms embedded subtitles only after native selection acknowledgment", async () => {
    const t = await setup(platform);
    const confirmed = vi.fn();
    t.session.engine.on("subtitlesTrackLoaded", confirmed);
    t.session.engine.dispatch({ type: "setProp", propName: "selectedSubtitlesTrackId", propValue: "EMBEDDED_3" });
    expect(confirmed).not.toHaveBeenCalled();
    t.listeners.get("mpv-prop-change")?.({ name: "sid", data: "2" });
    expect(confirmed).not.toHaveBeenCalled();
    t.listeners.get("mpv-prop-change")?.({ name: "sid", data: "3" });
    expect(confirmed).toHaveBeenCalledWith("EMBEDDED_3");
    t.session.destroy();
  });

  it("keeps Off and embedded selections when an addon finishes late", async () => {
    const t = await setup(platform);
    t.select();
    const title = t.title();
    t.select(null);
    t.send.mockClear();
    t.complete(title);
    expect(t.loaded).not.toHaveBeenCalled();
    expect(t.send).not.toHaveBeenCalledWith("mpv-set-prop", ["sid", "7"]);
    t.session.engine.dispatch({ type: "setProp", propName: "selectedSubtitlesTrackId", propValue: "EMBEDDED_4" });
    t.complete(title);
    expect(t.send).toHaveBeenLastCalledWith("mpv-set-prop", ["sid", "4"]);
    t.session.destroy();
  });

  it("expires requests and uses a fresh identity for retry", async () => {
    const t = await setup(platform);
    t.select();
    const oldTitle = t.title();
    await vi.advanceTimersByTimeAsync(15_000);
    expect(t.failed).toHaveBeenCalledOnce();
    t.complete(oldTitle);
    expect(t.loaded).not.toHaveBeenCalled();
    t.select();
    const newTitle = t.title();
    expect(newTitle).not.toBe(oldTitle);
    t.complete(oldTitle);
    expect(t.loaded).not.toHaveBeenCalled();
    t.complete(newTitle);
    expect(t.loaded).toHaveBeenCalledOnce();
    t.session.destroy();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("rejects old-source completion even when the addon id is reused", async () => {
    const t = await setup(platform);
    t.select();
    const oldTitle = t.title();
    t.session.engine.dispatch({ type: "command", commandName: "load", commandArgs: { stream: { url: "https://media.example/new" } } });
    t.add();
    t.select();
    const newTitle = t.title();
    expect(newTitle).not.toBe(oldTitle);
    t.complete(oldTitle);
    expect(t.loaded).not.toHaveBeenCalled();
    t.complete(newTitle);
    expect(t.loaded).toHaveBeenCalledOnce();
    t.session.destroy();
  });

  it("cancels cached confirmation when selection changes and disposes pending work", async () => {
    const t = await setup(platform);
    t.select();
    t.complete(t.title());
    t.loaded.mockClear();
    t.select();
    t.select(null);
    await Promise.resolve();
    expect(t.loaded).not.toHaveBeenCalled();
    t.session.engine.dispatch({ type: "command", commandName: "unload" });
    t.add();
    t.select();
    const title = t.title();
    t.session.destroy();
    expect(t.unsubscribe).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(15_000);
    t.complete(title);
    expect(t.failed).not.toHaveBeenCalled();
    expect(t.loaded).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });
});
