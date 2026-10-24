import type { ActiveProfile } from "@/api/generated/ActiveProfile";
import type { LoaderProfile } from "@/api/generated/LoaderProfile";
import { useDevice } from "@/features/recovery/useDevice";
import { useSettings } from "@/settings/context";

/**
 * The loader profile of the open library (LIB-1, ADR-0040): an override for
 * the device or what the device's files say. Until the device answers,
 * `ready` is false and `profile` is the default, OPL 1.2.0 beta.
 */
export function useLoaderProfile(): { profile: LoaderProfile; active: ActiveProfile | null; ready: boolean } {
  const { settings } = useSettings();
  const device = useDevice(settings.root);
  const active = device.data?.loader ?? null;
  return {
    profile: active?.profile ?? "Opl120Beta",
    active,
    ready: settings.root === null || device.data !== undefined || device.isError,
  };
}
