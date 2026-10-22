import { useQuery } from "@tanstack/react-query";

import { useApi } from "@/api/context";
import { inspectDevice } from "@/api/rpc";
import { useSettings } from "@/settings/context";

/** The device's diagnosis, cached until a write job ends. */
export function useDevice(root: string | null) {
  const { transport } = useApi();
  const { settings } = useSettings();
  return useQuery({
    queryKey: ["device", root, settings.profile],
    queryFn: () => inspectDevice(transport, root ?? "", settings.profile),
    enabled: root !== null,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}
