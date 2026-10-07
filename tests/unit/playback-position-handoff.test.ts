import { describe, expect, it } from "vitest";
import { StremioCoreRuntime } from "@/runtime/stremio-core-runtime";
import { initialRuntimeSnapshot } from "@/runtime/snapshot";

describe("playback position handoff", () => {
  it.each(["preparing", "error"] as const)("does not replace a source-switch timestamp while %s", (status) => {
    const runtime = new StremioCoreRuntime() as unknown as {
      snapshot: typeof initialRuntimeSnapshot;
      handleVideoProp(propName: string, value: unknown): void;
    };
    runtime.snapshot = {
      ...structuredClone(initialRuntimeSnapshot),
      player: {
        ...structuredClone(initialRuntimeSnapshot.player),
        status,
        stage: status === "preparing" ? "loadingVideo" : null,
        time: 247,
      },
    };

    runtime.handleVideoProp("time", 0);

    expect(runtime.snapshot.player.time).toBe(247);
  });
});
