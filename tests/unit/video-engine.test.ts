import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  dispatch: vi.fn(),
  destroy: vi.fn(),
  on: vi.fn(),
  constructor: vi.fn(),
}));

const constructor = vi.fn(function Video() {
  mocks.constructor();
  return { on: mocks.on, dispatch: mocks.dispatch, destroy: mocks.destroy };
});

vi.mock("@stremio/stremio-video", () => ({ default: constructor }));

afterEach(() => {
  vi.useRealTimers();
  delete window.panoramaDesktop;
  constructor.mockClear();
  mocks.dispatch.mockClear();
  mocks.destroy.mockClear();
  mocks.on.mockClear();
  vi.resetModules();
});

describe("video engine selection", () => {
  it("rejects incompatible cues from another track and refills a cached selection", async () => {
    const send = vi.fn();
    const listeners = new Map<string, (payload: unknown) => void>();
    window.panoramaDesktop = {
      getCapabilities: async () => ({ platform: "darwin", architecture: "arm64", nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" }, appVersion: "0.1.0" }),
      openExternal: vi.fn(),
      mpv: { send, on: (channel, listener) => { listeners.set(channel, listener); return () => listeners.delete(channel); }, setVideoSurface: vi.fn() },
    };
    const { createVideoEngine } = await import("../../runtime/video-engine");
    const container = document.createElement("div");
    const session = await createVideoEngine(container);
    const select = () => session.engine.dispatch({ type: "setProp", propName: "selectedSubtitlesTrackId", propValue: "EMBEDDED_2" });
    select();
    const cue = { playbackGeneration: 1, selectionGeneration: 1, seekGeneration: 0, sequence: 1, trackId: "2", kind: "text", text: "♪ Current track ♪", startSeconds: 0, endSeconds: 1 };
    const emit = (patch: Record<string, unknown>) => listeners.get("mpv-event-subtitle-cue")?.({ ...cue, ...patch });
    emit({});
    expect(container.querySelector(".panorama-subtitle-box")?.textContent).toBe(cue.text);
    emit({ sequence: 2, kind: "authored", trackId: "3" });
    expect(container.querySelector(".panorama-subtitle-box")?.textContent).toBe(cue.text);
    select();
    expect(container.querySelector(".panorama-subtitle-box")?.textContent).toBe("");
    emit({ sequence: 3 });
    expect(container.querySelector(".panorama-subtitle-box")?.textContent).toBe("");
    emit({ selectionGeneration: 2, sequence: 4 });
    expect(container.querySelector(".panorama-subtitle-box")?.textContent).toBe(cue.text);
    session.destroy();
    expect(container.querySelector(".panorama-subtitle-layer")).toBeNull();
  });
  it("uses HTMLVideo when the desktop bridge is absent", async () => {
    const { createVideoEngine } = await import("../../runtime/video-engine");
    const container = document.createElement("div");
    const session = await createVideoEngine(container);

    session.engine.dispatch({ type: "command", commandName: "load", commandArgs: { stream: { url: "https://example.test/movie" } } });

    expect(session.device).toBe("HTMLVideo");
    expect(mocks.dispatch).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({ containerElement: container, onSubtitleCue: expect.any(Function) }));
  });

  it("uses ShellVideo only for a healthy native capability", async () => {
    const send = vi.fn();
    window.panoramaDesktop = {
      getCapabilities: async () => ({
        platform: "darwin",
        architecture: "arm64",
        nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" },
        appVersion: "0.1.0",
      }),
      openExternal: vi.fn(),
      mpv: { send, on: vi.fn(() => vi.fn()), setVideoSurface: vi.fn() },
    };
    const { createVideoEngine } = await import("../../runtime/video-engine");
    const container = document.createElement("div");
    const session = await createVideoEngine(container);

    session.engine.dispatch({ type: "command", commandName: "load", commandArgs: { stream: { url: "https://example.test/movie" } } });
    expect(session.device).toBe("ShellVideo");
    expect(mocks.dispatch).toHaveBeenCalledWith(
      expect.objectContaining({ commandArgs: expect.objectContaining({
        platform: "macos",
        hardwareDecoding: true,
        gpuVideoProcessing: false,
      }) }),
      expect.objectContaining({ containerElement: container, mpvSeparateWindow: false, shellTransport: expect.any(Object) }),
    );
    session.destroy();
  });

  it("selects the Windows ShellVideo platform for a healthy win32/x64 host", async () => {
    window.panoramaDesktop = {
      getCapabilities: async () => ({
        platform: "win32",
        architecture: "x64",
        nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" },
        appVersion: "0.1.0",
      }),
      openExternal: vi.fn(),
      mpv: { send: vi.fn(), on: vi.fn(() => vi.fn()), setVideoSurface: vi.fn() },
    };
    const { createVideoEngine } = await import("../../runtime/video-engine");
    const container = document.createElement("div");
    const session = await createVideoEngine(container);

    session.engine.dispatch({ type: "command", commandName: "load", commandArgs: { stream: { url: "https://example.test/movie" } } });

    expect(session.device).toBe("ShellVideo");
    expect(mocks.dispatch).toHaveBeenCalledWith(
      expect.objectContaining({ commandArgs: expect.objectContaining({
        platform: "windows",
        hardwareDecoding: true,
        gpuVideoProcessing: true,
      }) }),
      expect.objectContaining({ shellTransport: expect.any(Object) }),
    );
    session.destroy();
  });

  it("loads desktop addon subtitles into Windows MPV instead of the HTML overlay", async () => {
    const send = vi.fn();
    const listeners = new Map<string, Array<(payload: unknown) => void>>();
    window.panoramaDesktop = {
      getCapabilities: async () => ({
        platform: "win32",
        architecture: "x64",
        nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" },
        appVersion: "0.1.0",
      }),
      openExternal: vi.fn(),
      mpv: {
        send,
        on: vi.fn((channel, listener) => {
          const channelListeners = listeners.get(channel) ?? [];
          channelListeners.push(listener);
          listeners.set(channel, channelListeners);
          return () => undefined;
        }),
        setVideoSurface: vi.fn(),
      },
    };
    const { createVideoEngine, NATIVE_ADDON_SUBTITLE_TITLE_PREFIX } = await import("../../runtime/video-engine");
    const session = await createVideoEngine(document.createElement("div"));
    const loaded = vi.fn();
    session.engine.on("extraSubtitlesTrackLoaded", loaded);
    session.engine.dispatch({
      type: "command",
      commandName: "addExtraSubtitlesTracks",
      commandArgs: {
        tracks: [{
          id: "addon-en",
          url: "https://subtitles.example/english.srt",
          lang: "en",
          label: "English",
          origin: "OpenSubtitles",
        }],
      },
    });
    session.engine.dispatch({
      type: "setProp",
      propName: "selectedExtraSubtitlesTrackId",
      propValue: "addon-en",
    });
    expect(send).toHaveBeenCalledWith("mpv-command", [
      "sub-add",
      "https://subtitles.example/english.srt",
      "cached",
      `${NATIVE_ADDON_SUBTITLE_TITLE_PREFIX}p1 · English`,
      "en",
    ]);
    expect(loaded).not.toHaveBeenCalled();
    for (const listener of listeners.get("mpv-prop-change") ?? []) {
      listener({
        name: "track-list",
        data: [{ id: 7, type: "sub", title: `${NATIVE_ADDON_SUBTITLE_TITLE_PREFIX}p1 · English` }],
      });
    }
    expect(loaded).toHaveBeenCalledWith(expect.objectContaining({ id: "addon-en" }));
    expect(send).toHaveBeenCalledWith("mpv-set-prop", ["sid", "7"]);

    send.mockClear();
    loaded.mockClear();
    session.engine.dispatch({
      type: "setProp",
      propName: "selectedExtraSubtitlesTrackId",
      propValue: "addon-en",
    });
    await Promise.resolve();
    expect(send.mock.calls.filter((call) => call[0] === "mpv-set-prop" && call[1][0] === "sid")).toEqual([["mpv-set-prop", ["sid", "7"]]]);
    expect(loaded).toHaveBeenCalledWith(expect.objectContaining({ id: "addon-en" }));
    expect(mocks.dispatch).not.toHaveBeenCalledWith(
      expect.objectContaining({ propName: "selectedExtraSubtitlesTrackId" }),
      expect.anything(),
    );
    session.destroy();
  });

  it("loads desktop addon subtitles into macOS MPV instead of the HTML overlay", async () => {
    const send = vi.fn();
    const listeners = new Map<string, Array<(payload: unknown) => void>>();
    window.panoramaDesktop = {
      getCapabilities: async () => ({
        platform: "darwin",
        architecture: "arm64",
        nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" },
        appVersion: "0.1.0",
      }),
      openExternal: vi.fn(),
      mpv: {
        send,
        on: vi.fn((channel, listener) => {
          const channelListeners = listeners.get(channel) ?? [];
          channelListeners.push(listener);
          listeners.set(channel, channelListeners);
          return () => undefined;
        }),
        setVideoSurface: vi.fn(),
      },
    };
    const { createVideoEngine, NATIVE_ADDON_SUBTITLE_TITLE_PREFIX } = await import("../../runtime/video-engine");
    const session = await createVideoEngine(document.createElement("div"));
    const loaded = vi.fn();
    session.engine.on("extraSubtitlesTrackLoaded", loaded);
    session.engine.dispatch({
      type: "command",
      commandName: "addExtraSubtitlesTracks",
      commandArgs: {
        tracks: [{
          id: "addon-en",
          url: "https://subtitles.example/english.srt",
          lang: "en",
          label: "English",
          origin: "OpenSubtitles",
        }],
      },
    });
    session.engine.dispatch({
      type: "setProp",
      propName: "selectedExtraSubtitlesTrackId",
      propValue: "addon-en",
    });
    expect(send).toHaveBeenCalledWith("mpv-command", [
      "sub-add",
      "https://subtitles.example/english.srt",
      "cached",
      `${NATIVE_ADDON_SUBTITLE_TITLE_PREFIX}p1 · English`,
      "en",
    ]);
    expect(loaded).not.toHaveBeenCalled();
    for (const listener of listeners.get("mpv-prop-change") ?? []) {
      listener({
        name: "track-list",
        data: [{ id: 9, type: "sub", title: `${NATIVE_ADDON_SUBTITLE_TITLE_PREFIX}p1 · English` }],
      });
    }
    expect(loaded).toHaveBeenCalledWith(expect.objectContaining({ id: "addon-en" }));
    session.destroy();
  });

  it("times out a stalled addon subtitle and prevents its late completion from replacing an embedded track", async () => {
    vi.useFakeTimers();
    const send = vi.fn();
    const listeners = new Map<string, Array<(payload: unknown) => void>>();
    window.panoramaDesktop = {
      getCapabilities: async () => ({
        platform: "win32",
        architecture: "x64",
        nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" },
        appVersion: "0.1.0",
      }),
      openExternal: vi.fn(),
      mpv: {
        send,
        on: vi.fn((channel, listener) => {
          const channelListeners = listeners.get(channel) ?? [];
          channelListeners.push(listener);
          listeners.set(channel, channelListeners);
          return () => undefined;
        }),
        setVideoSurface: vi.fn(),
      },
    };
    const {
      createVideoEngine,
      NATIVE_ADDON_SUBTITLE_LOAD_TIMEOUT_MS,
      NATIVE_ADDON_SUBTITLE_TITLE_PREFIX,
    } = await import("../../runtime/video-engine");
    const session = await createVideoEngine(document.createElement("div"));
    const failed = vi.fn();
    const loaded = vi.fn();
    session.engine.on("extraSubtitlesTrackError", failed);
    session.engine.on("extraSubtitlesTrackLoaded", loaded);
    session.engine.dispatch({
      type: "command",
      commandName: "addExtraSubtitlesTracks",
      commandArgs: {
        tracks: [{
          id: "community-en",
          url: "https://subtitles.example/community.srt",
          lang: "en",
          label: "Community English",
          origin: "Community Subtitles",
        }],
      },
    });
    session.engine.dispatch({
      type: "setProp",
      propName: "selectedExtraSubtitlesTrackId",
      propValue: "community-en",
    });

    await vi.advanceTimersByTimeAsync(NATIVE_ADDON_SUBTITLE_LOAD_TIMEOUT_MS);
    expect(failed).toHaveBeenCalledWith(expect.objectContaining({
      critical: false,
      trackId: "community-en",
    }));

    send.mockClear();
    session.engine.dispatch({
      type: "setProp",
      propName: "selectedSubtitlesTrackId",
      propValue: "EMBEDDED_4",
    });
    for (const listener of listeners.get("mpv-prop-change") ?? []) {
      listener({
        name: "track-list",
        data: [{
          id: 7,
          type: "sub",
          title: `${NATIVE_ADDON_SUBTITLE_TITLE_PREFIX}p1 · Community English`,
        }],
      });
    }
    expect(send).toHaveBeenCalledWith("mpv-set-prop", ["sid", "4"]);
    expect(send).not.toHaveBeenCalledWith("mpv-set-prop", ["sid", "7"]);
    expect(loaded).not.toHaveBeenCalled();
    session.destroy();
  });

  it("reports native playback from first-frame or advancing-time events", async () => {
    const listeners = new Map<string, Array<(payload: unknown) => void>>();
    window.panoramaDesktop = {
      getCapabilities: async () => ({
        platform: "darwin",
        architecture: "arm64",
        nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" },
        appVersion: "0.1.0",
      }),
      openExternal: vi.fn(),
      mpv: {
        send: vi.fn(),
        on: vi.fn((channel, listener) => {
          const channelListeners = listeners.get(channel) ?? [];
          channelListeners.push(listener);
          listeners.set(channel, channelListeners);
          return () => undefined;
        }),
        setVideoSurface: vi.fn(),
      },
    };
    const { createVideoEngine } = await import("../../runtime/video-engine");
    const session = await createVideoEngine(document.createElement("div"));
    const active = vi.fn();
    session.engine.on("nativePlaybackActive", active);

    for (const listener of listeners.get("mpv-event-video-ready") ?? []) {
      listener({ loadId: 3, ready: false });
      listener({ loadId: 3, ready: true });
    }
    expect(active).toHaveBeenCalledOnce();

    active.mockClear();
    for (const listener of listeners.get("mpv-prop-change") ?? []) {
      listener({ name: "time-pos", data: 10 });
      listener({ name: "time-pos", data: 11 });
    }
    expect(active).toHaveBeenCalledOnce();
    session.destroy();
  });
  it("keeps HTMLVideo when Windows native initialization is unavailable", async () => {
    window.panoramaDesktop = {
      getCapabilities: async () => ({
        platform: "win32",
        architecture: "x64",
        nativePlayback: { status: "unavailable", reason: "initialization-failed" },
        appVersion: "0.1.0",
      }),
      openExternal: vi.fn(),
      mpv: { send: vi.fn(), on: vi.fn(() => vi.fn()), setVideoSurface: vi.fn() },
    };
    const { createVideoEngine } = await import("../../runtime/video-engine");
    const session = await createVideoEngine(document.createElement("div"));
    expect(session.device).toBe("HTMLVideo");
  });
});
