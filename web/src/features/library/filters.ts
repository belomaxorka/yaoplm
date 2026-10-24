import type { Game } from "@/api/generated/Game";
import type { GameFormat } from "@/api/generated/GameFormat";
import type { Media } from "@/api/generated/Media";
import { hasCover } from "@/api/media";
import { baseName } from "@/lib/format";

export const sortKeys = ["title", "startup", "size", "media", "format"] as const;
export type SortKey = (typeof sortKeys)[number];

export interface LibraryFilters {
  q: string;
  media: Media | "all";
  format: GameFormat | "all";
  /** Only games with warnings. */
  warnings: boolean;
  /** Only games without a front cover (ART-4). */
  noCover: boolean;
  sort: SortKey;
  desc: boolean;
}

export const defaultFilters: LibraryFilters = {
  q: "",
  media: "all",
  format: "all",
  warnings: false,
  noCover: false,
  sort: "title",
  desc: false,
};

/**
 * The filters in effect: simple mode has no sorting or counter filters, so
 * those it hides do not apply, though they stay in the address and come
 * back with the advanced mode (UI-4).
 */
export function effectiveFilters(filters: LibraryFilters, simple: boolean): LibraryFilters {
  if (!simple) return filters;
  const { warnings, noCover, sort, desc } = defaultFilters;
  return { ...filters, warnings, noCover, sort, desc };
}

function isMedia(value: string | null): value is Media {
  return value === "CD" || value === "DVD";
}

function isFormat(value: string | null): value is GameFormat {
  return value === "ISO" || value === "ZSO" || value === "UL";
}

function isSortKey(value: string | null): value is SortKey {
  return sortKeys.includes(value as SortKey);
}

/** Filters live in the address, so Back from a game keeps them. */
export function parseFilters(params: URLSearchParams): LibraryFilters {
  const media = params.get("media");
  const format = params.get("format");
  const sort = params.get("sort");
  return {
    q: params.get("q") ?? "",
    media: isMedia(media) ? media : "all",
    format: isFormat(format) ? format : "all",
    warnings: params.get("warnings") === "1",
    noCover: params.get("nocover") === "1",
    sort: isSortKey(sort) ? sort : "title",
    desc: params.get("desc") === "1",
  };
}

export function filtersToParams(filters: LibraryFilters): URLSearchParams {
  const params = new URLSearchParams();
  if (filters.q) params.set("q", filters.q);
  if (filters.media !== "all") params.set("media", filters.media);
  if (filters.format !== "all") params.set("format", filters.format);
  if (filters.warnings) params.set("warnings", "1");
  if (filters.noCover) params.set("nocover", "1");
  if (filters.sort !== "title") params.set("sort", filters.sort);
  if (filters.desc) params.set("desc", "1");
  return params;
}

/** `SLUS_200.62`, `slus-20062` and `SLUS20062` all match the same game. */
function compactId(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9]/g, "");
}

export function matchesQuery(game: Game, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  if (game.title.toLowerCase().includes(q)) return true;
  if (baseName(game.path).toLowerCase().includes(q)) return true;
  const id = compactId(q);
  return id.length > 0 && compactId(game.startup).includes(id);
}

const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

function compare(a: Game, b: Game, key: SortKey): number {
  switch (key) {
    case "title":
      return collator.compare(a.title, b.title);
    case "startup":
      return collator.compare(a.startup, b.startup);
    case "size":
      return a.size - b.size;
    case "media":
      return collator.compare(a.media, b.media);
    case "format":
      return collator.compare(a.format, b.format);
  }
}

export function applyFilters(games: readonly Game[], filters: LibraryFilters): Game[] {
  const result = games.filter(
    (game) =>
      (filters.media === "all" || game.media === filters.media) &&
      (filters.format === "all" || game.format === filters.format) &&
      (!filters.warnings || game.warnings.length > 0) &&
      (!filters.noCover || !hasCover(game)) &&
      matchesQuery(game, filters.q),
  );
  result.sort((a, b) => {
    const order = compare(a, b, filters.sort) || collator.compare(a.title, b.title);
    return filters.desc ? -order : order;
  });
  return result;
}
