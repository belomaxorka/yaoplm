import i18n from "@/i18n";

const UNITS = ["B", "KiB", "MiB", "GiB", "TiB"] as const;

/** The current language's decimal mark: `531.2` in English, `531,2` in Russian. */
function decimal(value: number, digits: number): string {
  return new Intl.NumberFormat(i18n.language, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
    useGrouping: false,
  }).format(value);
}

/** Binary units with one decimal, like the CLI: `531.2 MiB`, `531,2 МиБ`. */
export function formatBytes(bytes: number, digits = 1): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const label = i18n.t(`units.${UNITS[unit] ?? "B"}`);
  if (unit === 0) return `${Math.round(bytes)} ${label}`;
  return `${decimal(value, digits)} ${label}`;
}

export function formatRate(bytesPerSecond: number): string {
  return i18n.t("units.perSecond", { size: formatBytes(bytesPerSecond) });
}

/** `3 h 12 min`, `4 min 05 s`, `12 s`. */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return i18n.t("units.hoursMinutes", { h, m });
  if (m > 0) return i18n.t("units.minutesSeconds", { m, s: String(s).padStart(2, "0") });
  return i18n.t("units.seconds", { s });
}

/** Time left for `total - done` bytes at `rate` bytes per second. */
export function formatEta(done: number, total: number | null, rate: number | null): string | null {
  if (total === null || rate === null || rate <= 0 || total <= done) return null;
  return formatDuration((total - done) / rate);
}

export function percent(done: number, total: number | null): number | null {
  if (total === null || total <= 0) return null;
  return Math.min(100, Math.round((done / total) * 100));
}

export function formatTime(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString(i18n.language, { hour12: false });
}

export function formatDate(epochMs: number): string {
  return new Date(epochMs).toLocaleDateString(i18n.language);
}

/** The last path component, for file names in lists. */
export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const index = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return index >= 0 ? trimmed.slice(index + 1) : trimmed;
}
