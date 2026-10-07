import { normalizeSubtitleAppearance } from "./subtitle-appearance";
import {
  clampSubtitleVerticalPosition,
} from "./normalize";
import { initialRuntimeSnapshot } from "./snapshot";
import type { PanoramaSubtitleStyle, RuntimeSnapshot } from "./types";

export const subtitlePreferencesKey = "panorama.subtitle-preferences.v1";

export type SubtitlePreferences = {
  style: PanoramaSubtitleStyle;
  verticalPosition: number;
};

export function defaultSubtitlePreferences(): SubtitlePreferences {
  const subtitles = initialRuntimeSnapshot.player.subtitles;
  return {
    style: structuredClone(subtitles.style),
    verticalPosition: subtitles.verticalPosition,
  };
}

export function createRuntimeSnapshotWithSubtitlePreferences(): RuntimeSnapshot {
  const snapshot = structuredClone(initialRuntimeSnapshot);
  const preferences = readSubtitlePreferences();
  snapshot.player.subtitles.style = preferences.style;
  snapshot.player.subtitles.verticalPosition = preferences.verticalPosition;
  return snapshot;
}

export function normalizeSubtitlePreferences(value: unknown): SubtitlePreferences {
  const defaults = defaultSubtitlePreferences();
  if (!value || typeof value !== "object" || Array.isArray(value)) return defaults;
  const record = value as { style?: unknown; verticalPosition?: unknown };
  const style = record.style && typeof record.style === "object" && !Array.isArray(record.style)
    ? record.style as Partial<PanoramaSubtitleStyle>
    : {};
  const finite = (candidate: unknown, fallback: number): number =>
    typeof candidate === "number" && Number.isFinite(candidate) ? candidate : fallback;
  return {
    style: normalizeSubtitleAppearance(style),
    verticalPosition: clampSubtitleVerticalPosition(
      finite(record.verticalPosition, defaults.verticalPosition),
    ),
  };
}

export function readSubtitlePreferences(storage?: Storage | null): SubtitlePreferences {
  let target: Storage | null;
  try { target = storage ?? (typeof window === "undefined" ? null : window.localStorage); } catch { return defaultSubtitlePreferences(); }
  if (!target) return defaultSubtitlePreferences();
  try {
    const raw = target.getItem(subtitlePreferencesKey);
    return raw ? normalizeSubtitlePreferences(JSON.parse(raw)) : defaultSubtitlePreferences();
  } catch {
    return defaultSubtitlePreferences();
  }
}

export function hasStoredSubtitlePreferences(storage?: Storage | null): boolean {
  let target: Storage | null;
  try { target = storage ?? (typeof window === "undefined" ? null : window.localStorage); } catch { return false; }
  if (!target) return false;
  try {
    return target.getItem(subtitlePreferencesKey) !== null;
  } catch {
    return false;
  }
}

export function writeSubtitlePreferences(preferences: SubtitlePreferences, storage?: Storage | null): void {
  let target: Storage | null;
  try { target = storage ?? (typeof window === "undefined" ? null : window.localStorage); } catch { return; }
  if (!target) return;
  try {
    target.setItem(subtitlePreferencesKey, JSON.stringify(normalizeSubtitlePreferences(preferences)));
  } catch {}
}
