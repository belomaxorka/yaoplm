import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";

import { useApi } from "@/api/context";
import { scanLibrary } from "@/api/rpc";
import { useSettings } from "@/settings/context";

/** The scanned library of the open root; rescanned after every write queue. */
export function useLibrary() {
  const { transport } = useApi();
  const { settings } = useSettings();
  const queryClient = useQueryClient();
  const { root, profile, verify } = settings;

  const query = useQuery({
    queryKey: ["library", root, profile, verify],
    queryFn: ({ signal }) => scanLibrary(transport, root ?? "", profile, verify, signal),
    enabled: root !== null,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });

  const rescan = useCallback(
    () => queryClient.invalidateQueries({ queryKey: ["library", root] }),
    [queryClient, root],
  );

  return { ...query, root, rescan };
}
