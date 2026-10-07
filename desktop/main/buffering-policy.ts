export type BufferingSample = {
  nowMs: number;
  bufferedSeconds: number | null;
  sourceMbps: number | null;
  downloadMbps: number | null;
  transferDemanded: boolean;
  buffering: boolean;
  paused: boolean;
  playbackSpeed: number;
  throttled: boolean;
};
export type BufferingDecision = { parallel: number; targetAheadSeconds: number; initialBufferSeconds: number; resumeBufferSeconds: number };

export class BufferingPolicy {
  private parallel = 3;
  private target = 60;
  private changedAt = 0;
  private activeSince: number | null = null;
  private rates: number[] = [];
  private probe: { previous: number; rate: number; until: number } | null = null;
  private wasBuffering = false;

  update(sample: BufferingSample): BufferingDecision {
    const demand = sample.sourceMbps === null ? null : sample.sourceMbps * Math.max(0.25, sample.playbackSpeed);
    if (sample.throttled) {
      this.parallel = Math.max(1, this.parallel - 1);
      this.changedAt = sample.nowMs;
      this.probe = null;
      this.activeSince = null;
      this.rates = [];
    } else if (!sample.transferDemanded || sample.paused) {
      if (this.probe) { this.parallel = this.probe.previous; this.changedAt = sample.nowMs; }
      this.activeSince = null;
      this.rates = [];
      this.probe = null;
    } else {
      this.activeSince ??= sample.nowMs;
      if (sample.downloadMbps !== null && Number.isFinite(sample.downloadMbps)) {
        this.rates.push(sample.downloadMbps);
        if (this.rates.length > 50) this.rates.shift();
      }
      const rate = this.rates.length ? this.rates.reduce((sum, value) => sum + value, 0) / this.rates.length : 0;
      if (sample.buffering && !this.wasBuffering && demand !== null && rate > demand * 1.1) this.target = Math.min(120, this.target + 15);
      if (this.probe && sample.nowMs >= this.probe.until) {
        if (rate < this.probe.rate * 1.1) this.parallel = this.probe.previous;
        this.probe = null;
        this.changedAt = sample.nowMs;
      } else if (!this.probe && sample.nowMs - this.changedAt >= 10_000 && sample.nowMs - this.activeSince >= 5000 && this.rates.length >= 5) {
        if (sample.bufferedSeconds !== null && sample.bufferedSeconds >= this.target + 15 && demand !== null && rate >= demand * 1.2) {
          this.parallel = 1;
          this.changedAt = sample.nowMs;
        } else if ((sample.bufferedSeconds ?? 0) < this.target - 15 && this.parallel < 6) {
          this.probe = { previous: this.parallel, rate, until: sample.nowMs + 5000 };
          this.parallel += 1;
          this.rates = [];
        }
      }
    }
    this.wasBuffering = sample.buffering;
    return { parallel: this.parallel, targetAheadSeconds: this.target, initialBufferSeconds: 5, resumeBufferSeconds: 5 };
  }
}
