import type { ArtSourceSetting } from "@/api/generated/ArtSourceSetting";
import { type ArtSourceId, artSourceIds, type Settings } from "@/settings/settings";

/** Display names and home pages of the cover sources (ADR-0007). */
export const artSourceInfo: Record<ArtSourceId, { name: string; url: string }> = {
  xlenore: { name: "xlenore/ps2-covers", url: "https://github.com/xlenore/ps2-covers" },
  libretro: { name: "libretro-thumbnails", url: "https://github.com/libretro-thumbnails/Sony_-_PlayStation_2" },
};

/** The per-source switches and mirrors from the settings, as `FetchArt` takes them. */
export function requestSources(settings: Settings): ArtSourceSetting[] {
  return artSourceIds.map((id) => {
    const source = settings.artSources[id];
    const mirror = source.mirror.trim();
    return { id, enabled: source.enabled, ...(mirror ? { mirror } : {}) };
  });
}

export function enabledSources(settings: Settings): ArtSourceId[] {
  return artSourceIds.filter((id) => settings.artSources[id].enabled);
}
