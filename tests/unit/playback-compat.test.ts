import { createRequire } from "node:module";
import { describe, expect, it, vi } from "vitest";
import { PLAYBACK_PREPARATION_TIMEOUT_MS, StremioCoreRuntime } from "@/runtime/stremio-core-runtime";
import { initialRuntimeSnapshot } from "@/runtime/snapshot";

const require = createRequire(import.meta.url);

vi.mock("@stremio/stremio-video", () => {
  throw new Error("Video engine unavailable");
});

describe("playback compatibility", () => {
  it("allows six minutes for source preparation", () => {
    expect(PLAYBACK_PREPARATION_TIMEOUT_MS).toBe(360_000);
  });

  it("provides the WebVTT exports expected by the video engine", () => {
    const compat = require("../../runtime/vtt-js-compat.cjs") as Record<string, unknown>;
    expect(compat.WebVTT).toBeTruthy();
    expect(compat.VTTCue).toBeTruthy();
    expect(compat.VTTRegion).toBeTruthy();
  });

  it("clears the preparation watchdog when initial buffering completes", () => {
    vi.useFakeTimers();
    try {
      const runtime = new StremioCoreRuntime() as unknown as {
        snapshot: typeof initialRuntimeSnapshot;
        playerGeneration: number;
        video: { dispatch: ReturnType<typeof vi.fn> };
        startPreparationMonitor(generation: number): void;
        handleVideoProp(name: string, value: unknown): void;
      };
      runtime.playerGeneration = 4;
      runtime.video = { dispatch: vi.fn() };
      runtime.snapshot = {
        ...structuredClone(initialRuntimeSnapshot),
        player: {
          ...structuredClone(initialRuntimeSnapshot.player),
          status: "buffering",
          stage: "loadingVideo",
          buffering: true,
          paused: true,
        },
      };

      runtime.startPreparationMonitor(4);
      runtime.handleVideoProp("buffering", false);
      vi.advanceTimersByTime(PLAYBACK_PREPARATION_TIMEOUT_MS + 1);

      expect(runtime.snapshot.player).toMatchObject({
        status: "ready",
        stage: null,
        paused: true,
        error: null,
      });
      expect(runtime.video.dispatch).not.toHaveBeenCalledWith({ type: "command", commandName: "unload" });
    } finally {
      vi.useRealTimers();
    }
  });

  it("leaves native preparation when playback time is advancing", () => {
    const runtime = new StremioCoreRuntime() as unknown as {
      snapshot: typeof initialRuntimeSnapshot;
      videoDevice: string;
      video: { dispatch: ReturnType<typeof vi.fn> };
      handleVideoProp(name: string, value: unknown): void;
    };
    runtime.videoDevice = "ShellVideo";
    runtime.video = { dispatch: vi.fn() };
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "preparing",
        stage: "loadingVideo",
      },
    };

    runtime.handleVideoProp("time", 1_000);
    expect(runtime.snapshot.player.status).toBe("preparing");

    runtime.handleVideoProp("time", 2_000);
    expect(runtime.snapshot.player).toMatchObject({
      status: "ready",
      stage: null,
      time: 2,
      buffering: false,
      error: null,
    });
  });

  it("leaves preparation when the desktop first-frame signal bypasses ShellVideo readiness", () => {
    const runtime = new StremioCoreRuntime() as unknown as {
      snapshot: typeof initialRuntimeSnapshot;
      playerGeneration: number;
      loadedPlayerGeneration: number;
      video: {
        dispatch: ReturnType<typeof vi.fn>;
        on(event: string, listener: (...args: unknown[]) => void): void;
      };
      bindVideoEvents(video: {
        dispatch: ReturnType<typeof vi.fn>;
        on(event: string, listener: (...args: unknown[]) => void): void;
      }, generation: number): void;
    };
    const listeners = new Map<string, (...args: unknown[]) => void>();
    const video = {
      dispatch: vi.fn(),
      on: (event: string, listener: (...args: unknown[]) => void) => listeners.set(event, listener),
    };
    runtime.playerGeneration = 5;
    runtime.loadedPlayerGeneration = 5;
    runtime.video = video;
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "preparing",
        stage: "loadingVideo",
      },
    };

    runtime.bindVideoEvents(video, 5);
    listeners.get("nativePlaybackActive")?.();

    expect(runtime.snapshot.player).toMatchObject({ status: "ready", stage: null, buffering: false });
  });

  it("still times out a source that never becomes playable", () => {
    vi.useFakeTimers();
    try {
      const runtime = new StremioCoreRuntime() as unknown as {
        snapshot: typeof initialRuntimeSnapshot;
        playerGeneration: number;
        video: { dispatch: ReturnType<typeof vi.fn> };
        startPreparationMonitor(generation: number): void;
      };
      runtime.playerGeneration = 7;
      runtime.video = { dispatch: vi.fn() };
      runtime.snapshot = {
        ...structuredClone(initialRuntimeSnapshot),
        player: {
          ...structuredClone(initialRuntimeSnapshot.player),
          status: "preparing",
          stage: "loadingVideo",
        },
      };

      runtime.startPreparationMonitor(7);
      vi.advanceTimersByTime(PLAYBACK_PREPARATION_TIMEOUT_MS);

      expect(runtime.snapshot.player).toMatchObject({
        status: "error",
        stage: null,
        error: "Playback is taking longer than expected. Try again or choose another source.",
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("moves to a recoverable error when the video container cannot initialize", async () => {
    const runtime = new StremioCoreRuntime();
    await runtime.attachPlayer(document.createElement("div"));
    expect(runtime.getSnapshot().player).toMatchObject({
      status: "error",
      error: "Unable to play this source. Choose another source or try again.",
    });
  });
});
