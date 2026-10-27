import type { DeviceMode } from "@/api/generated/DeviceMode";
import type { LoaderProfile } from "@/api/generated/LoaderProfile";
import type { ModeOverride } from "@/api/generated/ModeOverride";
import type { NameStyle } from "@/api/generated/NameStyle";
import type { ProfileOverride } from "@/api/generated/ProfileOverride";
import { readStorage, writeStorage } from "@/lib/storage";

export const profiles = ["Opl120Beta", "Opl110", "RiptOpl"] as const satisfies readonly LoaderProfile[];
/**
 * A profile chosen for one device, or `unknown`: the user does not know which
 * OPL they run (ADR-0041); on a memory card emulator's SD, `unknownMmce`:
 * they do not know which OPL with MMCE (ADR-0048).
 */
export type ProfileOverrideValue = LoaderProfile | "unknown" | "unknownMmce";
/** What "I don't know" gets: OPL 1.1.0's rules, the ones every OPL can read (ADR-0041). */
export const UNKNOWN_RULES: LoaderProfile = "Opl110";
/**
 * What "I don't know" gets on a memory card emulator's SD: the MMCE fork's
 * rules, OPL 1.2.0 beta's; OPL 1.1.0 does not see games there (ADR-0048).
 */
export const UNKNOWN_MMCE_RULES: LoaderProfile = "Opl120Beta";
/** The answers about a memory card emulator's SD: builds with MMCE only (ADR-0048). */
export const mmceProfiles = ["Opl120Beta", "RiptOpl"] as const satisfies readonly LoaderProfile[];
/** How OPL reads a device (ADR-0047, ADR-0048). */
export const deviceModes = ["ps2", "smb", "emulator"] as const satisfies readonly DeviceMode[];
export const nameStyles = ["idTitle", "title"] as const satisfies readonly NameStyle[];
export const viewModes = ["grid", "list"] as const;
export type ViewMode = (typeof viewModes)[number];
/** Picture sources; each type tries them in its own order (ADR-0035). */
export const artSourceIds = ["xlenore", "oplm", "libretro", "screenscraper"] as const;
export type ArtSourceId = (typeof artSourceIds)[number];

/** A picture source: on or off, and an optional local mirror folder (ADR-0007). */
export interface ArtSourceSettings {
  enabled: boolean;
  /** A folder with the repository's layout; empty means the network. */
  mirror: string;
}

/** A ScreenScraper account, for a larger daily quota (ADR-0035). */
export interface ScreenScraperAccount {
  user: string;
  password: string;
}

/** Preferences kept in the browser (or the WebView) until a settings file lands. */
export interface Settings {
  /**
   * Show only what most users need (UI-4, ADR-0034): sorting, counters,
   * conversions, picture sets, the jobs page and technical details hide.
   */
  simpleMode: boolean;
  /**
   * Profiles chosen for single devices, by `ActiveProfile.key`. Every other
   * device gets the profile its files say (LIB-1); there is no global
   * profile (ADR-0040).
   */
  profileOverrides: Record<string, ProfileOverrideValue>;
  /**
   * Modes chosen for single devices, by the same key (ADR-0047). Every other
   * device gets its place's: removable media plugged into the PS2, folders
   * on the computer and network folders read over SMB; a memory card
   * emulator's SD gets its own by its files (ADR-0048).
   */
  modeOverrides: Record<string, DeviceMode>;
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
  /** Title the games being added by the bundled database (META-1). */
  titlesFromDb: boolean;
  /** Write the bundled game info into the CFG of every added game the table knows (META-3). */
  infoToCfg: boolean;
  /** Picture sources on or off, and their mirrors (ART-1). */
  artSources: Record<ArtSourceId, ArtSourceSettings>;
  /** Optional; the source works without one. */
  screenscraper: ScreenScraperAccount;
  /** A folder with GitHub's layout for loader releases (tests, offline use); empty: GitHub. */
  loaderMirror: string;
  /** Show "Update available" when a device's loader has a newer build in the cache (LDR-4). */
  offerLoaderUpdates: boolean;
  /** `<device key>|<installed SHA-256>` pairs already offered: "Not now" holds per pair. */
  loaderUpdatesOffered: string[];
}

