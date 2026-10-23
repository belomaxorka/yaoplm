import "@/i18n";

import { describe, expect, it } from "vitest";

import type { Game } from "@/api/generated/Game";
import { applyFilters, defaultFilters, filtersToParams, matchesQuery, parseFilters } from "@/features/library/filters";
import { GRID_GAP, GRID_MIN_COLUMN, gridLayout } from "@/features/library/gridLayout";
import i18n from "@/i18n";
import { formatBytes, formatDuration, formatEta, percent } from "@/lib/format";
import { findGame, gameKey, gameRef } from "@/lib/gameKey";

function game(overrides: Partial<Game>): Game {
  return {
    title: "Game",
    startup: "SLUS_200.62",
    media: "CD",
    format: "ISO",
    path: "E:\\CD\\SLUS_200.62.Game.iso",
    nameFormat: "old",
    parts: [],
    ulIndex: null,
    size: 700_000_000,
    imageStartup: null,
    layer1Start: null,
    cfg: null,
    art: [],
    cheats: null,
    warnings: [],
    knownTitle: null,
    dump: null,
    ...overrides,
  };
}

const games = [
  game({ title: "Zelda-like", startup: "SLUS_200.62", size: 300 }),
  game({ title: "Alpha Racer", startup: "SCES_500.03", media: "DVD", format: "ZSO", size: 900, path: "E:\\DVD\\Alpha Racer.zso" }),
  game({
    title: "Big Game",
    startup: "SLPM_123.45",
    media: "DVD",
    format: "UL",
    ulIndex: 0,
    size: 5_000_000_000,
    path: "E:\\ul.cfg",
    warnings: [{ code: "ulPartMissing", message: "part 2 missing" }],
  }),
];

describe("library filters", () => {
  it("sorts by title by default", () => {
    expect(applyFilters(games, defaultFilters).map((g) => g.title)).toEqual(["Alpha Racer", "Big Game", "Zelda-like"]);
  });

  it("filters by media, format and warnings", () => {
    expect(applyFilters(games, { ...defaultFilters, media: "DVD" }).map((g) => g.title)).toEqual(["Alpha Racer", "Big Game"]);
    expect(applyFilters(games, { ...defaultFilters, format: "UL" }).map((g) => g.title)).toEqual(["Big Game"]);
    expect(applyFilters(games, { ...defaultFilters, warnings: true }).map((g) => g.title)).toEqual(["Big Game"]);
  });

  it("matches titles, file names and IDs in any spelling", () => {
    const g = games[0]!;
    expect(matchesQuery(g, "zelda")).toBe(true);
    expect(matchesQuery(g, "slus20062")).toBe(true);
    expect(matchesQuery(g, "SLUS-200.62")).toBe(true);
    expect(matchesQuery(g, "game.iso")).toBe(true);
    expect(matchesQuery(g, "mario")).toBe(false);
  });

  it("sorts by size descending", () => {
    expect(applyFilters(games, { ...defaultFilters, sort: "size", desc: true }).map((g) => g.title)).toEqual([
      "Big Game",
      "Alpha Racer",
      "Zelda-like",
    ]);
  });

  it("round-trips through the address", () => {
    const filters = { ...defaultFilters, q: "big", media: "DVD" as const, warnings: true, sort: "size" as const, desc: true };
    expect(parseFilters(filtersToParams(filters))).toEqual(filters);
    expect(parseFilters(new URLSearchParams("media=Blu-ray&sort=nope"))).toEqual(defaultFilters);
  });
});

describe("grid layout", () => {
  it("fits as many columns as the minimum width allows", () => {
    expect(gridLayout(GRID_MIN_COLUMN - 1).columns).toBe(1);
    expect(gridLayout(GRID_MIN_COLUMN).columns).toBe(1);
    expect(gridLayout(2 * GRID_MIN_COLUMN + GRID_GAP).columns).toBe(2);
    expect(gridLayout(2 * GRID_MIN_COLUMN + GRID_GAP - 1).columns).toBe(1);
  });

  it("grows the row with the column, so a card never reaches the next row", () => {
    // A card: 8 px padding on each side, the 7:10 cover, 8 px gap, 56 px of text.
    for (let width = 300; width <= 2600; width += 37) {
      const { columns, rowHeight } = gridLayout(width);
      const column = (width - GRID_GAP * (columns - 1)) / columns;
      const card = 8 + ((column - 16) * 10) / 7 + 8 + 56 + 8;
      expect(rowHeight).toBeGreaterThanOrEqual(card + GRID_GAP);
      expect(rowHeight).toBeLessThan(card + GRID_GAP + 1);
    }
  });

  it("follows the root font size", () => {
    // 500 px: three 156 px columns. At 20 px per rem the padding is 10 px and the text 70 px.
    expect(gridLayout(500, 20).columns).toBe(3);
    expect(gridLayout(500, 20).rowHeight).toBe(Math.ceil(10 + ((156 - 20) * 10) / 7 + 10 + 70 + 10) + GRID_GAP);
  });
});

describe("game keys", () => {
  it("uses the ul.cfg index for ul games and the path otherwise", () => {
    expect(gameKey(games[2]!)).toBe("ul:0");
    expect(gameKey(games[0]!)).toBe("img:E:\\CD\\SLUS_200.62.Game.iso");
    expect(gameRef(games[2]!)).toEqual({ kind: "ul", index: 0, startup: "SLPM_123.45", name: "Big Game" });
    expect(gameRef(games[0]!)).toEqual({ kind: "image", path: "E:\\CD\\SLUS_200.62.Game.iso" });
    expect(findGame(games, "ul:0")?.title).toBe("Big Game");
    expect(findGame(games, "ul:7")).toBeUndefined();
  });
});

describe("formatting", () => {
  it("formats sizes like the CLI", async () => {
    await i18n.changeLanguage("en");
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.5 KiB");
    expect(formatBytes(4_700_372_992)).toBe("4.4 GiB");
    await i18n.changeLanguage("ru");
    expect(formatBytes(4_700_372_992)).toBe("4,4 ГиБ");
    await i18n.changeLanguage("en");
  });

  it("formats durations and ETAs", () => {
    expect(formatDuration(12)).toBe("12 s");
    expect(formatDuration(65)).toBe("1 min 05 s");
    expect(formatDuration(3700)).toBe("1 h 1 min");
    expect(formatEta(50, 100, 10)).toBe("5 s");
    expect(formatEta(50, 100, null)).toBeNull();
    expect(formatEta(100, 100, 10)).toBeNull();
    expect(percent(25, 100)).toBe(25);
    expect(percent(1, null)).toBeNull();
  });
});
