export const PROVISIONAL_NATIVE_DURATION_SECONDS = 30;

function validDuration(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;
}

export function resolveInitialPlaybackDuration(
  currentDuration: unknown,
  resumeDuration: unknown,
): number {
  return Math.max(
    validDuration(currentDuration) ?? 0,
    validDuration(resumeDuration) ?? 0,
  );
}

export function normalizeNativePlaybackDuration(valueMs: unknown): number | null {
  const durationMs = validDuration(valueMs);
  if (durationMs === null) return null;
  const durationSeconds = durationMs / 1000;
  return durationSeconds > PROVISIONAL_NATIVE_DURATION_SECONDS ? durationSeconds : null;
}
