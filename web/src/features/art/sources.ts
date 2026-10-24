import type { ArtSourceSetting } from "@/api/generated/ArtSourceSetting";
import type { SourceEntry } from "@/api/generated/SourceEntry";
import { type ArtSourceId, artSourceIds, type Settings } from "@/settings/settings";

/** Display names and home pages of the picture sources (ADR-0007, ADR-0034). */
export const artSourceInfo: Record<ArtSourceId, { name: string; url: string }> = {
  xlenore: { name: "xlenore/ps2-covers", url: "https://github.com/xlenore/ps2-covers" },
  oplm: { name: "OPL Manager backup (archive.org)", url: "https://archive.org/details/OPLM_ART_2024_09" },
  libretro: { name: "libretro-thumbnails", url: "https://github.com/libretro-thumbnails/Sony_-_PlayStation_2" },
  screenscraper: { name: "ScreenScraper", url: "https://www.screenscraper.fr" },
};

/** The sources that take a mirror folder: ScreenScraper is an API. */
export function takesMirror(id: ArtSourceId): boolean {
  return id !== "screenscraper";
}

/** The per-source switches, mirrors and account from the settings, as `FetchArt` takes them. */
export function requestSources(settings: Settings): ArtSourceSetting[] {
  return artSourceIds.map((id) => {
    const source = settings.artSources[id];
    if (!takesMirror(id)) {
      const user = settings.screenscraper.user.trim();
      const { password } = settings.screenscraper;
      return { id, enabled: source.enabled, ...(user && password ? { user, password } : {}) };
    }
    const mirror = source.mirror.trim();
    return { id, enabled: source.enabled, ...(mirror ? { mirror } : {}) };
  });
}

/**
 * The sources switched on, without those this build cannot ask
 * (ScreenScraper without a developer key) once the About data says so.
 */
export function enabledSources(settings: Settings, art?: readonly SourceEntry[]): ArtSourceId[] {
  return artSourceIds.filter(
    (id) => settings.artSources[id].enabled && art?.find((entry) => entry.id === id)?.available !== false,
  );
}
