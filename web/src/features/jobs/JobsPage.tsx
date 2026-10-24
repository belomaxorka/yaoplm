import { cn } from "cn";
import {
  Ban,
  BrushCleaning,
  CheckCircle2,
  ChevronDown,
  CircleX,
  Clock,
  Database,
  Download,
  FileInput,
  FileSearch,
  FolderInput,
  History,
  ImageDown,
  ListChecks,
  Loader2,
  RefreshCw,
  ScanSearch,
  ShieldCheck,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router";

import type { ArtType } from "@/api/generated/ArtType";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { JobKind } from "@/api/generated/JobKind";
import type { JobLogEntry } from "@/api/generated/JobLogEntry";
import type { TxOutcome } from "@/api/generated/TxOutcome";
import { EmptyState } from "@/components/EmptyState";
import { PageScroll } from "@/components/PageScroll";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { jobTitle } from "@/features/jobs/jobTitle";
import { usePlan } from "@/features/plan/usePlan";
import { artTypes } from "@/features/art/kinds";
import { statusColours as dumpColours, statusCounts } from "@/features/verify/dump";
import { DumpIcon } from "@/features/verify/DumpBadge";
import { VerificationResults } from "@/features/verify/VerificationResults";
import { isUserJob, useJobs } from "@/jobs/context";
import { useSimpleMode } from "@/settings/context";
import { formatBytes, formatDate, formatEta, formatRate, formatTime, percent } from "@/lib/format";
import { errorText, phraseText } from "@/lib/phrase";

/**
 * Jobs panel (IMP-9): one card per job with progress, speed, time left,
 * the game being written, cancel, per-transaction outcomes and the live
 * step log (ideas from OrbitPS2 Manager's jobs panel, research/10).
 */
export function JobsPage() {
  const { t } = useTranslation();
  const { jobs, dismissed, clearFinished } = useJobs();
  const shown = jobs.filter((job) => isUserJob(job) && !dismissed.has(job.id));
  const finished = shown.filter((job) => job.status !== "queued" && job.status !== "running");

  return (
    <PageScroll>
      <div className="mx-auto flex max-w-4xl flex-col gap-4">
        {shown.length > 0 && (
          <div className="flex justify-end">
            <Button variant="outline" size="sm" onClick={clearFinished} disabled={finished.length === 0}>
              <BrushCleaning aria-hidden />
              {t("jobs.clearFinished")}
            </Button>
          </div>
        )}
        {shown.length === 0 && (
          <EmptyState icon={ListChecks} title={t("jobs.emptyTitle")} description={t("jobs.emptyDescription")} />
        )}
        <ul className="flex flex-col gap-3">
          {shown.map((job) => (
            <JobCard key={job.id} job={job} />
          ))}
        </ul>
      </div>
    </PageScroll>
  );
}

const statusIcons = {
  queued: Clock,
  running: Loader2,
  finished: CheckCircle2,
  failed: CircleX,
  cancelled: Ban,
} as const;

const statusColours = {
  queued: "text-muted-foreground",
  running: "text-primary",
  finished: "text-emerald-700 dark:text-emerald-400",
  failed: "text-destructive",
  cancelled: "text-amber-700 dark:text-amber-400",
} as const;

export function JobStatusIcon({ job, className }: { job: JobInfo; className?: string }) {
  const Icon = statusIcons[job.status];
  return (
    <Icon
      className={cn("size-4 shrink-0", statusColours[job.status], job.status === "running" && "animate-spin", className)}
      aria-hidden
    />
  );
}

export function JobProgressLine({ job }: { job: JobInfo }) {
  const { t } = useTranslation();
  const value = percent(job.done, job.total);
  const bytes = job.kind === "executePlan" || job.kind === "verifyGames";
  // Speed, time left and the item being worked on mean nothing once the
  // job has ended: the last item would read as still in progress.
  const active = job.status === "queued" || job.status === "running";
  const eta = active ? formatEta(job.done, job.total, job.rate) : null;
  return (
    <div className="flex flex-col gap-1">
      <Progress
        value={value ?? 0}
        className={cn("h-1.5", value === null && job.status === "running" && "animate-pulse")}
        aria-label={jobTitle(job)}
      />
      <div className="flex flex-wrap gap-x-3 text-xs text-muted-foreground tabular-nums">
        <span className="font-mono">
          {bytes ? formatBytes(job.done) : job.done}
          {job.total !== null && ` / ${bytes ? formatBytes(job.total) : job.total}`}
          {value !== null && ` (${value}%)`}
        </span>
        {active && job.rate !== null && <span>{formatRate(job.rate)}</span>}
        {eta && <span>{t("jobs.eta", { time: eta })}</span>}
        {active && job.item && (
          <span className="truncate" title={phraseText(job.itemPhrase, job.item)}>
            {phraseText(job.itemPhrase, job.item)}
          </span>
        )}
      </div>
    </div>
  );
}

function outcomeCounts(outcomes: readonly TxOutcome[]): Partial<Record<TxOutcome, number>> {
  const counts: Partial<Record<TxOutcome, number>> = {};
  for (const outcome of outcomes) counts[outcome] = (counts[outcome] ?? 0) + 1;
  return counts;
}

const outcomeVariant: Record<TxOutcome, "default" | "secondary" | "destructive" | "outline"> = {
  done: "default",
  rolledBack: "secondary",
  notStarted: "outline",
  interrupted: "destructive",
};

function JobCard({ job }: { job: JobInfo }) {
  const { t } = useTranslation();
  const { cancel, dismiss, load, executedPlans } = useJobs();
  const [logOpen, setLogOpen] = useState(false);
  const [checksOpen, setChecksOpen] = useState(false);
  const active = job.status === "queued" || job.status === "running";
  const report = job.result?.type === "Execution" ? job.result : null;
  const verification = job.result?.type === "Verification" ? job.result : null;
  const redump = job.result?.type === "Redump" ? job.result : null;
  const covers = job.result?.type === "Covers" ? job.result : null;
  const pendingPlan = job.result?.type === "Plan" && !executedPlans.has(job.result.id) ? job.result : null;
  const KindIcon = kindIcons[job.kind];
  // Plans, cover searches and dump checks keep a log; the rest only report progress.
  const plan = !noLog.has(job.kind);
  const flow = usePlan();

  // ListJobs leaves results and logs out; a finished plan fetches its own once.
  useEffect(() => {
    if (plan && !active && !job.result) void load(job.id);
  }, [plan, active, job.result, job.id, load]);

  return (
    <li className="rounded-lg border bg-card p-4" data-testid="job-card" data-status={job.status} data-kind={job.kind}>
      <div className="flex items-start gap-3">
        <KindIcon className="mt-0.5 size-5 shrink-0 text-muted-foreground" aria-hidden />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium wrap-anywhere">{jobTitle(job)}</span>
            <Badge variant="outline" className={cn("gap-1", statusColours[job.status])}>
              <JobStatusIcon job={job} className="size-3" />
              {t(`jobs.status.${job.status}`)}
            </Badge>
            <span className="font-mono text-xs text-muted-foreground">#{job.id}</span>
          </div>
          {job.root && (
            <div className="truncate font-mono text-xs text-muted-foreground" title={job.root}>
              {job.root}
            </div>
          )}
          {(active || (plan && job.total !== null && job.total > 0)) && (
            <div className="mt-2">
              <JobProgressLine job={job} />
            </div>
          )}
          {job.error && (
            <p className="mt-2 rounded-md border border-destructive/40 bg-destructive/10 p-2 text-sm text-destructive" role="alert">
              {errorText(job.error)}
            </p>
          )}
          {pendingPlan && (
            <div className="mt-2 flex flex-wrap items-center gap-2 text-sm" data-testid="job-plan">
              <span>
                {pendingPlan.executable
                  ? t("jobs.planReady", { title: phraseText(pendingPlan.titlePhrase, pendingPlan.title) })
                  : t("jobs.planEmpty", { title: phraseText(pendingPlan.titlePhrase, pendingPlan.title) })}
              </span>
              <Button
                size="sm"
                variant={pendingPlan.executable ? "default" : "outline"}
                onClick={() => {
                  const { type: _type, ...view } = pendingPlan;
                  flow.adopt(view);
                }}
                data-testid="job-review-plan"
              >
                {t("jobs.reviewPlan")}
              </Button>
              <PlanDialog flow={flow} />
            </div>
          )}
          {verification && (
            <ul className="mt-2 flex flex-wrap gap-1.5" data-testid="job-verification">
              {statusCounts(verification.games).map(([status, count]) => (
                <li key={status}>
                  <Badge variant="outline" className={dumpColours[status]}>
                    <DumpIcon status={status} className="size-3" />
                    {t(`dump.count.${status}`, { count })}
                  </Badge>
                </li>
              ))}
            </ul>
          )}
          {covers && (
            <p className="mt-2 text-sm" data-testid="job-covers">
              {t("add.coversResult", { found: covers.found, missing: covers.missing, present: covers.present })}
            </p>
          )}
          {redump && (
            <p className="mt-2 text-sm" data-testid="job-redump">
              {t(`dump.update.${redump.update}`, {
                games: redump.dat?.games ?? 0,
                date: redump.nextCheck === null ? "" : formatDate(redump.nextCheck),
              })}
            </p>
          )}
          {report && (
            <div className="mt-2 flex flex-col gap-2 text-sm" data-testid="job-outcomes">
              <ul className="flex flex-wrap gap-1.5">
                {Object.entries(outcomeCounts(report.outcomes)).map(([outcome, count]) => (
                  <li key={outcome}>
                    <Badge variant={outcomeVariant[outcome as TxOutcome]}>
                      {t(`jobs.outcome.${outcome as TxOutcome}`, { count })}
                    </Badge>
                  </li>
                ))}
              </ul>
              {report.error && <p className="text-destructive">{phraseText(report.errorPhrase, report.error)}</p>}
              {report.cancelled && <p className="text-muted-foreground">{t("jobs.cancelledNote")}</p>}
              {report.journal && (
                <p className="flex flex-wrap items-center gap-2 text-destructive">
                  <History className="size-4" aria-hidden />
                  {t("jobs.journalLeft", { id: report.journal })}
                  <Button asChild size="sm" variant="outline">
                    <Link to="/library">{t("jobs.recover")}</Link>
                  </Button>
                </p>
              )}
              {report.checks.length > 0 && (
                <div data-testid="job-checks">
                  <Button variant="ghost" size="sm" aria-expanded={checksOpen} onClick={() => setChecksOpen(!checksOpen)}>
                    <ChevronDown className={cn("transition-transform", checksOpen && "rotate-180")} aria-hidden />
                    {t("jobs.checks", { count: report.checks.length })}
                  </Button>
                  <ul className="mt-1 flex flex-wrap gap-1.5">
                    {statusCounts(report.checks).map(([status, count]) => (
                      <li key={status}>
                        <Badge variant="outline" className={dumpColours[status]}>
                          <DumpIcon status={status} className="size-3" />
                          {t(`dump.count.${status}`, { count })}
                        </Badge>
                      </li>
                    ))}
                  </ul>
                  {checksOpen && (
                    <div className="mt-2 flex max-h-96 flex-col">
                      <VerificationResults report={{ games: report.checks, dat: null }} summary={false} />
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
          {plan && (
            <div className="mt-2">
              <Button
                variant="ghost"
                size="sm"
                aria-expanded={logOpen}
                onClick={() => {
                  if (!logOpen && job.log.length === 0) void load(job.id);
                  setLogOpen(!logOpen);
                }}
              >
                <ChevronDown className={cn("transition-transform", logOpen && "rotate-180")} aria-hidden />
                {t("jobs.log", { count: job.log.length })}
              </Button>
              {logOpen && <JobLog entries={job.log} collapsible={false} />}
            </div>
          )}
        </div>
        <div className="flex shrink-0 gap-1">
          {active ? (
            <Button variant="outline" size="sm" onClick={() => void cancel(job.id)} data-testid="job-cancel">
              <Ban aria-hidden />
              {t("jobs.cancel")}
            </Button>
          ) : (
            <Button variant="ghost" size="icon-sm" onClick={() => dismiss(job.id)} aria-label={t("jobs.dismiss")}>
              <X aria-hidden />
            </Button>
          )}
        </div>
      </div>
    </li>
  );
}

const kindIcons = {
  scanLibrary: ScanSearch,
  executePlan: ListChecks,
  fetchArt: ImageDown,
  verifyGames: ShieldCheck,
  updateRedump: Database,
  importArt: FolderInput,
  updateLoaderCatalog: RefreshCw,
  loaderRelease: FileSearch,
  downloadLoader: Download,
  importLoader: FileInput,
} as const;

/** Jobs that only report progress: no log to fetch once they end. */
const noLog: ReadonlySet<JobKind> = new Set<JobKind>([
  "scanLibrary",
  "updateRedump",
  "updateLoaderCatalog",
  "loaderRelease",
  "downloadLoader",
  "importLoader",
]);

const stepStateColours = {
  started: "text-muted-foreground",
  done: "text-emerald-700 dark:text-emerald-400",
  failed: "text-destructive",
  undone: "text-amber-700 dark:text-amber-400",
} as const;

const artStateColours = {
  found: "text-emerald-700 dark:text-emerald-400",
  cached: "text-emerald-700 dark:text-emerald-400",
  notFound: "text-muted-foreground",
  failed: "text-destructive",
} as const;

/**
 * A job's log: folded away by default (`collapsible`, unless the caller has
 * its own toggle) and not shown at all in simple mode, which keeps to the
 * result. It scrolls by itself: wrap it without `overflow`, or the page
 * shows two scrollbars. `height` caps it (`max-h-72` by default).
 */
export function JobLog({
  entries,
  height,
  collapsible = true,
}: {
  entries: readonly JobLogEntry[];
  height?: string;
  collapsible?: boolean;
}) {
  const { t } = useTranslation();
  const simple = useSimpleMode();
  if (simple) return null;
  if (entries.length === 0) return <p className="mt-1 text-xs text-muted-foreground">{t("jobs.logEmpty")}</p>;
  if (collapsible) {
    return (
      <details className="text-xs" data-testid="job-log-details">
        <summary className="cursor-pointer text-muted-foreground select-none hover:text-foreground">
          {t("jobs.log", { count: entries.length })}
        </summary>
        <JobLogList entries={entries} height={height} />
      </details>
    );
  }
  return <JobLogList entries={entries} height={height} />;
}

function JobLogList({ entries, height }: { entries: readonly JobLogEntry[]; height?: string }) {
  const { t } = useTranslation();
  return (
    <ol
      className={cn("relative mt-1 min-h-0 overflow-auto rounded-md border bg-background p-2 font-mono text-xs", height ?? "max-h-72")}
      data-testid="job-log"
    >
      {entries.map((entry, index) => (
        <li
          key={index}
          className={cn(
            "flex gap-2 py-0.5",
            entry.kind === "transaction" ? "font-semibold" : "pl-3",
            entry.kind === "step" && stepStateColours[entry.state],
            entry.kind === "art" && artStateColours[entry.state],
            entry.kind === "verify" && dumpColours[entry.status],
            entry.kind === "transaction" && entry.state === "rolledBack" && "text-amber-700 dark:text-amber-400",
            entry.kind === "transaction" && entry.state === "interrupted" && "text-destructive",
          )}
          data-art-state={entry.kind === "art" ? entry.state : undefined}
          data-verify-status={entry.kind === "verify" ? entry.status : undefined}
        >
          <span className="shrink-0 text-muted-foreground tabular-nums">{formatTime(entry.at)}</span>
          <span className="min-w-0 wrap-anywhere">
            {entry.kind === "transaction" &&
              t(`jobs.txState.${entry.state}`, { title: phraseText(entry.titlePhrase, entry.title) })}
            {entry.kind === "step" &&
              t(`jobs.stepState.${entry.state}`, {
                step: t(`plan.steps.${entry.op}`, { path: entry.path, to: entry.to ?? "" }),
              })}
            {entry.kind === "art" &&
              t(`jobs.artState.${entry.state}`, {
                startup: entry.startup,
                title: entry.title,
                source: entry.source ?? "",
              })}
            {/* Front covers are the usual case; any other type is named. */}
            {entry.kind === "art" && entry.art.toUpperCase() !== "COV" && (
              <span className="ml-1.5 rounded border px-1 text-[0.65rem] text-muted-foreground">
                {isArtType(entry.art) ? t(`art.kinds.${entry.art}`) : entry.art}
              </span>
            )}
            {entry.kind === "verify" &&
              t(`jobs.verifyState.${entry.status}`, {
                startup: entry.startup,
                title: entry.title,
                redump: entry.redump ?? "",
              })}
            {entry.kind === "verify" && entry.reason && (
              <span className="block text-muted-foreground">{t(`dump.reason.${entry.reason}`)}</span>
            )}
            {entry.kind === "verify" && entry.cached && (
              <span className="block text-muted-foreground">{t("dump.fromCache")}</span>
            )}
            {entry.kind === "verify" && entry.error && (
              <span className="block">{phraseText(entry.errorPhrase, entry.error)}</span>
            )}
            {entry.kind === "step" && entry.error && (
              <span className="block">{phraseText(entry.errorPhrase, entry.error)}</span>
            )}
            {entry.kind === "art" && entry.detail && (
              <span className="block text-muted-foreground">{phraseText(entry.detailPhrase, entry.detail)}</span>
            )}
          </span>
        </li>
      ))}
    </ol>
  );
}

function isArtType(value: string): value is ArtType {
  return (artTypes as readonly string[]).includes(value);
}
