import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useLocation } from "react-router";
import { toast } from "sonner";

import type { JobId } from "@/api/generated/JobId";
import type { JobInfo } from "@/api/generated/JobInfo";
import { Toaster } from "@/components/ui/sonner";
import { jobTitle } from "@/features/jobs/jobTitle";
import { useJobs } from "@/jobs/context";
import { errorText, phraseText } from "@/lib/phrase";

function isActive(job: JobInfo): boolean {
  return job.status === "queued" || job.status === "running";
}

/**
 * How a write ended, as a notification (UI-4): simple mode has no jobs
 * page, and the dock disappears with the last active job. Only writes seen
 * running in this session notify; a failure stays until it is closed.
 */
export function JobToasts() {
  const { t } = useTranslation();
  const { jobs, load } = useJobs();
  const { pathname } = useLocation();
  const running = useRef(new Set<JobId>());
  // Jobs whose final state came without its result (ListJobs after a lost link).
  const fetching = useRef(new Set<JobId>());

  useEffect(() => {
    for (const job of jobs) {
      if (job.kind !== "executePlan") continue;
      if (isActive(job)) {
        running.current.add(job.id);
        continue;
      }
      if (!running.current.has(job.id)) continue;
      // A failed transaction still ends the job as finished, with the error in
      // its report: no toast before the report is here.
      if (job.status === "finished" && !job.result) {
        if (!fetching.current.has(job.id)) {
          fetching.current.add(job.id);
          void load(job.id).finally(() => fetching.current.delete(job.id));
        }
        continue;
      }
      running.current.delete(job.id);
      // The jobs page shows the outcome itself.
      if (pathname === "/jobs") continue;
      const title = jobTitle(job);
      const report = job.result?.type === "Execution" ? job.result : null;
      const problem = job.error
        ? errorText(job.error)
        : report?.error
          ? phraseText(report.errorPhrase, report.error)
          : report?.journal
            ? t("jobs.toast.journal")
            : null;
      if (job.status === "cancelled" || report?.cancelled) {
        toast(t("jobs.toast.cancelled", { title }), { description: t("jobs.cancelledNote") });
      } else if (job.status === "failed" || problem) {
        toast.error(t("jobs.toast.failed", { title }), { description: problem ?? undefined, duration: Infinity, closeButton: true });
      } else {
        toast.success(t("jobs.toast.done", { title }));
      }
    }
  }, [jobs, pathname, t, load]);

  return (
    <Toaster
      position="bottom-center"
      containerAriaLabel={t("jobs.toast.region")}
      toastOptions={{ closeButtonAriaLabel: t("common.close") }}
    />
  );
}
