export function retryAfterDeadline(value: string | null, nowMs: number): number | null {
  if (value === null) return null;
  const trimmed = value.trim();
  if (/^\d+$/.test(trimmed)) {
    const deadline = nowMs + Number(trimmed) * 1000;
    return Number.isSafeInteger(deadline) ? deadline : null;
  }
  if (!/^(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)/i.test(trimmed)) return null;
  const deadline = Date.parse(trimmed);
  return Number.isFinite(deadline) ? Math.max(nowMs, deadline) : null;
}
