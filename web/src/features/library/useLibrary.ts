import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect } from "react";

import { useApi } from "@/api/context";
import { scanLibrary } from "@/api/rpc";
import type { Transport } from "@/api/transport";
import type { DeviceMode } from "@/api/generated/DeviceMode";
import type { LoaderProfile } from "@/api/generated/LoaderProfile";
import { allowArt } from "@/features/add/desktop";
import { useLoaderProfile } from "@/features/devices/useLoaderProfile";
import { useSettings } from "@/settings/context";

/**
 * The scan of `root` under the device's loader profile (LIB-1) and mode
 * (ADR-0045: no fragment rule over SMB). It waits for both, so a device read
 * as OPL 1.1.0 is never scanned as 1.2.
 */
function libraryQuery(
  transport: Transport,
  root: string | null,
  verify: boolean,
  loader: { profile: LoaderProfile; mode: DeviceMode | null; ready: boolean },
) {
  const { profile, mode } = loader;
  return {
    queryKey: ["library", root, profile, mode, verify],
    queryFn: ({ signal }: { signal: AbortSignal }) => scanLibrary(transport, root ?? "", profile, mode, verify, signal),
    enabled: root !== null && loader.ready,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  } as const;
}

/** The scanned library of the open root; rescanned after every write queue. */
export function useLibrary() {
  const { transport } = useApi();
  const { settings } = useSettings();
  const queryClient = useQueryClient();
  const { root } = settings;
  const loader = useLoaderProfile();

  const query = useQuery(libraryQuery(transport, root, settings.verify, loader));

  const rescan = useCallback(
    () => queryClient.invalidateQueries({ queryKey: ["library", root] }),
    [queryClient, root],
  );

  // The desktop WebView may only read the open root's ART folder (ART-2).
  const scanned = query.data?.root;
  useEffect(() => {
    if (scanned) void allowArt(scanned);
  }, [scanned, query.dataUpdatedAt]);

  return { ...query, root, rescan };
}

/**
 * Keeps the open library's query observed on every screen. Without it,
 * leaving the library mid-scan (to a game, the jobs) drops the last
 * observer, which cancels the scan, and coming back starts another; a
 * rescan after a write also waits for the user to come back. Renders
 * nothing and reads nothing, so it never re-renders on library changes.
 */
export function LibraryKeeper() {
  const { transport } = useApi();
  const { settings } = useSettings();
  const loader = useLoaderProfile();
  useQuery(libraryQuery(transport, settings.root, settings.verify, loader));
  return null;
}
