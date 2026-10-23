import type { LoaderProfile } from "@/api/generated/LoaderProfile";
import type { NameStyle } from "@/api/generated/NameStyle";
import { readStorage, writeStorage } from "@/lib/storage";

export const profiles = ["Opl120Beta", "Opl110", "RiptOpl"] as const satisfies readonly LoaderProfile[];
export const nameStyles = ["idTitle", "title"] as const satisfies readonly NameStyle[];
export const viewModes = ["grid", "list"] as const;
export type ViewMode = (typeof viewModes)[number];
export const artSourceIds = ["xlenore", "libretro"] as const;
export type ArtSourceId = (typeof artSourceIds)[number];

/** A cover source: on or off, and an optional local mirror folder (ADR-0007). */
export interface ArtSourceSettings {
  enabled: boolean;
  /** A folder with the repository's layout; empty means the network. */
  mirror: string;
}

/** Preferences kept in the browser (or the WebView) until a settings file lands. */
export interface Settings {
  /** The loader the device is prepared for (SPEC §4, ADR-0004). */
  profile: LoaderProfile;
  /** `ID.Title.iso` (ADR-0006) or `Title.iso`. */
  nameStyle: NameStyle;
  /** The library root currently open. */
  root: string | null;
  view: ViewMode;
  /** Open every image while scanning to catch ID mismatches. */
  verify: boolean;
  /** Hash images while adding them and check them against Redump (GAME-5). */
  checkOnAdd: boolean;
  /** Look for covers of the games being added (ART-8). */
  coversOnAdd: boolean;
  /** Cover sources in priority order (ART-1). */
  artSources: Record<ArtSourceId, ArtSourceSettings>;
}

export const defaultSettings: Settings = {
  profile: "Opl120Beta",
  nameStyle: "idTitle",
  root: null,
  view: "grid",
  verify: true,
  checkOnAdd: true,
  coversOnAdd: true,
  artSources: {
    xlenore: { enabled: true, mirror: "" },
    libretro: { enabled: true, mirror: "" },
  },
};

const STORAGE_KEY = "yaoplm.settings";

function isProfile(value: unknown): value is LoaderProfile {
  return profiles.includes(value as LoaderProfile);
}

function isNameStyle(value: unknown): value is NameStyle {
  return nameStyles.includes(value as NameStyle);
}

function isViewMode(value: unknown): value is ViewMode {
  return viewModes.includes(value as ViewMode);
}

function readArtSources(value: unknown): Record<ArtSourceId, ArtSourceSettings> {
  const raw = typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
  const out = { ...defaultSettings.artSources };
  for (const id of artSourceIds) {
    const source = raw[id];
    if (typeof source !== "object" || source === null) continue;
    const s = source as Record<string, unknown>;
    out[id] = {
      enabled: typeof s.enabled === "boolean" ? s.enabled : true,
      mirror: typeof s.mirror === "string" ? s.mirror : "",
    };
  }
  return out;
}

export function readSettings(): Settings {
  const raw = readStorage(STORAGE_KEY);
  if (!raw) return defaultSettings;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return defaultSettings;
    const p = parsed as Record<string, unknown>;
    return {
      profile: isProfile(p.profile) ? p.profile : defaultSettings.profile,
      nameStyle: isNameStyle(p.nameStyle) ? p.nameStyle : defaultSettings.nameStyle,
      root: typeof p.root === "string" && p.root.length > 0 ? p.root : null,
      view: isViewMode(p.view) ? p.view : defaultSettings.view,
      verify: typeof p.verify === "boolean" ? p.verify : defaultSettings.verify,
      checkOnAdd: typeof p.checkOnAdd === "boolean" ? p.checkOnAdd : defaultSettings.checkOnAdd,
      coversOnAdd: typeof p.coversOnAdd === "boolean" ? p.coversOnAdd : defaultSettings.coversOnAdd,
      artSources: readArtSources(p.artSources),
    };
  } catch {
    return defaultSettings;
  }
}

export function saveSettings(settings: Settings): void {
  writeStorage(STORAGE_KEY, JSON.stringify(settings));
}
