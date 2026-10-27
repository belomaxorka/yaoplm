import type { Game } from "@/api/generated/Game";
import type { GameFormat } from "@/api/generated/GameFormat";
import type { Media } from "@/api/generated/Media";
import { hasCover } from "@/api/media";
import { baseName } from "@/lib/format";

/** What the library sorts by: the title or the game ID (LIB-5). */
export const sortKeys = ["title", "startup"] as const;
export type SortKey = (typeof sortKeys)[number];

/** The sort choices as the toolbar lists them: the key and the direction as one. */
export const sortChoices = ["title", "title-desc", "startup", "startup-desc"] as const;
export type SortChoice = (typeof sortChoices)[number];

export function sortChoice({ sort, desc }: Pick<LibraryFilters, "sort" | "desc">): SortChoice {
  return desc ? `${sort}-desc` : sort;
}

export function fromSortChoice(choice: SortChoice): Pick<LibraryFilters, "sort" | "desc"> {
  const [sort, direction] = choice.split("-") as [SortKey, string | undefined];
  return { sort, desc: direction === "desc" };
}

export const mediaKinds: readonly Media[] = ["CD", "DVD"];
export const formatKinds: readonly GameFormat[] = ["ISO", "ZSO", "UL"];

export interface LibraryFilters {
  q: string;
  /** The disc types ticked; none means every type. */
  media: Media[];
  /** The formats ticked; none means every format. */
  format: GameFormat[];
  /** Only games with warnings. */
  warnings: boolean;
  /** Only games without a front cover (ART-4). */
  noCover: boolean;
  sort: SortKey;
  desc: boolean;
}

export const defaultFilters: LibraryFilters = {
  q: "",
  media: [],
  format: [],
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

/** How many boxes of the "Filter" menu are ticked, for its button. */
export function activeFilterCount(filters: LibraryFilters): number {
  return filters.media.length + filters.format.length + (filters.warnings ? 1 : 0) + (filters.noCover ? 1 : 0);
}

function isMedia(value: string): value is Media {
  return mediaKinds.includes(value as Media);
}

function isFormat(value: string): value is GameFormat {
  return formatKinds.includes(value as GameFormat);
}

function isSortKey(value: string | null): value is SortKey {
  return sortKeys.includes(value as SortKey);
}

function list<T extends string>(value: string | null, keep: (item: string) => item is T): T[] {
  return (value ?? "").split(",").filter(keep);
}

/** Filters live in the address, so Back from a game keeps them. */
export function parseFilters(params: URLSearchParams): LibraryFilters {
  const sort = params.get("sort");
  // An old key (size, media, format) in a saved address reads as Title A→Z.
  const sorted = isSortKey(sort);
  return {
    q: params.get("q") ?? "",
    media: list(params.get("media"), isMedia),
    format: list(params.get("format"), isFormat),
    warnings: params.get("warnings") === "1",
    noCover: params.get("nocover") === "1",
    sort: sorted ? sort : "title",
    desc: sort === null || sorted ? params.get("desc") === "1" : false,
  };
}

export function filtersToParams(filters: LibraryFilters): URLSearchParams {
  const params = new URLSearchParams();
  if (filters.q) params.set("q", filters.q);
  if (filters.media.length > 0) params.set("media", filters.media.join(","));
  if (filters.format.length > 0) params.set("format", filters.format.join(","));
  if (filters.warnings) params.set("warnings", "1");
  if (filters.noCover) params.set("nocover", "1");
  if (filters.sort !== "title") params.set("sort", filters.sort);
  if (filters.desc) params.set("desc", "1");
  return params;
}

const QUERY_KEY = "yaoplm.libraryQuery";

/**
 * The library's address, remembered for the way back from a game card: a
 * deletion ends on the library with the same filters and sorting
 * (walkthrough finding 11). Session storage: a new window starts clean.
 */
export function rememberLibraryQuery(search: string): void {
  try {
    const query = search.replace(/^\?/, "");
    if (query) window.sessionStorage.setItem(QUERY_KEY, query);
    else window.sessionStorage.removeItem(QUERY_KEY);
  } catch {
    // Storage may be unavailable; the way back is then the plain library.
  }
}

export function libraryHref(): string {
  try {
    const query = window.sessionStorage.getItem(QUERY_KEY);
    return query ? `/library?${query}` : "/library";
  } catch {
    return "/library";
  }
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
  }
}

export function applyFilters(games: readonly Game[], filters: LibraryFilters): Game[] {
  const result = games.filter(
    (game) =>
      (filters.media.length === 0 || filters.media.includes(game.media)) &&
      (filters.format.length === 0 || filters.format.includes(game.format)) &&
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
