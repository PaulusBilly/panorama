"use client";

import { useEffect, useState } from "react";

// Matches the native cache-pause-wait: playback resumes after this much media
// is buffered ahead.
const RESUME_BUFFER_SECONDS = 5;
const POLL_INTERVAL_MS = 1_000;
const RATE_SAMPLES = 10;
const HEAVY_MIN_SAMPLES = 8;
const HEAVY_RATIO = 0.9;
// Below this much media ready ahead, the player is waiting on the network, so
// the download rate reflects what the connection can deliver. With more ready,
// the downloader idles by design and its rate says nothing about the source.
const DEMAND_BUFFER_SECONDS = 30;

export type PlaybackHealth = {
  bufferedUntil: number | null;
  waitSeconds: number | null;
  downloadMbps: number | null;
  sourceMbps: number | null;
  tooHeavy: boolean;
};

const emptyHealth: PlaybackHealth = {
  bufferedUntil: null,
  waitSeconds: null,
  downloadMbps: null,
  sourceMbps: null,
  tooHeavy: false,
};

export function averageRate(samples: number[]): number | null {
  const recent = samples.slice(-RATE_SAMPLES);
  return recent.length === 0 ? null : recent.reduce((total, value) => total + value, 0) / recent.length;
}

// Seconds until enough media is buffered to resume, from the source bitrate and
// the measured download rate.
export function estimateBufferWait(
  cacheSeconds: number | null,
  sourceMbps: number | null,
  downloadMbps: number | null,
  resumeBufferSeconds = RESUME_BUFFER_SECONDS,
): number | null {
  if (sourceMbps === null || downloadMbps === null || downloadMbps <= 0) return null;
  const missing = Math.max(0, resumeBufferSeconds - (cacheSeconds ?? 0));
  return missing * sourceMbps / downloadMbps;
}

// A source is too heavy once playback has stalled and the sustained download
// rate stays below what the file needs.
export function isSourceTooHeavy(samples: number[], sourceMbps: number | null, rebufferCount: number): boolean {
  if (sourceMbps === null || rebufferCount === 0 || samples.length < HEAVY_MIN_SAMPLES) return false;
  const rate = averageRate(samples);
  return rate !== null && rate < sourceMbps * HEAVY_RATIO;
}

export function formatMbps(value: number): string {
  return value >= 10 ? String(Math.round(value)) : value.toFixed(1);
}

export function formatWait(seconds: number): string {
  return seconds < 60 ? `${Math.max(1, Math.round(seconds))} s` : `${Math.round(seconds / 60)} min`;
}

export function usePlaybackHealth(enabled: boolean, sourceId: string | null): PlaybackHealth {
  const [health, setHealth] = useState<PlaybackHealth>(emptyHealth);

  useEffect(() => {
    const read = typeof window === "undefined" ? undefined : window.panoramaDesktop?.getPlaybackDiagnostics;
    if (!enabled || !read) return;
    let cancelled = false;
    const samples: number[] = [];
    const poll = async () => {
      try {
        const diagnostics = await read();
        if (cancelled || !diagnostics) return;
        const demanding = diagnostics.proxy?.transferDemanded ?? (diagnostics.buffering || (diagnostics.cacheSeconds ?? 0) < DEMAND_BUFFER_SECONDS);
        if (diagnostics.downloadMbps !== null && demanding) samples.push(diagnostics.downloadMbps);
        if (samples.length > RATE_SAMPLES * 3) samples.splice(0, samples.length - RATE_SAMPLES * 3);
        const downloadMbps = averageRate(samples);
        const tooHeavy = isSourceTooHeavy(samples, diagnostics.sourceBitrateMbps, diagnostics.rebufferCount);
        setHealth({
          bufferedUntil: diagnostics.cacheEndSeconds,
          waitSeconds: diagnostics.buffering
            ? estimateBufferWait(diagnostics.cacheSeconds, diagnostics.sourceBitrateMbps, downloadMbps, diagnostics.resumeBufferSeconds)
            : null,
          downloadMbps,
          sourceMbps: diagnostics.sourceBitrateMbps,
          tooHeavy,
        });
      } catch {}
    };
    void poll();
    const timer = setInterval(() => void poll(), POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
      setHealth(emptyHealth);
    };
  }, [enabled, sourceId]);

  return health;
}
