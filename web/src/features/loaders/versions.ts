import { useQuery } from "@tanstack/react-query";

import { useApi } from "@/api/context";
import type { LoaderBuild } from "@/api/generated/LoaderBuild";
import { loaderCatalog } from "@/api/rpc";

/** A numbered build: a beta, or YAOPL's nightly (ADR-0051). */
const BUILD = /^(v\d+(?:\.\d+)+-(?:Beta-|nightly\.)\d+)-([0-9a-f]{7})/i;

/** `v1.2.0-Beta-2271 (b3a2199)` for `v1.2.0-Beta-2271-b3a2199`, `v2.0.0-nightly.12 (0123abc)` for
 * `v2.0.0-nightly.12-0123abc-yaopl`; other versions as they are. */
export function versionLabel(version: string): string {
  const build = BUILD.exec(version);
  return build ? `${build[1]} (${build[2]})` : version;
}

/** `v1.2.0-Beta-2280` for a button; other versions as they are. */
export function versionShort(version: string): string {
  return BUILD.exec(version)?.[1] ?? version;
}

/** The commit a numbered build's version names. */
export function commitOf(version: string | null | undefined): string | null {
  return (version && BUILD.exec(version)?.[2]) || null;
}

/** "What changed since your version": GitHub's comparison of the two builds' commits. */
export function compareUrl(repo: string, from: string | null | undefined, to: string): string | null {
  const [a, b] = [commitOf(from), commitOf(to)];
  return a && b && a !== b ? `https://github.com/${repo}/compare/${a}...${b}` : null;
}

/** The loader catalogue in force (LDR-1), read once. */
export function useLoaderCatalog(enabled = true) {
  const { transport } = useApi();
  return useQuery({
    queryKey: ["loaderCatalog"],
    queryFn: () => loaderCatalog(transport),
    enabled,
    staleTime: Infinity,
  });
}

/** A build of the catalogue by id. */
export function useLoaderBuild(id: string | null | undefined): LoaderBuild | undefined {
  const catalog = useLoaderCatalog(Boolean(id));
  return catalog.data?.builds.find((b) => b.id === id);
}