export const defaultSettings: Settings = {
  simpleMode: true,
  profileOverrides: {},
  modeOverrides: {},
  nameStyle: "idTitle",
  root: null,
  view: "grid",
  verify: true,
  // Off by default (owner, 2026-10-26): a beginner needs no verdicts.
  checkOnAdd: false,
  coversOnAdd: true,
  titlesFromDb: true,
  infoToCfg: true,
  artSources: {
    xlenore: { enabled: true, mirror: "" },
    oplm: { enabled: true, mirror: "" },
    libretro: { enabled: true, mirror: "" },
    screenscraper: { enabled: true, mirror: "" },
  },
  screenscraper: { user: "", password: "" },
  loaderMirror: "",
  offerLoaderUpdates: true,
  loaderUpdatesOffered: [],
};

const STORAGE_KEY = "yaoplm.settings";

function isProfile(value: unknown): value is LoaderProfile {
  return profiles.includes(value as LoaderProfile);
}

// Saved settings may still hold the old global `loaderProfile` (or the
// older `profile`): both are ignored, the device decides (ADR-0040).
function readOverrides(value: unknown): Record<string, ProfileOverrideValue> {
  if (typeof value !== "object" || value === null) return {};
  return Object.fromEntries(
    Object.entries(value).filter(
      (entry): entry is [string, ProfileOverrideValue] =>
        isProfile(entry[1]) || entry[1] === "unknown" || entry[1] === "unknownMmce",
    ),
  );
}

function readModeOverrides(value: unknown): Record<string, DeviceMode> {
  if (typeof value !== "object" || value === null) return {};
  return Object.fromEntries(
    Object.entries(value).filter((entry): entry is [string, DeviceMode] =>
      deviceModes.includes(entry[1] as DeviceMode),
    ),
  );
}

/** What `ListDevices` and `InspectDevice` need to pick each device's profile and mode. */
export function deviceChoice(settings: Pick<Settings, "profileOverrides" | "modeOverrides">): {
  overrides: ProfileOverride[];
  modeOverrides: ModeOverride[];
} {
  return {
    overrides: Object.entries(settings.profileOverrides).map(([key, value]) => ({
      key,
      profile: value === "unknown" ? UNKNOWN_RULES : value === "unknownMmce" ? UNKNOWN_MMCE_RULES : value,
    })),
    modeOverrides: Object.entries(settings.modeOverrides).map(([key, mode]) => ({ key, mode })),
  };
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

function readAccount(value: unknown): ScreenScraperAccount {
  const raw = typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
  return {
    user: typeof raw.user === "string" ? raw.user : "",
    password: typeof raw.password === "string" ? raw.password : "",
  };
}

export function readSettings(): Settings {
  const raw = readStorage(STORAGE_KEY);
  if (!raw) return defaultSettings;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return defaultSettings;
    const p = parsed as Record<string, unknown>;
    return {
      simpleMode: typeof p.simpleMode === "boolean" ? p.simpleMode : defaultSettings.simpleMode,
      profileOverrides: readOverrides(p.profileOverrides),
      modeOverrides: readModeOverrides(p.modeOverrides),
      nameStyle: isNameStyle(p.nameStyle) ? p.nameStyle : defaultSettings.nameStyle,
      root: typeof p.root === "string" && p.root.length > 0 ? p.root : null,
      view: isViewMode(p.view) ? p.view : defaultSettings.view,
      verify: typeof p.verify === "boolean" ? p.verify : defaultSettings.verify,
      checkOnAdd: typeof p.checkOnAdd === "boolean" ? p.checkOnAdd : defaultSettings.checkOnAdd,
      coversOnAdd: typeof p.coversOnAdd === "boolean" ? p.coversOnAdd : defaultSettings.coversOnAdd,
      titlesFromDb: typeof p.titlesFromDb === "boolean" ? p.titlesFromDb : defaultSettings.titlesFromDb,
      infoToCfg: typeof p.infoToCfg === "boolean" ? p.infoToCfg : defaultSettings.infoToCfg,
      artSources: readArtSources(p.artSources),
      screenscraper: readAccount(p.screenscraper),
      loaderMirror: typeof p.loaderMirror === "string" ? p.loaderMirror : "",
      offerLoaderUpdates:
        typeof p.offerLoaderUpdates === "boolean" ? p.offerLoaderUpdates : defaultSettings.offerLoaderUpdates,
      loaderUpdatesOffered: Array.isArray(p.loaderUpdatesOffered)
        ? p.loaderUpdatesOffered.filter((k): k is string => typeof k === "string")
        : [],
    };
  } catch {
    return defaultSettings;
  }
}

export function saveSettings(settings: Settings): void {
  writeStorage(STORAGE_KEY, JSON.stringify(settings));
}
