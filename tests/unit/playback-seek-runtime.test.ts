import { describe, expect, it, vi } from "vitest";
import { StremioCoreRuntime } from "@/runtime/stremio-core-runtime";
import { initialRuntimeSnapshot } from "@/runtime/snapshot";

type RuntimeInternals = {
  snapshot: typeof initialRuntimeSnapshot;
  video: { dispatch(action: Record<string, unknown>): void } | null;
  handleVideoProp(propName: string, value: unknown): void;
  seekPlayback(time: number): void;
};

describe("playback seeking", () => {
  it("keeps the requested position while stale native time samples arrive", () => {
    const runtime = new StremioCoreRuntime() as unknown as RuntimeInternals;
    runtime.video = { dispatch: vi.fn() };
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status: "ready",
        time: 45,
        duration: 120,
      },
    };

    runtime.seekPlayback(75);
    runtime.handleVideoProp("time", 45_100);

    expect(runtime.snapshot.player.time).toBe(75);

    runtime.handleVideoProp("time", 75_000);
    expect(runtime.snapshot.player.time).toBe(75);
  });
});
