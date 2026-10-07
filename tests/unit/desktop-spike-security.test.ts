import { describe, expect, it } from "vitest";
import {
  createSpikeWindowOptions,
  isAllowedSpikeNavigation,
  isAllowedSpikeMediaUrl,
} from "../../desktop/spike/security";

describe("desktop spike security", () => {
  it("keeps Node and unsandboxed renderer capabilities unavailable", () => {
    const options = createSpikeWindowOptions("/app/preload.js");

    expect(options.webPreferences).toEqual({
      preload: "/app/preload.js",
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
      webSecurity: true,
    });
    expect(options.backgroundColor).toBe("#00000000");
  });

  it.each([
    "https://example.com/",
    "file:///tmp/index.html",
    "http://localhost:3000/",
    "http://127.0.0.1:3001/",
    "javascript:alert(1)",
  ])("rejects navigation outside the owned renderer origin: %s", (target) => {
    expect(isAllowedSpikeNavigation(target, "http://127.0.0.1:3000")).toBe(false);
  });

  it("allows navigation within the exact owned renderer origin", () => {
    expect(isAllowedSpikeNavigation(
      "http://127.0.0.1:3000/films/tmdb%3A1893/watch",
      "http://127.0.0.1:3000",
    )).toBe(true);
  });

  it("allows only the exact owned file when the spike uses a file renderer", () => {
    expect(isAllowedSpikeNavigation(
      "file:///app/index.html",
      "file:///app/index.html",
    )).toBe(true);
    expect(isAllowedSpikeNavigation(
      "file:///tmp/index.html",
      "file:///app/index.html",
    )).toBe(false);
  });

  it.each([
    "https://127.0.0.1:11470/media",
    "http://localhost:11470/media",
    "http://127.0.0.1:3000/media",
    "http://127.0.0.1:11470@evil.example/media",
    "file:///tmp/movie.mkv",
  ])("rejects media outside the exact Stremio Service origin: %s", (target) => {
    expect(isAllowedSpikeMediaUrl(target, "http://127.0.0.1:11470")).toBe(false);
  });

  it("allows a media URL on the exact Stremio Service origin", () => {
    expect(isAllowedSpikeMediaUrl(
      "http://127.0.0.1:11470/redacted/media",
      "http://127.0.0.1:11470",
    )).toBe(true);
  });
});
