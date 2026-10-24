import { useQuery } from "@tanstack/react-query";

import { useApi } from "@/api/context";
import { inspectDevice } from "@/api/rpc";
import { useSettings } from "@/settings/context";
import { profileChoice } from "@/settings/settings";

/** The device's diagnosis and loader profile, cached until a write job ends. */
export function useDevice(root: string | null) {
  const { transport } = useApi();
  const { settings } = useSettings();
  const choice = profileChoice(settings);
  return useQuery({
    queryKey: ["device", root, choice],
    queryFn: () => inspectDevice(transport, root ?? "", choice),
    enabled: root !== null,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}
