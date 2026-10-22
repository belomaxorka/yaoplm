import type { Game } from "@/api/generated/Game";
import type { GameRef } from "@/api/generated/GameRef";

/**
 * A stable key for a scanned game: `ul.cfg` games by record index, images by
 * path. Used in routes and React keys; the same shape the CLI accepts.
 */
export function gameKey(game: Game): string {
  if (game.format === "UL" && game.ulIndex !== null) return `ul:${game.ulIndex}`;
  return `img:${game.path}`;
}

export function gameRef(game: Game): GameRef {
  if (game.format === "UL" && game.ulIndex !== null) {
    return { kind: "ul", index: game.ulIndex, startup: game.startup, name: game.title };
  }
  return { kind: "image", path: game.path };
}

export function findGame(games: readonly Game[], key: string): Game | undefined {
  return games.find((game) => gameKey(game) === key);
}
