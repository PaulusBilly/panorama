import type { NativeSubtitleCue } from "../desktop/shared/subtitle-cue";

export function createSubtitleCueStore() {
  let snapshot: NativeSubtitleCue | null = null;
  let latest: NativeSubtitleCue | null = null;
  let suspended = false;
  const listeners = new Set<() => void>();
  const notify = () => { for (const listener of listeners) listener(); };
  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    accept(cue: NativeSubtitleCue): boolean {
      if (latest) {
        const keys = ["playbackGeneration", "selectionGeneration", "seekGeneration", "sequence"] as const;
        let newer = false;
        for (const key of keys) {
          if (cue[key] < latest[key]) return false;
          if (cue[key] > latest[key]) { newer = true; break; }
        }
        if (!newer) return false;
        if (suspended && cue.playbackGeneration === latest.playbackGeneration && cue.selectionGeneration === latest.selectionGeneration && cue.seekGeneration === latest.seekGeneration) return false;
      } else if (suspended && cue.kind !== "none") return false;
      suspended = false;
      latest = cue;
      snapshot = cue;
      notify();
      return true;
    },
    clear() { snapshot = null; suspended = latest !== null; notify(); },
  };
}

export type SubtitleCueStore = ReturnType<typeof createSubtitleCueStore>;
