import { describe, expect, it } from "vitest";
import { createSubtitleCueStore } from "../../runtime/subtitle-cue-store";
import { parseSubtitleCue, type NativeSubtitleCue } from "../../desktop/shared/subtitle-cue";

const cue = (patch: Partial<NativeSubtitleCue> = {}): NativeSubtitleCue => ({ playbackGeneration: 1, selectionGeneration: 1, seekGeneration: 0, sequence: 1, trackId: "2", kind: "text", text: "♫ First line\nSecond line ♫", startSeconds: 0, endSeconds: .1, ...patch });
describe("subtitle lifecycle ownership", () => {
  it("preserves Unicode and strips incompatible content from IPC", () => {
    expect(parseSubtitleCue(cue())?.text).toBe(cue().text);
    expect(parseSubtitleCue(cue({ kind: "authored" }))?.text).toBe("");
    expect(parseSubtitleCue(cue({ text: "♬".repeat(22000) }))).toBeNull();
    expect(parseSubtitleCue(cue({ sequence: Infinity }))).toBeNull();
  });
  it("rejects queued previous source, selection and seek cues", () => {
    const store = createSubtitleCueStore();
    expect(store.accept(cue())).toBe(true);
    store.clear();
    expect(store.accept(cue({ sequence: 2 }))).toBe(false);
    expect(store.accept(cue({ seekGeneration: 1, sequence: 3 }))).toBe(true);
    expect(store.accept(cue({ sequence: 4 }))).toBe(false);
    expect(store.accept(cue({ selectionGeneration: 2, seekGeneration: 0, sequence: 5 }))).toBe(true);
    expect(store.accept(cue({ playbackGeneration: 2, selectionGeneration: 0, sequence: 6 }))).toBe(true);
    expect(store.accept(cue({ sequence: 999 }))).toBe(false);
  });
  it("delivers identical text in distinct short cues and unsubscribes", () => {
    const store = createSubtitleCueStore();
    let updates = 0;
    const remove = store.subscribe(() => { updates++; });
    store.accept(cue());
    store.accept(cue({ sequence: 2, startSeconds: .1, endSeconds: .2 }));
    expect(updates).toBe(2);
    remove(); store.clear();
    expect(updates).toBe(2);
    expect(store.getSnapshot()).toBeNull();
  });
});
