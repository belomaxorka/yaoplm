import { createContext, use } from "react";

import type { JobId } from "@/api/generated/JobId";
import type { JobInfo } from "@/api/generated/JobInfo";

export interface JobsContextValue {
  /** Every job the core knows about, newest first. */
  jobs: JobInfo[];
  /** Queued or running. */
  active: JobInfo[];
  /** Finished jobs the user cleared from the panel. */
  dismissed: ReadonlySet<JobId>;
  cancel: (job: JobId) => Promise<void>;
  dismiss: (job: JobId) => void;
  clearFinished: () => void;
  /** Fetches a job with its result and log. */
  load: (job: JobId) => Promise<JobInfo | undefined>;
  /** Plans (by id) that were sent to `ExecutePlan`; a job's plan result is stale once it ran. */
  executedPlans: ReadonlySet<string>;
  markExecuted: (plan: string) => void;
}

export const JobsContext = createContext<JobsContextValue | null>(null);

/**
 * Jobs the user started and may want to follow. Library scans are the app's
 * own business: their progress shows in the library (`ScanProgress`); a
 * picture slot's search shows inside its window (`ArtChooseDialog`).
 */
export function isUserJob(job: JobInfo): boolean {
  return job.kind !== "scanLibrary" && job.kind !== "findArt";
}

export function useJobs(): JobsContextValue {
  const value = use(JobsContext);
  if (!value) throw new Error("useJobs must be used inside <JobsProvider>");
  return value;
}
