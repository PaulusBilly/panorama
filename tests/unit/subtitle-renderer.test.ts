import { describe, expect, it } from "vitest";
import { createSubtitleCueStore } from "../../runtime/subtitle-cue-store";
import { createSubtitleRenderer } from "../../runtime/subtitle-renderer";
import { defaultSubtitlePreferences } from "../../runtime/subtitle-preferences";
import { normalizeSubtitleAppearance } from "../../runtime/subtitle-appearance";

describe("shared subtitle renderer", () => {
  it("renders one safe multiline block with independent appearance and cleanup", () => {
    const container = document.createElement("div");
    Object.defineProperty(container, "clientHeight", { value: 600 });
    const store = createSubtitleCueStore();
    const renderer = createSubtitleRenderer(container, store, defaultSubtitlePreferences());
    store.accept({ playbackGeneration: 1, selectionGeneration: 1, seekGeneration: 0, sequence: 1, trackId: "1", kind: "text", text: "♪ <script> ♬\r\nمرحبا é ♫", startSeconds: 0, endSeconds: 1 });
    const box = container.querySelector<HTMLElement>(".panorama-subtitle-box")!;
    expect(box.textContent).toBe("♪ <script> ♬\nمرحبا é ♫");
    expect(box.querySelector("script")).toBeNull();
    expect(container.querySelectorAll(".panorama-subtitle-layer")).toHaveLength(1);
    renderer.updateAppearance(normalizeSubtitleAppearance({ fontSizePx: 60, paddingX: 13, paddingY: 7, backgroundOpacity: 0, borderRadius: 32 }), 15);
    expect(box.style.fontSize).toBe("60px");
    expect(box.style.padding).toBe("7px 13px");
    expect(box.style.borderRadius).toBe("32px");
    expect(box.style.overflow).toBe("visible");
    store.clear();
    expect(box.textContent).toBe("");
    renderer.destroy();
    expect(container.children).toHaveLength(0);
  });
});
