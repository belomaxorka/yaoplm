import { ChevronRight, ListChecks } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link, useLocation } from "react-router";

import { Button } from "@/components/ui/button";
import { JobProgressLine, JobStatusIcon } from "@/features/jobs/JobsPage";
import { useJobs } from "@/jobs/context";

/** Active jobs at a glance from any screen; the Jobs page has the rest. */
export function JobsDock() {
  const { t } = useTranslation();
  const { active } = useJobs();
  const { pathname } = useLocation();
  if (active.length === 0 || pathname === "/jobs") return null;
  const shown = active.slice(0, 3);
  return (
    <aside
      className="fixed right-4 bottom-4 z-40 w-80 rounded-lg border bg-popover p-3 text-popover-foreground shadow-lg"
      aria-label={t("jobs.dockLabel")}
      data-testid="jobs-dock"
    >
      <div className="mb-2 flex items-center gap-2 text-sm font-medium">
        <ListChecks className="size-4" aria-hidden />
        {t("jobs.active", { count: active.length })}
        <Button asChild variant="ghost" size="sm" className="ml-auto">
          <Link to="/jobs">
            {t("jobs.all")}
            <ChevronRight aria-hidden />
          </Link>
        </Button>
      </div>
      <ul className="flex flex-col gap-3">
        {shown.map((job) => (
          <li key={job.id} className="flex flex-col gap-1">
            <div className="flex items-center gap-2 text-sm">
              <JobStatusIcon job={job} />
              <span className="truncate" title={job.title}>
                {job.kind === "scanLibrary" ? t("jobs.scanTitle") : job.title}
              </span>
            </div>
            <JobProgressLine job={job} />
          </li>
        ))}
      </ul>
    </aside>
  );
}
