import { describe, expect, it } from "vitest";
import { formatRatingCount, formatTimeLeft, formatTmdbRating } from "@/runtime/film-display";

describe("film display formatting", () => {
  it("keeps one decimal place for TMDB scores", () => {
    expect(formatTmdbRating(8)).toBe("8.0");
    expect(formatTmdbRating(7.64)).toBe("7.6");
  });

  it("uses locale separators and correct rating pluralization", () => {
    expect(formatRatingCount(2485)).toBe("2,485 ratings");
    expect(formatRatingCount(1)).toBe("1 rating");
  });

  it("formats time left for the resume pill", () => {
    expect(formatTimeLeft(1560, 6120)).toBe("1h 16m");
    expect(formatTimeLeft(3600, 6120)).toBe("42m");
    expect(formatTimeLeft(6100, 6120)).toBe("1m");
    expect(formatTimeLeft(120, null)).toBeNull();
  });
});
