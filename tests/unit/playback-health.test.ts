import { describe, expect, it } from "vitest";
import {
  averageRate,
  estimateBufferWait,
  formatMbps,
  formatWait,
  isSourceTooHeavy,
} from "../../components/usePlaybackHealth";

describe("playback health", () => {
  it("estimates the wait until enough media is buffered to resume", () => {
    expect(estimateBufferWait(0, 37.6, 1.4)).toBeCloseTo(134.3, 1);
    expect(estimateBufferWait(4, 20, 10)).toBe(2);
    expect(estimateBufferWait(8, 20, 10)).toBe(0);
    expect(estimateBufferWait(0, null, 10)).toBeNull();
    expect(estimateBufferWait(0, 20, 0)).toBeNull();
  });

  it("flags a source only after a stall with a sustained shortfall", () => {
    const slow = Array<number>(10).fill(1.4);
    expect(isSourceTooHeavy(slow, 37.6, 1)).toBe(true);
    expect(isSourceTooHeavy(slow, 37.6, 0)).toBe(false);
    expect(isSourceTooHeavy(slow.slice(0, 4), 37.6, 1)).toBe(false);
    expect(isSourceTooHeavy(Array<number>(10).fill(40), 37.6, 2)).toBe(false);
    expect(isSourceTooHeavy(slow, null, 1)).toBe(false);
  });

  it("averages only recent download samples", () => {
    expect(averageRate([])).toBeNull();
    expect(averageRate([...Array<number>(10).fill(100), ...Array<number>(10).fill(2)])).toBe(2);
  });

  it("formats rates and waits concisely", () => {
    expect(formatMbps(37.6)).toBe("38");
    expect(formatMbps(1.43)).toBe("1.4");
    expect(formatWait(12.4)).toBe("12 s");
    expect(formatWait(134)).toBe("2 min");
  });
});
