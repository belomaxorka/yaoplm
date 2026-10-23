import { useCallback, useEffect, useState } from "react";

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
  /** Tells searches apart: each one runs its own job. */
  search: number;
  root: string;
  /** The plan the search started from. */
  plan: string;
  /** Game IDs the search covers, upper case. */
  ids: ReadonlySet<string>;
  job: JobId | null;
  status: "running" | "done" | "stopped" | "failed";
  result: CoverSearch | null;
  error: string | null;
}

interface Store {
  /** Searches started so far, for `CoverSearchState.search`. */
  started: number;
  search: CoverSearchState | null;
  /** A plan no search may start from: it is about to be replaced. */
  skip: string | null;
}

/** The game IDs an import plan adds. */
function planIds(plan: PlanView): Set<string> {
  return new Set(plan.transactions.flatMap((tx) => (tx.game ? [tx.game.startup.toUpperCase()] : [])));
}

/**
 * Whether `plan` needs a new search: it adds a game the last search did not
 * look for, or that search failed for an earlier plan.
 */
export function needsSearch(search: CoverSearchState | null, plan: PlanView): boolean {
  const ids = planIds(plan);
  if (ids.size === 0) return false;
  if (!search) return true;
  if (search.status === "failed" && search.plan !== plan.id) return true;
  return [...ids].some((id) => !search.ids.has(id));
}

function begin(store: Store, root: string, plan: PlanView): Store {
  const search = store.started + 1;
  return {
    started: search,
    search: { search, root, plan: plan.id, ids: planIds(plan), job: null, status: "running", result: null, error: null },
    skip: null,
  };
}

/**
 * Covers for the games an import plan adds (ART-8, SPEC §5.4.1): the same
 * FetchArt job as "Download pictures", scoped to the plan. A search runs
 * once per set of game IDs; a plan that adds a game the last search did not
 * see starts a new one (the cache answers for the rest). Stopping it keeps
 * the games, without covers.
 *
 * The state says which search should run; an effect keeps one job running
 * for it and cancels the job once the search is stopped, forgotten or left.
 */
export function useCoverSearch(root: string | null, plan: PlanView | null, enabled: boolean) {
  const { transport } = useApi();
  const { settings } = useSettings();
  const { cancel } = useJobs();
  const [store, setStore] = useState<Store>({ started: 0, search: null, skip: null });
  const state = store.search;

  // Covers switched off forget the search; switched on again, the plan gets a new one.
  const [wasEnabled, setWasEnabled] = useState(enabled);
  if (enabled !== wasEnabled) {
    setWasEnabled(enabled);
    if (!enabled) setStore((s) => ({ ...s, search: null }));
  }
  // Started while rendering, so no frame shows the plan as ready to confirm
  // before its search.
  if (enabled && root && plan && plan.id !== store.skip && needsSearch(state, plan)) {
    setStore((s) => begin(s, root, plan));
  }

  const running = state?.status === "running" ? state : null;
  useEffect(() => {
    if (!running) return;
    const { search, root, plan } = running;
    const update = (patch: Partial<CoverSearchState>) =>
      setStore((s) => (s.search?.search === search ? { ...s, search: { ...s.search, ...patch } } : s));
    const poll = new AbortController();
    let job: JobId | null = null;
    void (async () => {
      try {
        const id = await fetchArt(transport, {
          type: "FetchArt",
          root,
          profile: settings.profile,
          sources: requestSources(settings),
          import: plan,
        });
        if (poll.signal.aborted) {
          void cancel(id).catch(() => undefined);
          return;
        }
        job = id;
        update({ job: id });
        const info = await waitForJob(transport, id, poll.signal);
        job = null;
        if (poll.signal.aborted) return;
        if (info.status === "finished" && info.result?.type === "Covers") {
          const { type: _type, ...result } = info.result;
          update({ status: "done", result });
        } else if (info.status === "cancelled") {
          update({ status: "stopped" });
        } else {
          update({ status: "failed", error: info.error ? errorText(info.error) : null });
        }
      } catch (error) {
        if (!poll.signal.aborted) update({ status: "failed", error: errorText(error) });
      }
    })();
    // Stopped, switched off, discarded or the page left: nobody waits for the job.
    return () => {
      poll.abort();
      if (job !== null) void cancel(job).catch(() => undefined);
    };
    // One job per search; the settings are read as they are when it starts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [running?.search]);

  const stop = useCallback(() => {
    setStore((s) => (s.search?.status === "running" ? { ...s, search: { ...s.search, status: "stopped" } } : s));
  }, []);

  /**
   * Forgets the search, stopping it if it runs: the next plan starts a new
   * one. `replaced` names a plan on screen that is about to be re-planned,
   * which the new search must not start from (it may have expired too).
   */
  const reset = useCallback(
    (replaced?: string) => setStore((s) => ({ ...s, search: null, skip: replaced ?? null })),
    [],
  );

  /** Looks again for the plan on screen, after a failed or stopped search. */
  const retry = useCallback(() => {
    if (plan && root) setStore((s) => begin(s, root, plan));
  }, [plan, root]);

  return { state: enabled ? state : null, stop, reset, retry };
}
