import { describe, expect, it } from "vitest";
import { BufferingPolicy, type BufferingSample } from "../../desktop/main/buffering-policy";

const sample = (nowMs: number, patch: Partial<BufferingSample> = {}): BufferingSample => ({ nowMs, bufferedSeconds: 10, sourceMbps: 8, downloadMbps: 12, transferDemanded: true, buffering: false, paused: false, playbackSpeed: 1, throttled: false, ...patch });
describe("measured buffering policy", () => {
  it("does not probe capacity during paused or intentionally idle downloads", () => {
    const policy = new BufferingPolicy();
    for (let now = 0; now <= 60000; now += 1000) expect(policy.update(sample(now, { transferDemanded: false })).parallel).toBe(3);
    expect(policy.update(sample(61000, { paused: true })).parallel).toBe(3);
  });
  it("reverts an extra connection when useful throughput does not improve", () => {
    const policy = new BufferingPolicy();
    for (let now = 0; now < 10000; now += 1000) policy.update(sample(now));
    expect(policy.update(sample(10000)).parallel).toBe(4);
    for (let now = 11000; now < 15000; now += 1000) policy.update(sample(now));
    expect(policy.update(sample(15000)).parallel).toBe(3);
  });
  it("retains a useful increase, then respects throttling and buffer limits", () => {
    const policy = new BufferingPolicy();
    for (let now = 0; now <= 10000; now += 1000) policy.update(sample(now));
    for (let now = 11000; now < 15000; now += 1000) policy.update(sample(now, { downloadMbps: 16 }));
    expect(policy.update(sample(15000, { downloadMbps: 16 })).parallel).toBe(4);
    expect(policy.update(sample(15001, { throttled: true })).parallel).toBe(3);
    for (let now = 16000; now < 100000; now += 1000) {
      const decision = policy.update(sample(now, { buffering: now % 2000 === 0, downloadMbps: 20 }));
      expect(decision.parallel).toBeGreaterThanOrEqual(1);
      expect(decision.parallel).toBeLessThanOrEqual(6);
      expect(decision.targetAheadSeconds).toBeLessThanOrEqual(120);
      expect(decision.resumeBufferSeconds).toBe(5);
    }
  });
});
