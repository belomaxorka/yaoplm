// Data the loader dialogs' tests share: catalogue builds,
// a device, a release offer, a stored ELF and a plan as the core sends them.
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { InstalledLoader } from "@/api/generated/InstalledLoader";
import type { JobResult } from "@/api/generated/JobResult";
import type { LoaderBuild } from "@/api/generated/LoaderBuild";
import type { PlanNotice } from "@/api/generated/PlanNotice";
import type { PlanView } from "@/api/generated/PlanView";
import type { Response } from "@/api/generated/Response";
import { defaultSettings, type Settings } from "@/settings/settings";

export const SHA = "a".repeat(64);

export function build(id: string, patch: Partial<LoaderBuild> = {}): LoaderBuild {
  const folder = id === "yaopl" ? "YAOPL" : "OPL";
  return {
    id,
    family: "opl",
    name: id === "opl-beta" ? "OPL 1.2.0 beta" : id === "opl-stable" ? "OPL 1.1.0" : "YAOPL",
    publisher: id === "yaopl" ? "fork" : "official",
    repo: "ps2homebrew/Open-PS2-Loader",
    profile: id === "opl-stable" ? "Opl110" : "Opl120Beta",
    recommended: id === "opl-beta",
    devices: ["usb", "mx4sio"],
    license: { spdx: "AFL-3.0", url: "https://github.com/ps2homebrew/Open-PS2-Loader/blob/master/LICENSE", authors: "OPL team" },
    summary: { en: `${id} in English`, ru: `${id} по-русски` },
    limits: {},
    docs: [{ kind: "readme", url: "https://github.com/ps2homebrew/Open-PS2-Loader#readme" }],
    channel: { tag: "latest", floating: true, prerelease: false },
    presets: [
      { id: "fmcb", path: "UPDT/OPL.ELF", title: null, devices: ["usb"], launchers: ["fmcb"] },
      { id: "ps2bbl", path: "APPS/OPNPS2LD.ELF", title: null, devices: ["usb"], launchers: ["ps2bbl"] },
      {
        id: "app",
        path: `APPS/${folder}/OPNPS2LD.ELF`,
        title: id === "yaopl" ? "YAOPL" : "Open PS2 Loader",
        devices: ["usb", "mx4sio"],
        launchers: ["oplApps", "ps2bblMx4sio", "osdmenu"],
      },
    ],
    defaultPresets: [
      { device: "usb", preset: "fmcb" },
      { device: "mx4sio", preset: "app" },
    ],
    ...patch,
  };
}

export function device(patch: Partial<DeviceInfo> = {}): DeviceInfo {
  return {
    root: "F:\\",
    label: "TESTS",
    fileSystem: "FAT32",
    fsName: "FAT32",
    totalBytes: 64 << 30,
    freeBytes: 60 << 30,
    clusterSize: 32768,
    sectorSize: 512,
    serial: "3C761797",
    kind: "removable",
    bus: "usb",
    model: null,
    external: true,
    system: false,
    readOnly: false,
    partition: null,
    advice: [],
    appleDouble: null,
    unfinished: [],
    loader: { profile: "Opl120Beta", source: "detected", key: "serial:3C761797", detected: null },
    installed: [],
    ...patch,
  };
}

export const offer = (languages: boolean): JobResult => ({
  type: "LoaderOffer",
  build: "opl-beta",
  tag: "latest",
  version: "v1.2.0-Beta-2271-b3a2199",
  date: "2026-10-20",
  file: { name: "OPNPS2LD.7z", url: "https://github.com/x", size: 1_500_000, sha256: SHA, sources: ["digest"] },
  member: "OPNPS2LD.ELF",
  languages: languages
    ? { name: "OPNPS2LD-LANGS.7z", url: "https://github.com/y", size: 900_000, sha256: "b".repeat(64), sources: ["digest"] }
    : null,
  releaseUrl: "https://github.com/ps2homebrew/Open-PS2-Loader/releases/tag/latest",
  commitUrl: "https://github.com/ps2homebrew/Open-PS2-Loader/commit/b3a2199",
  changelog: [{ text: "Fix exFAT on big sticks", link: null }],
  changelogMore: false,
  license: { spdx: "AFL-3.0", url: "https://github.com/ps2homebrew/Open-PS2-Loader/blob/master/LICENSE", authors: "OPL team" },
  offline: false,
  stored: null,
});

export const file: JobResult = {
  type: "LoaderFile",
  sha256: SHA,
  size: 1_200_000,
  version: "v1.2.0-Beta-2271-b3a2199",
  build: "opl-beta",
  tag: "latest",
  file: "OPNPS2LD.7z",
  member: "OPNPS2LD.ELF",
  source: "https://github.com/ps2homebrew/Open-PS2-Loader/releases/tag/latest",
  confirmed: true,
  stable: null,
  languages: false,
  storedAt: 0,
};

export function plan(notices: PlanNotice[] = [], kind: PlanView["kind"] = "installLoader"): Response {
  const view: PlanView = {
    id: `plan-${notices.length}`,
    kind,
    title: "Install OPL 1.2.0 beta",
    root: "F:\\",
    writeBytes: 1_200_000,
    deleteBytes: 0,
    freeBytes: 60 << 30,
    executable: !notices.some((n) => n.level === "error"),
    deletesUserData: false,
    transactions: [],
    skipped: [],
    notices,
  };
  return { type: "Plan", ...view };
}

export function storeSettings(settings: Partial<Settings>) {
  window.localStorage.setItem("yaoplm.settings", JSON.stringify({ ...defaultSettings, ...settings }));
}

/** OPL 1.2.0 beta 2271 at FreeMcBoot's path, with 2280 known and 2260 in the backup. */
export function installedLoader(patch: Partial<InstalledLoader> = {}): InstalledLoader {
  return {
    path: "UPDT/OPL.ELF",
    family: "OPL",
    version: "v1.2.0-Beta-2271-b3a2199",
    sha256: "c".repeat(64),
    build: "opl-beta",
    knownBy: "manifest",
    manifestStale: false,
    preset: "fmcb",
    device: "usb",
    language: null,
    latest: "v1.2.0-Beta-2280-0123abc",
    checkedAt: Date.UTC(2026, 9, 25, 12),
    freshness: "update",
    backup: {
      file: ".yaoplm/backup/loader/20261024-120000/UPDT_OPL.ELF",
      family: "OPL",
      version: "v1.2.0-Beta-2260-aaaaaaa",
    },
    ...patch,
  };
}
