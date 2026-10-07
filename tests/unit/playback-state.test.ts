import { describe, expect, it } from "vitest";
import {
  normalizeNativePlaybackDuration,
  resolveInitialPlaybackDuration,
} from "@/runtime/playback-state";

describe("playback state", () => {
  it("restores duration only from prior playback observations", () => {
    expect(resolveInitialPlaybackDuration(6200, 6120)).toBe(6200);
    expect(resolveInitialPlaybackDuration(30, null)).toBe(30);
    expect(resolveInitialPlaybackDuration(0, 6120)).toBe(6120);
    expect(resolveInitialPlaybackDuration(Number.NaN, null)).toBe(0);
  });

  it("rejects the native player's provisional 30-second duration", () => {
    expect(normalizeNativePlaybackDuration(0)).toBeNull();
    expect(normalizeNativePlaybackDuration(30_000)).toBeNull();
    expect(normalizeNativePlaybackDuration(Number.NaN)).toBeNull();
    expect(normalizeNativePlaybackDuration(30_001)).toBe(30.001);
    expect(normalizeNativePlaybackDuration(6_487_250)).toBe(6487.25);
  });
});
