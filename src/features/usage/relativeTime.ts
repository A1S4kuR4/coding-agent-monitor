import type { Language } from "./i18n";
import { dictFor } from "./i18n";

/**
 * Natural relative time for the "Updated X ago" label. `now` and the resolved
 * `lang` are injected so the function stays a pure, testable unit and the
 * label can tick without re-fetching.
 */
export function relativeTime(
  collectedAt: string,
  now: Date,
  lang: Language,
): string {
  const d = dictFor(lang);
  const then = new Date(collectedAt).getTime();
  if (Number.isNaN(then)) return d.justNow;

  const seconds = Math.max(0, Math.floor((now.getTime() - then) / 1000));
  if (seconds < 60) return d.justNow;

  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return d.minutesAgo(minutes);

  const hours = Math.floor(minutes / 60);
  if (hours < 24) return d.hoursAgo(hours);

  const days = Math.floor(hours / 24);
  return d.daysAgo(days);
}
