/**
 * Natural relative time for the "Updated X ago" label. `now` is injected so the
 * function is a pure, testable unit and the label can tick without re-fetching.
 */
export function relativeTime(collectedAt: string, now: Date): string {
  const then = new Date(collectedAt).getTime();
  if (Number.isNaN(then)) return "just now";

  const seconds = Math.max(0, Math.floor((now.getTime() - then) / 1000));
  if (seconds < 60) return "just now";

  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;

  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;

  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}