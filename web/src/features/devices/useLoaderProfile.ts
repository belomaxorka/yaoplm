import type { ActiveProfile } from "@/api/generated/ActiveProfile";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { DeviceMode } from "@/api/generated/DeviceMode";
import type { LoaderProfile } from "@/api/generated/LoaderProfile";
import { useDevice } from "@/features/recovery/useDevice";
import { worstLevel } from "@/lib/notices";
import { useSettings } from "@/settings/context";

/**
 * The loader profile of the open library (LIB-1, ADR-0040): an override for
 * the device or what the device's files say. Until the device answers,
 * `ready` is false, `profile` is the default, OPL 1.2.0 beta, and `mode`
 * is null (the core then goes by the device's place, ADR-0043). `usable`
 * is false when the storage doctor finds the device unusable for OPL.
 */
export function useLoaderProfile(): {
  profile: LoaderProfile;
  active: ActiveProfile | null;
  mode: DeviceMode | null;
  device: DeviceInfo | null;
  ready: boolean;
  usable: boolean;
} {
  const { settings } = useSettings();
  const device = useDevice(settings.root);
  const active = device.data?.loader ?? null;
  return {
    profile: active?.profile ?? "Opl120Beta",
    active,
    mode: device.data?.mode.mode ?? null,
    device: device.data ?? null,
    ready: settings.root === null || device.data !== undefined || device.isError,
    usable: device.data === undefined || worstLevel(device.data.advice) !== "error",
  };
}
