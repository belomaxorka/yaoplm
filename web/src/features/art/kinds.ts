import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import type { ArtType } from "@/api/generated/ArtType";
import { attribution } from "@/api/rpc";
import { enabledSources } from "@/features/art/sources";
import { errorText } from "@/lib/phrase";
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

export interface FetchableTypes {
  /** The types the enabled sources have; empty while `blocked`. */
  types: ReadonlySet<ArtType>;
  /**
   * Why no type can be downloaded now, the same for every type: the
   * sources' types are still loading or failed to load, or every source
   * is switched off.
   */
  blocked: { why: "loading" | "failed" | "noSources"; text: string } | null;
}

/**
 * The types the enabled picture sources have (the core lists them per
 * source in the About data, with whether this build can ask it); the rest
 * come from files and picture sets.
 */
export function useFetchableTypes(): FetchableTypes {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { settings } = useSettings();
  const about = useQuery({ queryKey: ["attribution"], queryFn: () => attribution(transport), staleTime: Infinity });
  if (enabledSources(settings, about.data?.art).length === 0) {
    const text = settings.simpleMode ? t("art.noSourcesSimple") : t("art.noSources");
    return { types: new Set(), blocked: { why: "noSources", text } };
  }
  if (about.isPending) return { types: new Set(), blocked: { why: "loading", text: t("art.sourcesLoading") } };
  if (about.isError) {
    return { types: new Set(), blocked: { why: "failed", text: t("art.sourcesFailed", { error: errorText(about.error) }) } };
  }
  const types = new Set<ArtType>();
  for (const source of about.data.art) {
    const enabled = settings.artSources[source.id as keyof typeof settings.artSources]?.enabled ?? false;
    if (enabled && source.available) for (const kind of source.kinds) types.add(kind);
  }
  return { types, blocked: null };
}
