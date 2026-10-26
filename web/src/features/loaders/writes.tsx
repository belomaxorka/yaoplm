import { Ban } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { JobId } from "@/api/generated/JobId";
import { Button } from "@/components/ui/button";
import { JobProgressLine } from "@/features/jobs/JobsPage";
import { useJobs } from "@/jobs/context";

/**
 * The write a loader dialog waits for, with its own Cancel: the dialog covers
 * the jobs dock, and in simple mode there is no jobs page (UI-4).
 */
export function WaitedWrite({ job }: { job: JobId }) {
  const { jobs } = useJobs();
  const info = jobs.find((j) => j.id === job);
  return info ? <JobProgressLine job={info} /> : null;
}

export function CancelWrite({ job }: { job: JobId }) {
  const { t } = useTranslation();
  const { cancel } = useJobs();
  return (
    <Button variant="outline" onClick={() => void cancel(job)} data-testid="loader-install-cancel-write">
      <Ban aria-hidden />
      {t("jobs.cancel")}
    </Button>
  );
}
