import { useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { useApi } from "@/api/context";
import type { JobEvent } from "@/api/generated/JobEvent";
import type { JobId } from "@/api/generated/JobId";
import type { JobInfo } from "@/api/generated/JobInfo";
import { cancelJob, getJob, listJobs } from "@/api/rpc";
import { JobsContext } from "@/jobs/context";

type JobMap = ReadonlyMap<JobId, JobInfo>;

function isActive(job: JobInfo): boolean {
  return job.status === "queued" || job.status === "running";
}

/**
 * Mirrors the core's job list from events and `ListJobs`, and rescans the
 * open library once after a run of write jobs ends: one rescan per queue,
 * not one per game (the OrbitPS2 Manager idea, research/10).
 */
export function JobsProvider({ children }: { children: ReactNode }) {
  const { transport, status, addEventListener } = useApi();
  const queryClient = useQueryClient();
  const [jobs, setJobs] = useState<JobMap>(new Map());
  const [dismissed, setDismissed] = useState<ReadonlySet<JobId>>(new Set());
  const [executedPlans, setExecutedPlans] = useState<ReadonlySet<string>>(new Set());
  // Event handlers and callbacks read the newest map without re-subscribing.
  const latest = useRef<JobMap>(jobs);
  useEffect(() => {
    latest.current = jobs;
  }, [jobs]);

  const patch = useCallback((id: JobId, change: (job: JobInfo) => JobInfo) => {
    setJobs((current) => {
      const job = current.get(id);
      if (!job) return current;
      const next = new Map(current);
      next.set(id, change(job));
      return next;
    });
  }, []);

  // Events missed while disconnected are caught up from ListJobs on every (re)connect.
  useEffect(() => {
    if (status !== "open") return;
    let cancelled = false;
    listJobs(transport).then(
      (list) => {
        if (cancelled) return;
        setJobs((current) => {
          const next = new Map<JobId, JobInfo>();
          for (const job of list) {
            // ListJobs leaves results and logs out; keep what events gave us.
            const known = current.get(job.id);
            next.set(job.id, known ? { ...job, result: known.result, log: known.log } : job);
          }
          return next;
        });
      },
      () => {
        // The connection badge already says the core is unreachable.
      },
    );
    return () => {
      cancelled = true;
    };
  }, [status, transport]);

  const load = useCallback(
    async (id: JobId) => {
      try {
        const info = await getJob(transport, id);
        setJobs((current) => new Map(current).set(id, info));
        return info;
      } catch {
        return undefined;
      }
    },
    [transport],
  );

  // The library changes when a write job ends; wait for the queue to drain.
  // Dump verdicts come from the hash cache and the Redump DAT, so a finished
  // check or DAT update shows up after a scan too (GAME-5).
  const rescanIfIdle = useCallback(
    (finished: JobInfo) => {
      const verdicts =
        finished.status === "finished" && (finished.kind === "verifyGames" || finished.kind === "updateRedump");
      if (finished.kind !== "executePlan" && !verdicts) return;
      const stillWriting = [...latest.current.values()].some(
        (job) => job.id !== finished.id && job.kind === "executePlan" && isActive(job),
      );
      if (stillWriting) return;
      void queryClient.invalidateQueries({ queryKey: ["library"] });
      if (finished.kind === "executePlan") {
        void queryClient.invalidateQueries({ queryKey: ["device"] });
        // A game's settings and the memory cards (CFG-2, VMC-1).
        void queryClient.invalidateQueries({ queryKey: ["gameCfg"] });
        void queryClient.invalidateQueries({ queryKey: ["vmcs"] });
      }
    },
    [queryClient],
  );

  useEffect(
    () =>
      addEventListener((event: JobEvent) => {
        switch (event.type) {
          case "Queued":
            setJobs((current) =>
              new Map(current).set(event.job, {
                id: event.job,
                kind: event.kind,
                title: event.title,
                titlePhrase: event.titlePhrase,
                root: event.root,
                status: "queued",
                done: 0,
                total: null,
                rate: null,
                item: null,
                error: null,
                log: [],
              }),
            );
            break;
          case "Started":
            patch(event.job, (job) => ({ ...job, status: "running" }));
            break;
          case "Progress":
            patch(event.job, (job) => ({
              ...job,
              done: event.done,
              total: event.total,
              rate: event.rate,
              item: event.item ?? job.item,
              itemPhrase: event.item ? event.itemPhrase : job.itemPhrase,
            }));
            break;
          case "Log":
            patch(event.job, (job) => ({ ...job, log: [...job.log, event.entry] }));
            break;
          case "Finished":
          case "Failed":
          case "Cancelled": {
            const finished = latest.current.get(event.job);
            // The final state carries the result; fetch it once.
            void load(event.job).then((info) => {
              if (info) rescanIfIdle(info);
              else if (finished) rescanIfIdle(finished);
            });
            break;
          }
        }
      }),
    [addEventListener, patch, load, rescanIfIdle],
  );

  const cancel = useCallback((id: JobId) => cancelJob(transport, id), [transport]);
  const markExecuted = useCallback((plan: string) => setExecutedPlans((current) => new Set(current).add(plan)), []);
  const dismiss = useCallback((id: JobId) => setDismissed((current) => new Set(current).add(id)), []);
  const clearFinished = useCallback(() => {
    setDismissed((current) => {
      const next = new Set(current);
      for (const job of latest.current.values()) if (!isActive(job)) next.add(job.id);
      return next;
    });
  }, []);

  const value = useMemo(() => {
    const list = [...jobs.values()].sort((a, b) => b.id - a.id);
    return {
      jobs: list,
      active: list.filter(isActive),
      dismissed,
      cancel,
      dismiss,
      clearFinished,
      load,
      executedPlans,
      markExecuted,
    };
  }, [jobs, dismissed, cancel, dismiss, clearFinished, load, executedPlans, markExecuted]);

  return <JobsContext value={value}>{children}</JobsContext>;
}
