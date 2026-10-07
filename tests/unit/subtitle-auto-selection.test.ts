import { describe, expect, it, vi } from "vitest";
import { StremioCoreRuntime } from "@/runtime/stremio-core-runtime";
import { initialRuntimeSnapshot } from "@/runtime/snapshot";
import { NATIVE_ADDON_SUBTITLE_TITLE_PREFIX } from "@/runtime/video-engine";

type RuntimeInternals = {
  snapshot: typeof initialRuntimeSnapshot;
  subtitleTargets: Map<string, { origin: "embedded" | "addon"; engineId: string }>;
  autoSelectEnglishSubtitle: boolean;
  completePlaybackPreparation(): void;
  handleVideoProp(name: string, value: unknown): void;
  video: { dispatch: ReturnType<typeof vi.fn> };
  tryAutoSelectEnglishSubtitle(): void;
  selectSubtitle(trackId: string | null): void;
  confirmSubtitle(value: unknown, origin: "embedded" | "addon", generation: number): void;
  updateSubtitleTracks(tracks: unknown[], origin: "embedded" | "addon"): void;
};

describe("automatic English subtitle selection", () => {
  it("starts playback immediately and selects the English embedded track once video is ready", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    runtime.video = { dispatch: vi.fn() };
    runtime.snapshot = structuredClone(initialRuntimeSnapshot);
    runtime.snapshot.player.status = "preparing";
    runtime.snapshot.player.stage = "loadingVideo";
    runtime.updateSubtitleTracks([{ id: "EMBEDDED_2", lang: "eng", label: "English" }], "embedded");
    runtime.completePlaybackPreparation();
    expect(runtime.snapshot.player.status).toBe("ready");
    expect(runtime.snapshot.player.trackDiscoveryReady).toBe(true);
    expect(runtime.snapshot.player.subtitles.pendingId).toBeTruthy();
    expect(runtime.video.dispatch).toHaveBeenCalledWith({ type: "setProp", propName: "selectedSubtitlesTrackId", propValue: "EMBEDDED_2" });
  });

  it("selects an English addon track that arrives after playback started", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    runtime.video = { dispatch: vi.fn() };
    runtime.snapshot = structuredClone(initialRuntimeSnapshot);
    runtime.snapshot.player.status = "preparing";
    runtime.completePlaybackPreparation();
    expect(runtime.snapshot.player.status).toBe("ready");
    expect(runtime.snapshot.player.subtitles.pendingId).toBeNull();
    runtime.updateSubtitleTracks([{ id: "addon-en", lang: "en", label: "English", origin: "OpenSubtitles" }], "addon");
    expect(runtime.snapshot.player.subtitles.pendingId).toBeTruthy();
    expect(runtime.video.dispatch).toHaveBeenCalledWith({ type: "setProp", propName: "selectedExtraSubtitlesTrackId", propValue: "addon-en" });
  });

  it("selects the topmost English track and lets a manual choice win afterward", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    const dispatch = vi.fn();
    runtime.video = { dispatch };
    runtime.subtitleTargets = new Map([
      ["english-forced", { origin: "embedded", engineId: "EMBEDDED_2" }],
      ["english-addon", { origin: "addon", engineId: "addon-en" }],
    ]);
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "ready",
        trackDiscoveryReady: true,
        subtitles: {
          ...structuredClone(initialRuntimeSnapshot.player.subtitles),
          tracks: [
            { id: "english-forced", label: "English", language: "eng", origin: "embedded", sourceLabel: "Embedded 1 · Forced" },
            { id: "english-addon", label: "English", language: "en", origin: "addon", sourceLabel: "OpenSubtitles" },
          ],
        },
      },
    };

    runtime.tryAutoSelectEnglishSubtitle();
    expect(dispatch).toHaveBeenCalledWith({
      type: "setProp",
      propName: "selectedExtraSubtitlesTrackId",
      propValue: null,
    });
    expect(dispatch).toHaveBeenCalledWith({
      type: "setProp",
      propName: "selectedSubtitlesTrackId",
      propValue: "EMBEDDED_2",
    });
    expect(runtime.snapshot.player.subtitles.pendingId).toBe("english-forced");

    dispatch.mockClear();
    runtime.selectSubtitle("english-addon");
    expect(dispatch).toHaveBeenCalledWith({
      type: "setProp",
      propName: "selectedSubtitlesTrackId",
      propValue: null,
    });
    expect(dispatch).toHaveBeenCalledWith({
      type: "setProp",
      propName: "selectedExtraSubtitlesTrackId",
      propValue: "addon-en",
    });
    dispatch.mockClear();
    runtime.snapshot.player.subtitles.pendingId = null;
    runtime.snapshot.player.subtitles.selectedId = "english-addon";
    runtime.tryAutoSelectEnglishSubtitle();

    expect(runtime.autoSelectEnglishSubtitle).toBe(false);
    expect(dispatch).not.toHaveBeenCalled();
  });

  it("reapplies the current appearance when an addon subtitle finishes loading", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    const dispatch = vi.fn();
    runtime.video = { dispatch };
    runtime.subtitleTargets = new Map([
      ["english-addon", { origin: "addon", engineId: "addon-en" }],
    ]);
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        subtitles: {
          ...structuredClone(initialRuntimeSnapshot.player.subtitles),
          style: {
            ...structuredClone(initialRuntimeSnapshot.player.subtitles.style),
            paddingX: 17,
            paddingY: 9,
            fontWeight: "bold",
          },
        },
      },
    };

    runtime.confirmSubtitle({ id: "addon-en" }, "addon", 0);

    expect(dispatch).toHaveBeenCalledWith({ type: "setProp", propName: "extraSubtitlesPaddingX", propValue: 17 });
    expect(dispatch).toHaveBeenCalledWith({ type: "setProp", propName: "extraSubtitlesPaddingY", propValue: 9 });
    expect(dispatch).toHaveBeenCalledWith({ type: "setProp", propName: "extraSubtitlesFontWeight", propValue: "bold" });
    expect(dispatch).toHaveBeenCalledWith({
      type: "setProp",
      propName: "extraSubtitlesOutlineColor",
      propValue: "rgba(0, 0, 0, 0)",
    });
    expect(dispatch).toHaveBeenCalledWith({
      type: "setProp",
      propName: "subtitlesOutlineColor",
      propValue: "rgba(0, 0, 0, 0)",
    });
  });

  it("does not relist MPV-loaded addon tracks as embedded variants", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    runtime.snapshot = structuredClone(initialRuntimeSnapshot);

    runtime.updateSubtitleTracks([
      { id: "EMBEDDED_1", lang: "en", label: "English" },
      { id: "EMBEDDED_2", lang: "en", label: `${NATIVE_ADDON_SUBTITLE_TITLE_PREFIX}English` },
    ], "embedded");

    expect(runtime.snapshot.player.subtitles.tracks).toHaveLength(1);
    expect(runtime.snapshot.player.subtitles.tracks[0]).toMatchObject({
      origin: "embedded",
      id: "subtitle-embedded-1",
    });
  });
});
