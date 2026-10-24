import { ImageDown, ImageOff, RotateCcw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import type { CoverSearchState } from "@/features/add/useCoverSearch";
import { JobLog, JobProgressLine } from "@/features/jobs/JobsPage";
import { useJobs } from "@/jobs/context";
import { useSimpleMode } from "@/settings/context";

/**
 * The cover search under an import plan (ART-8): progress and the per-game
 * log while it runs, what it found once done. The plan is confirmed only
 * after it, or without covers once the user stops it.
 */
export function CoverSearchPanel({
  search,
  onStop,
  onRetry,
}: {
  search: CoverSearchState;
  onStop: () => void;
  onRetry: () => void;
}) {
  const { t } = useTranslation();
  const { jobs } = useJobs();
  const simple = useSimpleMode();
  const job = search.job === null ? undefined : jobs.find((j) => j.id === search.job);
  return (
    <div className="flex flex-col gap-2 rounded-md border bg-muted/40 p-3 text-sm" data-testid="cover-search" data-status={search.status}>
      <div className="flex flex-wrap items-center gap-2">
        <ImageDown className="size-4 text-muted-foreground" aria-hidden />
        <span className="min-w-0 flex-1 font-medium">{t("add.coversTitle")}</span>
        {search.status === "running" && (
          <Button size="sm" variant="outline" onClick={onStop} data-testid="cover-search-stop">
            <ImageOff aria-hidden />
            {t("add.coversSkip")}
          </Button>
        )}
        {(search.status === "stopped" || search.status === "failed") && (
          <Button size="sm" variant="outline" onClick={onRetry} data-testid="cover-search-retry">
            <RotateCcw aria-hidden />
            {t("add.coversRetry")}
          </Button>
        )}
      </div>
      {search.status === "running" && (
        <>
          <p className="text-muted-foreground" role="status">
            {t("add.coversSearching")}
          </p>
          {job && <JobProgressLine job={job} />}
        </>
      )}
      {search.status === "done" && search.result && (
        <p data-testid="cover-search-result">
          {t("add.coversResult", {
            found: search.result.found,
            missing: search.result.missing,
            present: search.result.present,
          })}
        </p>
      )}
      {search.status === "stopped" && <p className="text-muted-foreground">{t("add.coversSkipped")}</p>}
      {search.status === "failed" && (
        <p className="text-destructive" role="alert">
          {search.error ? t("add.coversFailed", { error: search.error }) : t("add.coversFailedPlain")}
        </p>
      )}
      {!simple && job && job.log.length > 0 && (
        <JobLog entries={job.log} height="max-h-48" />
      )}
    </div>
  );
}
