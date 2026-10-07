export type NativeSubtitleCue = {
  playbackGeneration: number;
  selectionGeneration: number;
  seekGeneration: number;
  sequence: number;
  trackId: string | null;
  kind: "text" | "bitmap" | "authored" | "none";
  text: string;
  startSeconds: number | null;
  endSeconds: number | null;
};

export function parseSubtitleCue(value: unknown): NativeSubtitleCue | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const cue = value as NativeSubtitleCue;
  if (![cue.playbackGeneration, cue.selectionGeneration, cue.seekGeneration, cue.sequence].every((entry) => Number.isSafeInteger(entry) && entry >= 0) || !["text", "bitmap", "authored", "none"].includes(cue.kind) || typeof cue.text !== "string" || new TextEncoder().encode(cue.text).length > 65536 || cue.text.includes("\u0000") || (cue.trackId !== null && (typeof cue.trackId !== "string" || !/^\d+$/.test(cue.trackId))) || ![cue.startSeconds, cue.endSeconds].every((entry) => entry === null || typeof entry === "number" && Number.isFinite(entry) && entry >= 0)) return null;
  return { playbackGeneration: cue.playbackGeneration, selectionGeneration: cue.selectionGeneration, seekGeneration: cue.seekGeneration, sequence: cue.sequence, trackId: cue.trackId, kind: cue.kind, text: cue.kind === "text" ? cue.text : "", startSeconds: cue.startSeconds, endSeconds: cue.endSeconds };
}
