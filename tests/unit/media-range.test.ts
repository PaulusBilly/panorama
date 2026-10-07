import { describe, expect, it } from "vitest";
import { mediaValidator, validateMediaRange } from "../../desktop/main/media-range";
import { retryAfterDeadline } from "../../desktop/main/media-retry";

describe("media representation integrity", () => {
  it("accepts the exact interval including a short final chunk", () => {
    expect(validateMediaRange(new Headers({ "content-range": "bytes 8-9/10", "content-length": "2" }), 8, 15, 10)).toEqual({ start: 8, end: 9, total: 10 });
  });
  it.each<Record<string, string>>([
    { "content-range": "bytes 0-3/10" },
    { "content-range": "bytes 4-6/10" },
    { "content-range": "bytes 4-7/11" },
    { "content-range": "bytes 4-7/*" },
    { "content-range": "bytes 4-7/9007199254740992" },
    { "content-range": "bytes 4-7/10", "content-length": "5" },
    { "content-range": "bytes 4-7/10", "content-encoding": "gzip" },
    { "content-range": "bytes 4-7/10", "content-type": "multipart/byteranges" },
  ])("rejects incompatible headers %j", (headers) => {
    expect(() => validateMediaRange(new Headers(headers), 4, 7, 10)).toThrow();
  });
  it("prefers a strong ETag and excludes weak tags", () => {
    expect(mediaValidator(new Headers({ etag: '"v1"', "last-modified": "Sun, 04 Oct 2026 12:00:00 GMT" }))).toBe('"v1"');
    expect(mediaValidator(new Headers({ etag: 'W/"v1"' }))).toBeNull();
  });
  it("preserves long server deadlines and rejects malformed values", () => {
    expect(retryAfterDeadline("30", 1000)).toBe(31000);
    const now = Date.parse("Sun, 04 Oct 2026 12:00:00 GMT");
    expect(retryAfterDeadline("Sun, 04 Oct 2026 12:01:00 GMT", now)).toBe(now + 60000);
    expect(retryAfterDeadline("Sun, 04 Oct 2026 11:00:00 GMT", now)).toBe(now);
    for (const value of [null, "-1", "1.2", "garbage", "9999999999999999999"]) expect(retryAfterDeadline(value, now)).toBeNull();
  });
});
