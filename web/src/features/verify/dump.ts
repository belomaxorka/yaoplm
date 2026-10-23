import type { Game } from "@/api/generated/Game";
import type { VerifyStatus } from "@/api/generated/VerifyStatus";

/** Display order: the good news first, the problems after. */
export const verifyStatuses = ["verified", "badDump", "unknown", "failed"] as const satisfies readonly VerifyStatus[];

/** How many results have each status, in display order, without the empty ones. */
export function statusCounts(results: readonly { status: VerifyStatus }[]): [VerifyStatus, number][] {
  return verifyStatuses
    .map((status): [VerifyStatus, number] => [status, results.filter((r) => r.status === status).length])
    .filter(([, count]) => count > 0);
}

/** Games with no cached hashes: never checked, or changed since (GAME-5). */
export function uncheckedGames(games: readonly Game[]): Game[] {
  return games.filter((game) => game.dump === null);
}

export const statusColours = {
  verified: "text-emerald-600 dark:text-emerald-400",
  badDump: "text-destructive",
  unknown: "text-muted-foreground",
  failed: "text-destructive",
} as const satisfies Record<VerifyStatus, string>;
