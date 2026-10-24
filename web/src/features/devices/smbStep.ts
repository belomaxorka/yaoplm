import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import { worstLevel } from "@/lib/notices";

/**
 * The next step of a folder read over SMB (ADR-0044): a whole system disk
 * gets a folder of its own, an empty folder gets OPL's folders. Neither
 * needs a loader: the console starts OPL from its memory card or stick.
 */
export function smbStep(device: DeviceInfo): "folder" | "init" | null {
  // A device nothing can be written to gets no step at all.
  if (device.mode.mode !== "smb" || worstLevel(device.advice) === "error") return null;
  if (device.advice.some((notice) => notice.code === "systemRoot")) return "folder";
  return device.loader.detected && !device.loader.detected.library ? "init" : null;
}
