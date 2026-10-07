import { describe, expect, it } from "vitest";
import { normalizeSubtitleAppearance, subtitleAppearanceCss, updateSubtitleAppearance } from "../../runtime/subtitle-appearance";

describe("precise subtitle appearance", () => {
  it("defaults legacy text opacity to fully visible and bounds invalid values", () => {
    expect(normalizeSubtitleAppearance({ textColor: "#ffe066" }).textOpacity).toBe(100);
    expect(normalizeSubtitleAppearance({ textOpacity: -1 }).textOpacity).toBe(0);
    expect(normalizeSubtitleAppearance({ textOpacity: 101 }).textOpacity).toBe(100);
    expect(normalizeSubtitleAppearance({ textOpacity: NaN }).textOpacity).toBe(100);
  });
  it("changes text opacity without fading the background box", () => {
    const css = subtitleAppearanceCss(normalizeSubtitleAppearance({ textColor: "#ffe066", textOpacity: 50, backgroundOpacity: 68 }));
    expect(css.color).toBe("color-mix(in srgb, #ffe066 50%, transparent)");
    expect(css.backgroundColor).toContain("68%");
    expect(css).not.toHaveProperty("opacity");
    expect(subtitleAppearanceCss(normalizeSubtitleAppearance({ textOpacity: 0 })).color).toContain("0%");
  });
  it("migrates legacy size and semibold while preserving box alpha", () => {
    expect(normalizeSubtitleAppearance({ size: 150, fontWeight: "semibold", backgroundColor: "rgba(0,0,0,.68)" })).toMatchObject({ fontSizePx: 57, size: 150, fontWeight: "medium", backgroundOpacity: 68 });
  });
  it("honors explicit pixels and clamps each independent field", () => {
    expect(normalizeSubtitleAppearance({ size: 150, fontSizePx: 200, lineHeight: 0, paddingX: 100, paddingY: -1, backgroundOpacity: 120, borderRadius: 100 })).toMatchObject({ fontSizePx: 96, lineHeight: 1, paddingX: 64, paddingY: 0, backgroundOpacity: 100, borderRadius: 32 });
    expect(normalizeSubtitleAppearance({ fontSizePx: NaN, lineHeight: Infinity })).toMatchObject({ fontSizePx: 38, lineHeight: 1.24 });
  });
  it("lets legacy and pixel updates work after migration", () => {
    const current = normalizeSubtitleAppearance({ fontSizePx: 48 });
    expect(updateSubtitleAppearance(current, { size: 150 }).fontSizePx).toBe(57);
    expect(updateSubtitleAppearance(current, { size: 150, fontSizePx: 60 }).fontSizePx).toBe(60);
  });
  it.each(["#0008", "#00000088", "rgba(0,0,0,53%)", "hsla(0,0%,0%,.53)"])("separates background alpha from text for %s", (backgroundColor) => {
    const style = normalizeSubtitleAppearance({ backgroundColor, backgroundOpacity: 50, paddingX: 13, paddingY: 7 });
    const css = subtitleAppearanceCss(style);
    expect(css.backgroundColor).toContain("50%");
    expect(css.padding).toBe("7px 13px");
    expect(css).not.toHaveProperty("opacity");
  });
  it("rejects unsafe colors without losing the remaining appearance", () => {
    expect(normalizeSubtitleAppearance({ backgroundColor: "url(https://example.test)", textColor: "var(--private)", fontSizePx: 40 })).toMatchObject({ fontSizePx: 40, textColor: "#ffffff", backgroundColor: "rgba(0, 0, 0, 0.68)" });
  });
});
