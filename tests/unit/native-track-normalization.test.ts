import { describe, expect, it } from "vitest";
import {
  embeddedSubtitleSourceLabel,
  preferredEnglishSubtitleTrack,
} from "../../runtime/normalize";

describe("native track normalization", () => {
  it("prefers the topmost English track while retaining numbered labels", () => {
    const tracks = [
      { id: "subtitle-embedded-1", label: "English", language: "eng", origin: "embedded" as const, sourceLabel: embeddedSubtitleSourceLabel(0, "Forced") },
      { id: "subtitle-embedded-2", label: "English", language: "eng", origin: "embedded" as const, sourceLabel: embeddedSubtitleSourceLabel(1, "SDH") },
    ];

    expect(preferredEnglishSubtitleTrack(tracks)?.id).toBe("subtitle-embedded-1");
    expect(tracks.map((track) => track.sourceLabel)).toEqual(["Embedded 1 · Forced", "Embedded 2 · SDH"]);
    expect(tracks.every((track) => !track.id.includes("EMBEDDED_"))).toBe(true);
  });

  it("returns a forced English track when it is available", () => {
    const forced = { id: "subtitle-embedded-1", label: "Forced", language: "en", origin: "embedded" as const, sourceLabel: "Embedded 1" };
    expect(preferredEnglishSubtitleTrack([forced])).toBe(forced);
  });
});
