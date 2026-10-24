import type { ActiveMode } from "@/api/generated/ActiveMode";
import type { ActiveProfile } from "@/api/generated/ActiveProfile";
import type { DetectedProfile } from "@/api/generated/DetectedProfile";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";

/** Test fixtures: a device as `InspectDevice` describes it. */

export const KEY = "serial:7E1CD2C5";

export function detected(patch: Partial<DetectedProfile> = {}): DetectedProfile {
  return {
    profile: "Opl120Beta",
    confidence: "default",
    reason: { key: "reasons.profileNoSigns", params: {} },
    reasonText: "no signs of a loader on the device",
    loaders: [],
    notices: [],
    library: true,
    ...patch,
  };
}

export function loader(patch: Partial<ActiveProfile> = {}): ActiveProfile {
  return { profile: "Opl120Beta", source: "detected", key: KEY, detected: detected(), ...patch };
}

export function mode(patch: Partial<ActiveMode> = {}): ActiveMode {
  return {
    mode: "ps2",
    source: "detected",
    detected: "ps2",
    place: { key: "reasons.placeRemovable", params: {} },
    placeText: "a removable device",
    ...patch,
  };
}

/** A folder on the computer's disk, read over SMB (ADR-0044). */
export const smbMode = mode({
  mode: "smb",
  detected: "smb",
  place: { key: "reasons.placeComputerDisk", params: {} },
  placeText: "a folder on the computer’s disk",
});

/** A FAT32 stick by default, prepared for OPL and asked about. */
export function testDevice(patch: Partial<DeviceInfo> = {}): DeviceInfo {
  return {
    root: "E:\\",
    label: null,
    fileSystem: "FAT32",
    fsName: "FAT32",
    totalBytes: 8 << 30,
    freeBytes: 4 << 30,
    clusterSize: 32768,
    sectorSize: 512,
    serial: "7E1CD2C5",
    kind: "removable",
    bus: "usb",
    model: null,
    external: true,
    system: false,
    readOnly: false,
    partition: null,
    advice: [],
    appleDouble: 0,
    unfinished: [],
    loader: loader(),
    mode: mode(),
    askProfile: true,
    ...patch,
  };
}
