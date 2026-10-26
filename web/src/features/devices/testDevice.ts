import type { ActiveMode } from "@/api/generated/ActiveMode";
import type { ActiveProfile } from "@/api/generated/ActiveProfile";
import type { DetectedProfile } from "@/api/generated/DetectedProfile";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { EmulatorInfo } from "@/api/generated/EmulatorInfo";

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

/** A folder on the computer's disk, read over SMB (ADR-0047). */
export const smbMode = mode({
  mode: "smb",
  detected: "smb",
  place: { key: "reasons.placeComputerDisk", params: {} },
  placeText: "a folder on the computer’s disk",
});

/** A memory card emulator's SD known by its files (ADR-0048). */
export const emulatorMode = mode({
  mode: "emulator",
  detected: "emulator",
  place: { key: "reasons.placeEmulator", params: { files: ".sd2psx, MemoryCards" } },
  placeText: "the files of a memory card emulator: .sd2psx, MemoryCards",
});

/** The owner's factory card, as `InspectDevice` describes it (research/19). */
export const emulatorInfo: EmulatorInfo = {
  kinds: ["sd2psx"],
  signs: [".sd2psx", "MemoryCards/PS2", "MemoryCards/PS1"],
  folder: "",
  protected: [".sd2psx", "MemoryCards", "civ.bin"],
  variants: [
    {
      name: "PS2",
      boot: [
        { channel: 1, name: "FMCB 1.966", file: "MemoryCards/PS2/BOOT/BootCard-1.mcd", bytes: 8 << 20 },
        { channel: 2, name: null, file: "MemoryCards/PS2/BOOT/BootCard-2.mcd", bytes: 8 << 20 },
      ],
      maxChannels: null,
      cardSize: null,
      cards: 3,
      gameCards: 1,
    },
  ],
  settings: [{ name: "PS2", entries: [{ key: "CardSize", value: "64" }] }],
  damaged: [],
};

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
    installed: [],
    emulator: null,
    ...patch,
  };
}
