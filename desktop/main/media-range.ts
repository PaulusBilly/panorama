export class MediaRepresentationError extends Error {}

export function validateMediaRange(headers: Headers, expectedStart: number, expectedEnd: number, expectedTotal: number | null): { start: number; end: number; total: number } {
  const match = /^bytes (\d+)-(\d+)\/(\d+)$/.exec(headers.get("content-range") ?? "");
  if (!match) throw new MediaRepresentationError("Invalid media range");
  const [start, end, total] = match.slice(1).map(Number);
  if (![start, end, total, expectedStart, expectedEnd].every(Number.isSafeInteger) || start < 0 || end < start || total <= end || start !== expectedStart || end !== Math.min(expectedEnd, total - 1) || (expectedTotal !== null && total !== expectedTotal)) throw new MediaRepresentationError("Media range changed");
  const encoding = headers.get("content-encoding");
  if (encoding && encoding !== "identity") throw new MediaRepresentationError("Encoded media range");
  if (headers.get("content-type")?.toLowerCase().startsWith("multipart/")) throw new MediaRepresentationError("Multipart media range");
  const length = headers.get("content-length");
  if (length !== null && (!/^\d+$/.test(length) || Number(length) !== end - start + 1)) throw new MediaRepresentationError("Invalid media range length");
  return { start, end, total };
}

export function mediaValidator(headers: Headers): string | null {
  const etag = headers.get("etag");
  if (etag && /^"[^"\r\n]*"$/.test(etag)) return etag;
  const modified = headers.get("last-modified");
  return modified && Number.isFinite(Date.parse(modified)) ? modified : null;
}
