import { afterEach, describe, expect, it } from "vitest";
import {
  readSubtitlePreferences,
  subtitlePreferencesKey,
  writeSubtitlePreferences,
} from "@/runtime/subtitle-preferences";
import { normalizeSubtitleAppearance } from "@/runtime/subtitle-appearance";
import { StremioCoreRuntime } from "@/runtime/stremio-core-runtime";

afterEach(() => window.localStorage.removeItem(subtitlePreferencesKey));

describe("subtitle preferences", () => {
  it("uses Panorama's requested appearance defaults", () => {
    expect(readSubtitlePreferences()).toEqual({
      verticalPosition: 15,
      style: {
        size: 100,
        fontSizePx: 38,
        lineHeight: 1.24,
        backgroundOpacity: 68,
        borderRadius: 0,
        textColor: "#ffffff",
        textOpacity: 100,
        backgroundColor: "rgba(0, 0, 0, 0.68)",
        outlineColor: "rgba(0, 0, 0, 0.78)",
        paddingX: 20,
        paddingY: 0,
        fontWeight: "regular",
      },
    });
  });

  it("persists appearance and clamps malformed stored values", () => {
    writeSubtitlePreferences({
      verticalPosition: 41,
      style: normalizeSubtitleAppearance({
        size: 173,
        textColor: "#eeeeee",
        backgroundColor: "#111111",
        outlineColor: "#222222",
        paddingX: 19,
        paddingY: 7,
        fontWeight: "bold",
      }),
    });
    expect(readSubtitlePreferences()).toMatchObject({
      verticalPosition: 41,
      style: { fontSizePx: 66, paddingX: 19, paddingY: 7, fontWeight: "bold" },
    });

    window.localStorage.setItem(subtitlePreferencesKey, JSON.stringify({
      verticalPosition: 140,
      style: { size: 900, paddingX: -4, paddingY: 90, fontWeight: "black" },
    }));
    expect(readSubtitlePreferences()).toMatchObject({
      verticalPosition: 100,
      style: { fontSizePx: 96, paddingX: 0, paddingY: 64, fontWeight: "regular" },
    });
  });

  it("falls back safely when storage is corrupt", () => {
    window.localStorage.setItem(subtitlePreferencesKey, "not json");
    expect(readSubtitlePreferences().style.size).toBe(100);
  });

  it("restores appearance in a new runtime without persisting subtitle delay", () => {
    const runtime = new StremioCoreRuntime();
    runtime.setSubtitleStyle({ size: 166, paddingX: 13, fontWeight: "semibold", textColor: "#ffe066", textOpacity: 55 });
    runtime.setSubtitleVerticalPosition(37);
    runtime.setSubtitleOffset(2.5);

    const restored = new StremioCoreRuntime().getSnapshot().player.subtitles;
    expect(restored).toMatchObject({
      offset: 0,
      verticalPosition: 37,
      style: { fontSizePx: 63, paddingX: 13, fontWeight: "medium", textColor: "#ffe066", textOpacity: 55 },
    });
  });
});
