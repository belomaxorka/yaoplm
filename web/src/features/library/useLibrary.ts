import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect } from "react";

import { useApi } from "@/api/context";
import { scanLibrary } from "@/api/rpc";
import type { Transport } from "@/api/transport";
import { allowArt } from "@/features/add/desktop";
import { useSettings } from "@/settings/context";
import type { Settings } from "@/settings/settings";

function libraryQuery(transport: Transport, { root, profile, verify }: Pick<Settings, "root" | "profile" | "verify">) {
  return {
    queryKey: ["library", root, profile, verify],
    queryFn: ({ signal }: { signal: AbortSignal }) => scanLibrary(transport, root ?? "", profile, verify, signal),
    enabled: root !== null,
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

  const query = useQuery(libraryQuery(transport, settings));

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
  useQuery(libraryQuery(transport, settings));
  return null;
}
