/**
 * @agent-context: Compact time formatters for the install-timestamp pill.
 *
 * `relativeTime` returns a one or two character suffix form (matches Linear /
 * GitHub conventions) so it fits in the dense metadata pill row without
 * wrapping.
 *
 * `absoluteTime` returns the user's locale full datetime, used as tooltip on
 * the pill so the precise install moment is one hover away.
 */

const SECOND = 1;
const MINUTE = 60;
const HOUR = 60 * 60;
const DAY = 24 * HOUR;
const WEEK = 7 * DAY;
const MONTH = 30 * DAY;
const YEAR = 365 * DAY;

/**
 * Convert a unix timestamp (seconds) into a short relative string.
 * Examples: "just now", "5m", "2h", "3d", "2w", "5mo", "3y".
 *
 * Future timestamps (clock skew) clamp to "just now". Null and undefined
 * return null so callers can decide whether to render anything.
 */
export function relativeTime(unixSec: number | null | undefined, nowMs?: number): string | null {
  if (unixSec === null || unixSec === undefined || !Number.isFinite(unixSec)) {
    return null;
  }
  const now = (nowMs ?? Date.now()) / 1000;
  const diff = Math.max(0, now - unixSec);

  if (diff < 45 * SECOND) return "just now";
  if (diff < HOUR) return `${Math.round(diff / MINUTE)}m`;
  if (diff < DAY) return `${Math.round(diff / HOUR)}h`;
  if (diff < WEEK) return `${Math.round(diff / DAY)}d`;
  if (diff < MONTH) return `${Math.round(diff / WEEK)}w`;
  if (diff < YEAR) return `${Math.round(diff / MONTH)}mo`;
  return `${Math.round(diff / YEAR)}y`;
}

/**
 * Locale full datetime string for tooltip. Returns null when input missing.
 */
export function absoluteTime(unixSec: number | null | undefined): string | null {
  if (unixSec === null || unixSec === undefined || !Number.isFinite(unixSec)) {
    return null;
  }
  try {
    return new Date(unixSec * 1000).toLocaleString();
  } catch {
    return null;
  }
}
