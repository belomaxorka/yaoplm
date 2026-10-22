import type { LoaderProfile } from "@/api/generated/LoaderProfile";
import type { NameStyle } from "@/api/generated/NameStyle";
import { readStorage, writeStorage } from "@/lib/storage";

export const profiles = ["Opl120Beta", "Opl110", "RiptOpl"] as const satisfies readonly LoaderProfile[];
export const nameStyles = ["idTitle", "title"] as const satisfies readonly NameStyle[];
export const viewModes = ["grid", "list"] as const;
export type ViewMode = (typeof viewModes)[number];

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
}

export const defaultSettings: Settings = {
  profile: "Opl120Beta",
  nameStyle: "idTitle",
  root: null,
  view: "grid",
  verify: true,
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
    };
  } catch {
    return defaultSettings;
  }
}

export function saveSettings(settings: Settings): void {
  writeStorage(STORAGE_KEY, JSON.stringify(settings));
}
