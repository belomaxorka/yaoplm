import type { ConvertTarget } from "@/api/generated/ConvertTarget";
import type { Game } from "@/api/generated/Game";
import type { GameFormat } from "@/api/generated/GameFormat";

export const convertTargets = ["iso", "zso", "ul"] as const satisfies readonly ConvertTarget[];

export const formatOf: Record<ConvertTarget, GameFormat> = { iso: "ISO", zso: "ZSO", ul: "UL" };

/** The first target that changes something, ZSO first when the loader reads it. */
export function suggestTarget(games: readonly Game[], zsoReadable: boolean): ConvertTarget {
  const order: ConvertTarget[] = zsoReadable ? ["zso", "iso", "ul"] : ["iso", "ul"];
  return order.find((target) => games.some((game) => game.format !== formatOf[target])) ?? "iso";
}
