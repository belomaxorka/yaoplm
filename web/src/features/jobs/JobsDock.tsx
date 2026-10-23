import { Ban, ChevronRight, ListChecks } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link, useLocation } from "react-router";

import { Button } from "@/components/ui/button";
import { JobProgressLine, JobStatusIcon } from "@/features/jobs/JobsPage";
import { jobTitle } from "@/features/jobs/jobTitle";
import { isUserJob, useJobs } from "@/jobs/context";
import { useSimpleMode } from "@/settings/context";

/**
 * Active jobs at a glance from any screen, each with Cancel: in simple mode
 * this is the only place a write can be stopped. The Jobs page has the rest.
 */
export function JobsDock() {
  const { t } = useTranslation();
  const { cancel } = useJobs();
  const active = useJobs().active.filter(isUserJob);
  const { pathname } = useLocation();
  const simple = useSimpleMode();
  if (active.length === 0 || pathname === "/jobs") return null;
  const shown = active.slice(0, 3);
  return (
    <aside
      className="fixed right-4 bottom-4 z-40 w-80 max-w-[calc(100vw-2rem)] rounded-lg border bg-popover p-3 text-popover-foreground shadow-lg"
      aria-label={t("jobs.dockLabel")}
      data-testid="jobs-dock"
    >
      <div className="mb-2 flex items-center gap-2 text-sm font-medium">
        <ListChecks className="size-4" aria-hidden />
        {t("jobs.active", { count: active.length })}
        {!simple && (
          <Button asChild variant="ghost" size="sm" className="ml-auto">
            <Link to="/jobs">
              {t("jobs.all")}
              <ChevronRight aria-hidden />
            </Link>
          </Button>
        )}
      </div>
      <ul className="flex flex-col gap-3">
        {shown.map((job) => (
          <li key={job.id} className="flex flex-col gap-1">
            <div className="flex items-center gap-2 text-sm">
              <JobStatusIcon job={job} />
              <span className="min-w-0 flex-1 truncate" title={jobTitle(job)}>
                {jobTitle(job)}
              </span>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => void cancel(job.id)}
                aria-label={t("jobs.cancelJob", { title: jobTitle(job) })}
                title={t("jobs.cancel")}
                data-testid="dock-cancel"
              >
                <Ban aria-hidden />
              </Button>
            </div>
            <JobProgressLine job={job} />
          </li>
        ))}
      </ul>
    </aside>
  );
}
