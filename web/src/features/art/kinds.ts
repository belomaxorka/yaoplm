import { useQuery } from "@tanstack/react-query";

import { useApi } from "@/api/context";
import type { ArtType } from "@/api/generated/ArtType";
import { attribution } from "@/api/rpc";
import { useSettings } from "@/settings/context";

/** Picture types in the order the game card shows them (SPEC §5.4.1). */
export const artTypes: readonly ArtType[] = ["COV", "ICO", "COV2", "BG", "SCR", "SCR2", "LGO", "LAB"];

/**
 * Width over height of each type at OPL Manager's sizes (research/02 §7),
 * for previews in the right shape.
 */
export const artAspect: Record<ArtType, number> = {
  COV: 140 / 200,
  COV2: 242 / 344,
  LAB: 18 / 240,
  ICO: 1,
  LGO: 300 / 125,
  SCR: 250 / 188,
  SCR2: 250 / 188,
  BG: 640 / 480,
};

/**
 * The types the enabled cover sources have (the core lists them per
 * source in the About data); the rest come from files and picture sets.
 */
export function useFetchableTypes(): ReadonlySet<ArtType> {
  const { transport } = useApi();
  const { settings } = useSettings();
  const about = useQuery({ queryKey: ["attribution"], queryFn: () => attribution(transport), staleTime: Infinity });
  const types = new Set<ArtType>();
  for (const source of about.data?.art ?? []) {
    const enabled = settings.artSources[source.id as keyof typeof settings.artSources]?.enabled ?? false;
    if (enabled) for (const kind of source.kinds) types.add(kind);
  }
  return types;
}
