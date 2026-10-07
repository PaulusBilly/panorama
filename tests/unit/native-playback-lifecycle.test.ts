import { describe, expect, it, vi } from "vitest";
import { MpvController } from "../../desktop/main/mpv-controller";
import { MpvHost } from "../../desktop/native/mpv-host/mpv-host";
import type { MpvHostCommand, MpvHostDiagnostics, NativeMpvBinding } from "../../desktop/native/mpv-host/types";

class LifecycleBinding implements NativeMpvBinding {
  commands: MpvHostCommand[] = [];
  diagnostics: MpvHostDiagnostics = {
    mpvVersion: "0.41.0",
    videoCodec: "hevc",
    hardwareDecoder: "videotoolbox",
    buffering: false,
    cacheSeconds: 30,
    timeSeconds: 0,
    durationSeconds: 7200,
    paused: false,
    seeking: false,
    eofReached: false,
    tracks: [],
    renderReady: true,
    renderedFrames: 1,
    videoWidth: 3840,
    videoHeight: 2160,
  };
  dispatch(command: MpvHostCommand): void { this.commands.push(command); }
  getDiagnostics(): MpvHostDiagnostics { return this.diagnostics; }
  destroy(): void {}
}

describe("native playback lifecycle", () => {
  it("reports playback active only while loaded native media is unpaused", () => {
    const binding = new LifecycleBinding();
    const activeStates: boolean[] = [];
    const controller = new MpvController(
      new MpvHost(binding, "http://127.0.0.1:11470"),
      () => undefined,
      60_000,
      Date.now,
      (active) => { activeStates.push(active); },
    );

    controller.poll();
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/media"]);
    controller.poll();
    controller.poll();
    binding.diagnostics.paused = true;
    controller.poll();
    binding.diagnostics.paused = false;
    controller.poll();
    controller.destroy();

    expect(activeStates).toEqual([true, false, true, false]);
  });

  it("keeps replacement playback active when MPV reports the previous load stopped", () => {
    const binding = new LifecycleBinding();
    const activeStates: boolean[] = [];
    const events: Array<[string, unknown]> = [];
    const controller = new MpvController(
      new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => events.push([channel, payload]),
      60_000,
      Date.now,
      (active) => { activeStates.push(active); },
    );

    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/first"]);
    binding.diagnostics.path = "http://127.0.0.1:11470/first";
    controller.poll();
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/second"]);
    binding.diagnostics.path = "http://127.0.0.1:11470/second";
    binding.diagnostics.endSequence = 1;
    binding.diagnostics.endReason = "stop";
    controller.poll();

    const activeStatesBeforeDestroy = [...activeStates];
    controller.destroy();

    expect(activeStatesBeforeDestroy).toEqual([true]);
    expect(events.filter(([channel]) => channel === "mpv-event-ended")).toHaveLength(0);
  });

  it("replays multiple initial property observations from one native snapshot", () => {
    const binding = new LifecycleBinding();
    const events: Array<[string, unknown]> = [];
    let diagnosticReads = 0;
    const originalGetDiagnostics = binding.getDiagnostics.bind(binding);
    binding.getDiagnostics = () => {
      diagnosticReads += 1;
      if (diagnosticReads > 1) throw new Error("native diagnostics read more than once");
      return originalGetDiagnostics();
    };
    const controller = new MpvController(
      new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => events.push([channel, payload]),
      60_000,
    );

    controller.handleSend("mpv-observe-prop", "mpv-version");
    controller.handleSend("mpv-observe-prop", "pause");

    expect(events).toContainEqual(["mpv-prop-change", { name: "mpv-version", data: "0.41.0" }]);
    expect(events).toContainEqual(["mpv-prop-change", { name: "pause", data: false }]);
    controller.destroy();
  });

  it("rejects stale readiness and emits end once per load", () => {
    const binding = new LifecycleBinding();
    const events: Array<[string, unknown]> = [];
    const controller = new MpvController(
      new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => events.push([channel, payload]),
      60_000,
    );
    controller.handleSend("mpv-observe-prop", "time-pos");
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/media"]);
    binding.diagnostics.timeSeconds = 7_190;
    binding.diagnostics.renderedFrames = 2;
    Object.assign(binding.diagnostics, { loadGeneration: 1, ownedEntryId: 1, fileStarted: true, fileLoaded: true, decodedReady: true, presented: true });
    controller.poll();
    binding.diagnostics.eofReached = true;
    controller.poll();
    controller.poll();

    expect(events).toContainEqual(["mpv-event-video-ready", { loadId: 1, ready: false }]);
    expect(events).toContainEqual(["mpv-event-video-ready", { loadId: 1, ready: true }]);
    expect(events.filter(([channel]) => channel === "mpv-event-ended")).toHaveLength(1);
    expect(binding.commands).toContainEqual({ type: "shellCommand", args: ["loadfile", "http://127.0.0.1:11470/media"] });
    controller.destroy();
  });

  it("hides the native surface on suspend and teardown", () => {
    const binding = new LifecycleBinding();
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"), () => undefined, 60_000);
    controller.setVideoSurface({ visible: true, x: 10, y: 20, width: 800, height: 450, scaleFactor: 2 });
    controller.suspendSurface(true);

    expect(binding.commands.slice(-2)).toEqual([
      { type: "setBounds", x: 10, y: 20, width: 800, height: 450, scaleFactor: 2 },
      { type: "setBounds", x: 0, y: 0, width: 0, height: 0, scaleFactor: 2 },
    ]);
    controller.destroy();
  });

  it("replays an observed property for each ShellVideo session", () => {
    const binding = new LifecycleBinding();
    const events: Array<[string, unknown]> = [];
    const controller = new MpvController(
      new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => events.push([channel, payload]),
      60_000,
    );

    controller.handleSend("mpv-observe-prop", "mpv-version");
    events.length = 0;
    controller.handleSend("mpv-observe-prop", "mpv-version");

    expect(events).toEqual([
      ["mpv-prop-change", { name: "mpv-version", data: "0.41.0" }],
    ]);
    controller.destroy();
  });

  it("routes ShellVideo time changes through the native seek command", () => {
    const binding = new LifecycleBinding();
    const controller = new MpvController(
      new MpvHost(binding, "http://127.0.0.1:11470"),
      () => undefined,
      60_000,
    );

    controller.handleSend("mpv-set-prop", ["time-pos", 1078]);

    expect(binding.commands).toContainEqual({ type: "seek", seconds: 1078 });
    controller.destroy();
  });

  it("turns a native end-file error into a recoverable ShellVideo error", () => {
    const binding = new LifecycleBinding();
    const events: Array<[string, unknown]> = [];
    const controller = new MpvController(
      new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => events.push([channel, payload]),
      60_000,
    );
    binding.diagnostics.endSequence = 1;
    binding.diagnostics.endReason = "error";
    binding.diagnostics.endError = "loading failed";
    controller.poll();

    expect(events).toContainEqual([
      "mpv-event-ended",
      { reason: "error", error: { critical: true, message: "loading failed" } },
    ]);
    controller.destroy();
  });

  it("baselines an unobserved old-source end before replacing the load", () => {
    const binding = new LifecycleBinding();
    const events: Array<[string, unknown]> = [];
    const controller = new MpvController(
      new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => events.push([channel, payload]),
      60_000,
    );
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/first"]);
    binding.diagnostics.endSequence = 1;
    binding.diagnostics.endReason = "error";
    binding.diagnostics.endError = "old source failed";

    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/second"]);
    controller.poll();

    expect(events).not.toContainEqual([
      "mpv-event-ended",
      { reason: "error", error: { critical: true, message: "old source failed" } },
    ]);
    controller.destroy();
  });

  it.each(["stop", "quit"])("ignores a native %s end during replacement", (reason) => {
    const binding = new LifecycleBinding();
    const events: Array<[string, unknown]> = [];
    const controller = new MpvController(
      new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => events.push([channel, payload]),
      60_000,
    );
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/second"]);
    binding.diagnostics.endSequence = 1;
    binding.diagnostics.endReason = reason;
    controller.poll();

    expect(events.filter(([channel]) => channel === "mpv-event-ended")).toHaveLength(0);
    controller.destroy();
  });

  it.each(["stop", "quit"])("updates pause state after ignoring replacement %s", (reason) => {
    const binding = new LifecycleBinding();
    const active: boolean[] = [];
    const events: unknown[] = [];
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => { if (channel === "mpv-event-ended") events.push(payload); }, 60_000, Date.now,
      (value) => active.push(value));
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/first"]);
    controller.poll();
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/second"]);
    binding.diagnostics.endSequence = 1;
    binding.diagnostics.endReason = reason;
    binding.diagnostics.paused = true;
    controller.poll();
    binding.diagnostics.paused = false;
    binding.diagnostics.timeSeconds = 7_190;
    controller.poll();
    expect(events).toEqual([]);
    expect(active).toEqual([true, false, true]);
    binding.diagnostics.endSequence = 2;
    binding.diagnostics.endReason = "eof";
    controller.poll();
    expect(events).toEqual([{ reason: "eof" }]);
    expect(active).toEqual([true, false, true, false]);
    controller.destroy();
  });

  it("refreshes the latest bounds while respecting suspension", () => {
    const binding = new LifecycleBinding();
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"), () => undefined, 60_000);
    controller.setVideoSurface({ visible: true, x: 0, y: 32, width: 800, height: 450, scaleFactor: 1.5 });
    controller.refreshSurface();
    expect(binding.commands.at(-1)).toEqual({ type: "setBounds", x: 0, y: 32, width: 800, height: 450, scaleFactor: 1.5 });
    controller.suspendSurface(true);
    controller.refreshSurface();
    expect(binding.commands.at(-1)).toMatchObject({ width: 0, height: 0 });
    controller.suspendSurface(false);
    expect(binding.commands.at(-1)).toMatchObject({ width: 800, height: 450 });
    controller.destroy();
  });

  it("ignores an old end sequence invalidated atomically by the native replacement", () => {
    const binding = new LifecycleBinding();
    const events: unknown[] = [];
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => { if (channel === "mpv-event-ended") events.push(payload); }, 60_000);
    binding.dispatch = () => {
      binding.diagnostics.endSequence = 1;
      binding.diagnostics.endReason = null;
      binding.diagnostics.endError = null;
    };
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/new"]);
    controller.poll();
    expect(events).toEqual([]);
    vi.useFakeTimers();
    try {
      for (const sequence of [2, 3, 4]) {
        binding.diagnostics.endSequence = sequence;
        binding.diagnostics.endReason = "error";
        binding.diagnostics.endError = "current failure";
        controller.poll();
        vi.advanceTimersByTime(3_000);
      }
    } finally { vi.useRealTimers(); }
    expect(events).toEqual([{ reason: "error", error: { critical: true, message: "current failure" } }]);
    controller.destroy();
  });

  it("reloads at the last position after a network error before reporting it", () => {
    vi.useFakeTimers();
    const binding = new LifecycleBinding();
    const events: unknown[] = [];
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => { if (channel === "mpv-event-ended") events.push(payload); }, 60_000);
    try {
      controller.handleSend("mpv-command", ["loadfile", "https://cdn.example/remux.mkv"]);
      binding.diagnostics.timeSeconds = 1234.6;
      controller.poll();
      binding.diagnostics.endSequence = 1;
      binding.diagnostics.endReason = "error";
      binding.diagnostics.endError = "network error";
      controller.poll();
      expect(events).toEqual([]);
      vi.advanceTimersByTime(1_000);
      expect(binding.commands).toContainEqual({
        type: "shellCommand",
        args: ["loadfile", "https://cdn.example/remux.mkv", "replace", "-1", "start=+1234"],
      });
    } finally {
      controller.destroy();
      vi.useRealTimers();
    }
  });

  it("reloads a stream that ends far before its duration instead of reporting EOF", () => {
    vi.useFakeTimers();
    const binding = new LifecycleBinding();
    const events: unknown[] = [];
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => { if (channel === "mpv-event-ended") events.push(payload); }, 60_000);
    try {
      controller.handleSend("mpv-command", ["loadfile", "https://cdn.example/remux.mkv"]);
      binding.diagnostics.timeSeconds = 600;
      controller.poll();
      binding.diagnostics.eofReached = true;
      controller.poll();
      vi.advanceTimersByTime(1_000);
      binding.diagnostics.eofReached = false;
      controller.poll();
      expect(events).toEqual([]);
      expect(binding.commands.at(-1)).toEqual({
        type: "shellCommand",
        args: ["loadfile", "https://cdn.example/remux.mkv", "replace", "-1", "start=+600"],
      });
    } finally {
      controller.destroy();
      vi.useRealTimers();
    }
  });

  it("keeps polling after transient native failures and reports only persistent ones", () => {
    vi.useFakeTimers();
    const binding = new LifecycleBinding();
    const events: unknown[] = [];
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => { if (channel === "mpv-event-ended") events.push(payload); }, 100);
    const read = binding.getDiagnostics.bind(binding);
    let failing = false;
    binding.getDiagnostics = () => { if (failing) throw new Error("busy"); return read(); };
    try {
      controller.handleSend("mpv-command", ["loadfile", "https://cdn.example/remux.mkv"]);
      failing = true;
      vi.advanceTimersByTime(300);
      failing = false;
      vi.advanceTimersByTime(100);
      expect(events).toEqual([]);
      failing = true;
      vi.advanceTimersByTime(1_000);
      expect(events).toEqual([{ reason: "error", error: { critical: true, message: "Native player stopped." } }]);
    } finally {
      controller.destroy();
      vi.useRealTimers();
    }
  });

  it("ignores ShellVideo hwdec requests so the native decoder selection is kept", () => {
    const binding = new LifecycleBinding();
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"), () => undefined, 60_000);
    controller.handleSend("mpv-set-prop", ["hwdec", "auto-copy"]);
    expect(binding.commands).not.toContainEqual(expect.objectContaining({ name: "hwdec" }));
    controller.destroy();
  });

  it("writes one host-only playback sample per second while media is loaded", () => {
    const binding = new LifecycleBinding();
    const lines: string[] = [];
    let now = 10_000;
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"), () => undefined, 60_000,
      () => now, () => undefined, undefined, (line) => lines.push(line));
    controller.poll();
    controller.handleSend("mpv-command", ["loadfile", "https://cdn.example/secret/token/film.mkv"]);
    Object.assign(binding.diagnostics, { cacheSeconds: 20, cacheForwardBytes: 50_000_000, inputBytesPerSecond: 2_500_000 });
    controller.poll();
    now += 500;
    controller.poll();
    now += 600;
    controller.poll();
    controller.destroy();

    expect(lines).toHaveLength(2);
    expect(lines.join("\n")).not.toContain("secret");
    expect(JSON.parse(lines[0])).toMatchObject({ host: "cdn.example", netMbps: 20, mediaMbps: 20 });
  });

  it("routes remote sources through the parallel media proxy and reports their bitrate", () => {
    const binding = new LifecycleBinding();
    const opened: string[] = [];
    let closed = 0;
    const proxy = {
      open: (url: string) => { opened.push(url); return "http://127.0.0.1:50000/media/" + "a".repeat(32); },
      close: () => { closed += 1; },
      stats: () => ({ downloadMbps: 7.5, sizeBytes: 20_300_000_000, unreachable: false }),
      setReadAhead: () => undefined,
    };
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"), () => undefined, 60_000,
      Date.now, () => undefined, undefined, undefined, proxy);
    controller.handleSend("mpv-command", ["loadfile", "https://cdn.example/film.mkv"]);
    expect(binding.commands.at(-1)).toEqual({ type: "shellCommand", args: ["loadfile", "http://127.0.0.1:50000/media/" + "a".repeat(32)] });
    binding.diagnostics.durationSeconds = 4_320;
    expect(controller.getPlaybackDiagnostics()).toMatchObject({ sourceKind: "remote", downloadMbps: 7.5 });
    expect(controller.getPlaybackDiagnostics().sourceBitrateMbps).toBeCloseTo(37.6, 1);
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11470/hash/0"]);
    expect(binding.commands.at(-1)).toEqual({ type: "shellCommand", args: ["loadfile", "http://127.0.0.1:11470/hash/0"] });
    expect(opened).toEqual(["https://cdn.example/film.mkv"]);
    controller.handleSend("mpv-command", ["stop"]);
    expect(closed).toBe(2);
    controller.destroy();
  });

  it("reports an unreachable remote source at once instead of reloading it", () => {
    vi.useFakeTimers();
    const binding = new LifecycleBinding();
    const events: unknown[] = [];
    const proxy = {
      open: () => "http://127.0.0.1:50000/media/" + "b".repeat(32),
      close: () => undefined,
      stats: () => ({ downloadMbps: null, sizeBytes: null, unreachable: true }),
      setReadAhead: () => undefined,
    };
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11470"),
      (channel, payload) => { if (channel === "mpv-event-ended") events.push(payload); }, 60_000,
      Date.now, () => undefined, undefined, undefined, proxy);
    try {
      controller.handleSend("mpv-command", ["loadfile", "https://cdn.example/film.mkv"]);
      binding.diagnostics.endSequence = 1;
      binding.diagnostics.endReason = "error";
      binding.diagnostics.endError = "loading failed";
      controller.poll();
      vi.advanceTimersByTime(5_000);
      expect(events).toEqual([{ reason: "error", error: { critical: true, message: "loading failed" } }]);
      expect(binding.commands.filter((command) => command.type === "shellCommand")).toHaveLength(1);
      expect(controller.getPlaybackDiagnostics().sourceUnreachable).toBe(true);
    } finally {
      controller.destroy();
      vi.useRealTimers();
    }
  });
});
