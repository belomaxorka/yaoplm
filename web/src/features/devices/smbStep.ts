import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import { worstLevel } from "@/lib/notices";

/**
 * The next step of a folder read over SMB (ADR-0047): a whole system disk
 * gets a folder of its own, an empty folder gets OPL's folders. Neither
 * needs a loader: the console starts OPL from its memory card or stick.
 */
export function smbStep(device: DeviceInfo): "folder" | "init" | null {
  // A device nothing can be written to gets no step at all.
  if (device.mode.mode !== "smb" || worstLevel(device.advice) === "error") return null;
  if (device.advice.some((notice) => notice.code === "systemRoot")) return "folder";
  return device.loader.detected && !device.loader.detected.library ? "init" : null;
}

/**
 * The next step of a memory card emulator's SD (ADR-0048): first which OPL
 * with MMCE reads it (`ask`, the version question), then OPL's folders next
 * to the emulator's files. It never gets a loader: OPL runs from inside its
 * boot card.
 */
export function emulatorStep(device: DeviceInfo): "ask" | "init" | null {
  if (device.mode.mode !== "emulator" || worstLevel(device.advice) === "error") return null;
  if (device.askProfile) return "ask";
  return device.loader.detected && !device.loader.detected.library ? "init" : null;
}

/** The step that comes before anything else in the device's library. */
export function folderStep(device: DeviceInfo): "folder" | "init" | "ask" | null {
  return smbStep(device) ?? emulatorStep(device);
}
