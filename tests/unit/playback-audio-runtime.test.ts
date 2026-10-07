import { afterEach, describe, expect, it, vi } from "vitest";
import { StremioCoreRuntime } from "@/runtime/stremio-core-runtime";
import { initialRuntimeSnapshot } from "@/runtime/snapshot";
import type { RuntimeSnapshot } from "@/runtime/types";

const legacyAudioPreferencesKey = "panorama.playback.audio.v1";

type RuntimeInternals = {
  snapshot: typeof initialRuntimeSnapshot;
  playerGeneration: number;
  video: { dispatch(action: Record<string, unknown>): void } | null;
  videoSession: { device: "HTMLVideo" | "ShellVideo" } | null;
  autoSelectEnglishAudio: boolean;
  getSnapshot(): typeof initialRuntimeSnapshot;
  setPlaybackVolume(volume: number): void;
  setPlaybackMuted(muted: boolean): void;
  selectAudioTrack(trackId: string): void;
  applyPlaybackAudioState(): void;
  handleVideoProp(propName: string, value: unknown): void;
  seekPlayback(time: number): void;
};

afterEach(() => {
  window.localStorage.removeItem(legacyAudioPreferencesKey);
});

describe("playback audio runtime", () => {
  it("selects the first English track automatically and preserves a manual override", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    const dispatch = vi.fn();
    runtime.video = { dispatch };

    runtime.handleVideoProp("audioTracks", [
      { id: "native-ja", lang: "jpn", label: "Japanese" },
      { id: "native-en", lang: "eng", label: "English 5.1" },
      { id: "native-en-commentary", lang: "en", label: "Commentary" },
    ]);

    expect(dispatch).toHaveBeenCalledWith({
      type: "setProp",
      propName: "selectedAudioTrackId",
      propValue: "native-en",
    });
    expect(runtime.snapshot.player.audio.selectedId).toBe("audio-2");

    dispatch.mockClear();
    runtime.selectAudioTrack("audio-1");
    expect(runtime.autoSelectEnglishAudio).toBe(false);
    expect(dispatch).toHaveBeenCalledWith({
      type: "setProp",
      propName: "selectedAudioTrackId",
      propValue: "native-ja",
    });

    dispatch.mockClear();
    runtime.handleVideoProp("audioTracks", [
      { id: "native-ja", lang: "jpn", label: "Japanese" },
      { id: "native-en", lang: "eng", label: "English 5.1" },
    ]);
    expect(dispatch).not.toHaveBeenCalled();
    expect(runtime.snapshot.player.audio.selectedId).toBe("audio-1");
  });

  it("unmutes before changing volume and preserves the last nonzero level", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    const dispatch = vi.fn();
    runtime.video = { dispatch };

    runtime.setPlaybackVolume(0.65);

    expect(dispatch.mock.calls.map(([action]) => action)).toEqual([
      { type: "setProp", propName: "muted", propValue: false },
      { type: "setProp", propName: "volume", propValue: 65 },
    ]);
    expect(runtime.getSnapshot().player).toMatchObject({ volume: 0.65, muted: false });

    dispatch.mockClear();
    runtime.setPlaybackVolume(0);
    expect(dispatch.mock.calls.map(([action]) => action)).toEqual([
      { type: "setProp", propName: "volume", propValue: 0 },
      { type: "setProp", propName: "muted", propValue: true },
    ]);
    expect(window.localStorage.getItem(legacyAudioPreferencesKey)).toBeNull();

    dispatch.mockClear();
    runtime.setPlaybackMuted(false);
    expect(dispatch.mock.calls.map(([action]) => action)).toEqual([
      { type: "setProp", propName: "muted", propValue: false },
      { type: "setProp", propName: "volume", propValue: 65 },
    ]);
    expect(runtime.getSnapshot().player).toMatchObject({ volume: 0.65, muted: false });
  });

  it("allows native amplification up to 200%", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    const dispatch = vi.fn();
    runtime.video = { dispatch };

    runtime.setPlaybackVolume(1.65);

    expect(dispatch.mock.calls.map(([action]) => action)).toEqual([
      { type: "setProp", propName: "muted", propValue: false },
      { type: "setProp", propName: "volume", propValue: 165 },
    ]);
    expect(runtime.getSnapshot().player).toMatchObject({ volume: 1.65, muted: false });
  });

  it("starts at 100% and ignores legacy persisted audio preferences", () => {
    window.localStorage.setItem(legacyAudioPreferencesKey, JSON.stringify({
      version: 1,
      volume: 0.4,
      muted: true,
    }));
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    const dispatch = vi.fn();
    runtime.video = { dispatch };

    runtime.applyPlaybackAudioState();

    expect(dispatch.mock.calls.map(([action]) => action)).toEqual([
      { type: "setProp", propName: "muted", propValue: false },
      { type: "setProp", propName: "volume", propValue: 100 },
    ]);
  });

  it("restores session audio after ShellVideo has accepted the asynchronous stream", async () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    const dispatch = vi.fn();
    runtime.video = { dispatch };
    runtime.playerGeneration = 3;
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: { ...structuredClone(initialRuntimeSnapshot.player), status: "preparing", volume: 1.55 },
    };

    runtime.handleVideoProp("stream", {});
    expect(dispatch).not.toHaveBeenCalled();
    await Promise.resolve();

    expect(dispatch.mock.calls.map(([action]) => action)).toEqual([
      { type: "setProp", propName: "muted", propValue: false },
      { type: "setProp", propName: "volume", propValue: 155 },
    ]);
  });

  it("reasserts session audio when native playback reports ready", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    const dispatch = vi.fn();
    runtime.video = { dispatch };
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "buffering",
        stage: "loadingVideo",
        volume: 1.7,
      },
    };

    runtime.handleVideoProp("loaded", true);

    expect(dispatch.mock.calls.map(([action]) => action)).toEqual([
      { type: "setProp", propName: "muted", propValue: false },
      { type: "setProp", propName: "volume", propValue: 170 },
    ]);
    expect(runtime.snapshot.player).toMatchObject({ status: "ready", volume: 1.7, muted: false });
  });

  it("does not let default ShellVideo properties replace restored preferences", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    runtime.videoSession = { device: "ShellVideo" };
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: { ...structuredClone(initialRuntimeSnapshot.player), volume: 0.65, muted: false },
    };

    runtime.handleVideoProp("volume", 0);
    runtime.handleVideoProp("muted", true);

    expect(runtime.snapshot.player).toMatchObject({ volume: 0.65, muted: false });
  });

  it("keeps a known duration when native playback reports provisional or zero values", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: { ...structuredClone(initialRuntimeSnapshot.player), duration: 6120 },
    };

    runtime.handleVideoProp("duration", 0);
    expect(runtime.snapshot.player.duration).toBe(6120);

    runtime.handleVideoProp("duration", 30_000);
    expect(runtime.snapshot.player.duration).toBe(6120);

    runtime.handleVideoProp("duration", 6_200_000);
    expect(runtime.snapshot.player.duration).toBe(6200);
  });

  it("uses the selected file probe as authoritative over metadata and provisional MPV duration", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      details: {
        ...structuredClone(initialRuntimeSnapshot.details),
        metadata: {
          status: "ready",
          item: { runtime: "109 min" } as RuntimeSnapshot["details"]["metadata"]["item"],
          error: null,
        },
      },
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "ready",
        duration: 6540,
      },
    };

    runtime.handleVideoProp("videoParams", { durationMs: 6_487_250 });
    expect(runtime.snapshot.player.duration).toBe(6487.25);

    runtime.handleVideoProp("duration", 30_000);
    expect(runtime.snapshot.player.duration).toBe(6487.25);
  });

  it("keeps an unknown duration unknown until selected-file metadata arrives", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "ready",
        time: 0,
        duration: 0,
      },
    };

    runtime.handleVideoProp("duration", 30_000);
    runtime.handleVideoProp("time", 12_000);
    expect(runtime.snapshot.player).toMatchObject({ time: 12, duration: 0 });

    runtime.handleVideoProp("videoParams", { durationMs: 6_487_250 });
    expect(runtime.snapshot.player.duration).toBe(6487.25);
  });

  it("expands duration with playback time and does not clamp seeks to a stale maximum", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    const dispatch = vi.fn();
    runtime.video = { dispatch };
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "ready",
        time: 45,
        duration: 30,
      },
    };

    runtime.handleVideoProp("time", 60_000);
    expect(runtime.snapshot.player).toMatchObject({ time: 60, duration: 60 });

    runtime.seekPlayback(50);
    expect(dispatch).toHaveBeenCalledWith({ type: "setProp", propName: "time", propValue: 50_000 });
    expect(runtime.snapshot.player.time).toBe(50);
  });
});
