import { useCallback, useEffect, useRef, useState } from "react";

import { useApi } from "@/api/context";
import type { CoverSearch } from "@/api/generated/CoverSearch";
import type { JobId } from "@/api/generated/JobId";
import type { PlanView } from "@/api/generated/PlanView";
import { fetchArt, waitForJob } from "@/api/rpc";
import { requestSources } from "@/features/art/sources";
import { useJobs } from "@/jobs/context";
import { errorText } from "@/lib/phrase";
import { useSettings } from "@/settings/context";

export interface CoverSearchState {
  /** Game IDs the search covers, upper case. */
  ids: ReadonlySet<string>;
  job: JobId | null;
  status: "running" | "done" | "stopped" | "failed";
  result: CoverSearch | null;
  error: string | null;
}

/** The game IDs an import plan adds. */
function planIds(plan: PlanView): Set<string> {
  return new Set(plan.transactions.flatMap((tx) => (tx.game ? [tx.game.startup.toUpperCase()] : [])));
}

/**
 * Covers for the games an import plan adds (ART-8, SPEC §5.4.1): the same
 * FetchArt job as "Download covers", scoped to the plan. A search runs once
 * per set of game IDs; a plan that adds a game the last search did not see
 * starts a new one (the cache answers for the rest). Stopping it keeps the
 * games, without covers.
 */
export function useCoverSearch(root: string | null, plan: PlanView | null, enabled: boolean) {
  const { transport } = useApi();
  const { settings } = useSettings();
  const { cancel } = useJobs();
  const [state, setState] = useState<CoverSearchState | null>(null);
  // The last search, for effects; null once covers are switched off.
  const latest = useRef<CoverSearchState | null>(null);
  const generation = useRef(0);

  useEffect(() => {
    latest.current = enabled ? state : null;
  }, [state, enabled]);

  const stopJob = useCallback(() => {
    const job = latest.current?.status === "running" ? latest.current.job : null;
    if (job !== null) void cancel(job);
  }, [cancel]);

  const start = useCallback(
    (root: string, plan: PlanView, ids: Set<string>) => {
      const mine = ++generation.current;
      const update = (patch: Partial<CoverSearchState>) => {
        if (mine === generation.current) setState((s) => (s ? { ...s, ...patch } : s));
      };
      setState({ ids, job: null, status: "running", result: null, error: null });
      void (async () => {
        try {
          const job = await fetchArt(transport, {
            type: "FetchArt",
            root,
            profile: settings.profile,
            sources: requestSources(settings),
            import: plan.id,
          });
          if (mine !== generation.current) {
            void cancel(job);
            return;
          }
          update({ job });
          const info = await waitForJob(transport, job);
          if (info.status === "finished" && info.result?.type === "Covers") {
            const { type: _type, ...result } = info.result;
            update({ status: "done", result });
          } else if (info.status === "cancelled") {
            update({ status: "stopped" });
          } else {
            update({ status: "failed", error: info.error ? errorText(info.error) : null });
          }
        } catch (error) {
          update({ status: "failed", error: errorText(error) });
        }
      })();
    },
    [transport, settings, cancel],
  );

  useEffect(() => {
    if (!enabled) {
      generation.current += 1;
      stopJob();
      latest.current = null;
      return;
    }
    if (!plan || !root) return;
    const ids = planIds(plan);
    if (ids.size === 0) return;
    const last = latest.current;
    if (last && last.status !== "failed" && [...ids].every((id) => last.ids.has(id))) return;
    stopJob();
    start(root, plan, ids);
    // A new plan is what starts a search; the settings are read as they are then.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plan?.id, enabled, root]);

  // Leaving the page stops a search nobody waits for.
  useEffect(() => stopJob, [stopJob]);

  const stop = useCallback(() => {
    stopJob();
    generation.current += 1;
    setState((s) => (s ? { ...s, status: "stopped" } : s));
  }, [stopJob]);

  return { state: enabled ? state : null, stop };
}
