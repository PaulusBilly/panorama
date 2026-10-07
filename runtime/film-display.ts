export function formatTmdbRating(rating: number): string {
  return rating.toFixed(1);
}

export function formatRatingCount(count: number, locale = "en-US"): string {
  return `${count.toLocaleString(locale)} ${count === 1 ? "rating" : "ratings"}`;
}

export function formatTimeLeft(offset: number, duration: number | null): string | null {
  if (!duration || duration <= 0) return null;
  const minutes = Math.max(1, Math.round(Math.max(0, duration - offset) / 60));
  const hours = Math.floor(minutes / 60);
  return hours > 0 ? `${hours}h ${minutes % 60}m` : `${minutes}m`;
}
