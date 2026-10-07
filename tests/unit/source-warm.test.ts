import { describe, expect, it } from "vitest";
import { warmableSourceUrl } from "@/runtime/stremio-core-runtime";

const url = "https://resolver.example/playback/film.mkv";

describe("source warm-up eligibility", () => {
  it("warms only direct links the addon marks as instantly available", () => {
    expect(warmableSourceUrl({ url, name: "1080P ⚡ ⟨REMUX⟩" })).toBe(url);
    expect(warmableSourceUrl({ url, name: "1080P", description: "⚡ cached" })).toBe(url);
  });

  it("never warms uncached, undecorated, or non-HTTP sources", () => {
    expect(warmableSourceUrl({ url, name: "⏳ DVDRIP" })).toBeNull();
    expect(warmableSourceUrl({ url, name: "⚡ ⌛ queued" })).toBeNull();
    expect(warmableSourceUrl({ url, name: "1080P" })).toBeNull();
    expect(warmableSourceUrl({ url: "magnet:?xt=urn:btih:abc", name: "⚡" })).toBeNull();
    expect(warmableSourceUrl({ infoHash: "abc", name: "⚡" })).toBeNull();
    expect(warmableSourceUrl(null)).toBeNull();
  });
});
