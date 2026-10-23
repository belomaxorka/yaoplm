import { useQuery } from "@tanstack/react-query";
import { Ban, ExternalLink, Loader2, ShieldCheck } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import type { Game } from "@/api/generated/Game";
import type { GameVerification } from "@/api/generated/GameVerification";
import type { JobId } from "@/api/generated/JobId";
import type { VerificationReport } from "@/api/generated/VerificationReport";
import { redumpStatus, verifyGames, waitForJob } from "@/api/rpc";
import { RpcError } from "@/api/transport";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { JobLog, JobProgressLine } from "@/features/jobs/JobsPage";
import { statusColours, statusCounts, uncheckedGames } from "@/features/verify/dump";
import { DumpBadge, DumpIcon } from "@/features/verify/DumpBadge";
import { RedumpPanel } from "@/features/verify/RedumpPanel";
import { useJobs } from "@/jobs/context";
import { formatBytes } from "@/lib/format";
import { gameRef } from "@/lib/gameKey";
import { useSettings } from "@/settings/context";

type Scope = "unchecked" | "all";

/**
 * "Check dumps" (GAME-5, META-2): hash the images on the device and look
 * them up in the Redump DAT. Reads only; the device is never written.
 * Images whose hashes are cached and unchanged are not read again unless
 * the user asks for a fresh read.
 */
export function VerifyDialog({
  root,
  games,
  open,
  onOpenChange,
  only,
}: {
  root: string;
  games: readonly Game[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Check one game (the game page). */
  only?: Game;
}) {
  const { t } = useTranslation();
  const [running, setRunning] = useState(false);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && running) return;
        onOpenChange(next);
      }}
    >
      <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-2xl" data-testid="verify-dialog">
        <DialogHeader>
          <DialogTitle>{only ? t("dump.verifyOneTitle", { title: only.title }) : t("dump.verifyTitle")}</DialogTitle>
          <DialogDescription>{t("dump.verifyDescription")}</DialogDescription>
        </DialogHeader>
        {/* Mounted only while open, so every opening starts afresh. */}
        <VerifyBody root={root} games={games} only={only} onRunning={setRunning} onClose={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  );
}

function VerifyBody({
  root,
  games,
  only,
  onRunning,
  onClose,
}: {
  root: string;
  games: readonly Game[];
  only?: Game;
  onRunning: (running: boolean) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { settings } = useSettings();
  const { jobs, cancel } = useJobs();
  const ids = useId();
  const unchecked = uncheckedGames(only ? [only] : games);
  const [scope, setScope] = useState<Scope>(unchecked.length > 0 ? "unchecked" : "all");
  const [md5, setMd5] = useState(false);
  const [fresh, setFresh] = useState(false);
  const [jobId, setJobId] = useState<JobId | null>(null);
  const [report, setReport] = useState<VerificationReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const dat = useQuery({ queryKey: ["redump"], queryFn: () => redumpStatus(transport) });
  const job = jobId === null ? undefined : jobs.find((j) => j.id === jobId);
  const active = job !== undefined && (job.status === "queued" || job.status === "running");
  const chosen = only ? [only] : scope === "unchecked" ? unchecked : null;

  const start = async () => {
    setStarting(true);
    setError(null);
    try {
      const id = await verifyGames(transport, {
        type: "VerifyGames",
        root,
        profile: settings.profile,
        ...(chosen ? { games: chosen.map(gameRef) } : {}),
        ...(md5 ? { md5 } : {}),
        ...(fresh ? { fresh } : {}),
      });
      setJobId(id);
      onRunning(true);
      const info = await waitForJob(transport, id);
      if (info.status === "finished" && info.result?.type === "Verification") {
        const { type: _type, ...result } = info.result;
        setReport(result);
      }
    } catch (err) {
      setError(err instanceof RpcError ? err.message : String(err));
    } finally {
      setStarting(false);
      onRunning(false);
    }
  };

  const showForm = jobId === null;
  return (
    <>
      {showForm && (
        <div className="flex min-h-0 flex-col gap-4 overflow-auto">
          <RedumpPanel compact />
          {!only && (
            <fieldset>
              <legend className="mb-2 text-sm font-medium">{t("dump.scope")}</legend>
              <RadioGroup value={scope} onValueChange={(value) => setScope(value as Scope)}>
                <Label htmlFor={`${ids}-unchecked`} className="items-start font-normal">
                  <RadioGroupItem id={`${ids}-unchecked`} value="unchecked" className="mt-0.5" data-testid="verify-scope-unchecked" />
                  <span>
                    {t("dump.scopeUnchecked", { count: unchecked.length })}
                    <span className="block text-xs text-muted-foreground">{t("dump.scopeUncheckedHint")}</span>
                  </span>
                </Label>
                <Label htmlFor={`${ids}-all`} className="items-start font-normal">
                  <RadioGroupItem id={`${ids}-all`} value="all" className="mt-0.5" data-testid="verify-scope-all" />
                  <span>
                    {t("dump.scopeAll", { count: games.length })}
                    <span className="block text-xs text-muted-foreground">{t("dump.scopeAllHint")}</span>
                  </span>
                </Label>
              </RadioGroup>
            </fieldset>
          )}
          <div className="flex flex-col gap-3">
            <Label htmlFor={`${ids}-fresh`} className="items-start font-normal">
              <Checkbox id={`${ids}-fresh`} checked={fresh} onCheckedChange={(value) => setFresh(value === true)} className="mt-0.5" />
              <span>
                {t("dump.fresh")}
                <span className="block text-xs text-muted-foreground">{t("dump.freshHint")}</span>
              </span>
            </Label>
            <Label htmlFor={`${ids}-md5`} className="items-start font-normal">
              <Checkbox id={`${ids}-md5`} checked={md5} onCheckedChange={(value) => setMd5(value === true)} className="mt-0.5" />
              <span>
                {t("dump.md5")}
                <span className="block text-xs text-muted-foreground">{t("dump.md5Hint")}</span>
              </span>
            </Label>
          </div>
        </div>
      )}
      {error && (
        <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
          {error}
        </p>
      )}
      {job && !report && (
        <div className="flex min-h-0 flex-1 flex-col gap-3">
          <JobProgressLine job={job} />
          {job.error && (
            <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
              {job.error.message}
            </p>
          )}
          {job.status === "cancelled" && <p className="text-sm text-muted-foreground">{t("dump.cancelled")}</p>}
          <div className="min-h-0 flex-1 overflow-auto">
            <JobLog entries={job.log} />
          </div>
        </div>
      )}
      {report && <VerificationResults report={report} />}
      <DialogFooter>
        {showForm ? (
          <>
            <Button variant="outline" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={() => void start()}
              disabled={starting || !dat.data || (chosen !== null && chosen.length === 0)}
              data-testid="verify-start"
            >
              {starting ? <Loader2 className="animate-spin" aria-hidden /> : <ShieldCheck aria-hidden />}
              {t("dump.start")}
            </Button>
          </>
        ) : active ? (
          <Button variant="outline" onClick={() => void cancel(job.id)}>
            <Ban aria-hidden />
            {t("jobs.cancel")}
          </Button>
        ) : (
          <Button variant="outline" onClick={onClose} data-testid="verify-close">
            {t("common.close")}
          </Button>
        )}
      </DialogFooter>
    </>
  );
}

/** The outcome per game, problems first. */
export function VerificationResults({ report }: { report: VerificationReport }) {
  const { t } = useTranslation();
  const order = { failed: 0, badDump: 1, unknown: 2, verified: 3 } as const;
  const sorted = [...report.games].sort((a, b) => order[a.status] - order[b.status] || a.title.localeCompare(b.title));
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3" data-testid="verify-results">
      <ul className="flex flex-wrap gap-1.5" aria-label={t("dump.summary")}>
        {statusCounts(report.games).map(([status, count]) => (
          <li key={status}>
            <Badge variant="outline" className={statusColours[status]} data-testid={`verify-count-${status}`}>
              <DumpIcon status={status} className="size-3" />
              {t(`dump.count.${status}`, { count })}
            </Badge>
          </li>
        ))}
      </ul>
      {report.dat && (
        <p className="text-xs text-muted-foreground">{t("dump.comparedWith", { version: report.dat.version, games: report.dat.games })}</p>
      )}
      <ul className="min-h-0 flex-1 overflow-auto rounded-md border text-sm">
        {sorted.map((game) => (
          <ResultRow key={`${game.path}:${game.ulIndex ?? ""}`} game={game} />
        ))}
      </ul>
    </div>
  );
}

function ResultRow({ game }: { game: GameVerification }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <li className="border-b p-2 last:border-0" data-testid="verify-row" data-status={game.status}>
      <div className="flex flex-wrap items-center gap-2">
        <DumpBadge status={game.status} reason={game.reason} />
        <span className="min-w-0 flex-1 truncate font-medium" title={game.title}>
          {game.title}
        </span>
        <span className="font-mono text-xs text-muted-foreground">{game.startup}</span>
        {game.hashes && (
          <button
            type="button"
            className="text-xs text-primary underline-offset-2 hover:underline"
            aria-expanded={open}
            onClick={() => setOpen(!open)}
          >
            {open ? t("dump.hideHashes") : t("dump.showHashes")}
          </button>
        )}
      </div>
      <div className="mt-1 flex flex-col gap-0.5 text-xs text-muted-foreground">
        {game.redump && (
          <span>
            {t("dump.matches")}{" "}
            <RedumpLink name={game.redump.name} url={game.redump.url} />
            {game.serialMatches === false && <span className="text-amber-600 dark:text-amber-400"> · {t("dump.serialMismatch")}</span>}
          </span>
        )}
        {game.reason && <span>{t(`dump.reasonLong.${game.reason}`)}</span>}
        {game.status === "badDump" && <span>{t("dump.badDumpHint")}</span>}
        {game.candidates.length > 0 && (
          <span>
            {t("dump.candidates")}{" "}
            {game.candidates.map((candidate, index) => (
              <span key={`${candidate.name}:${candidate.rom}`}>
                {index > 0 && ", "}
                <RedumpLink name={candidate.name} url={candidate.url} />
                {candidate.sameSize &&
                  ` (${candidate.rom.toLowerCase().endsWith(".bin") ? t("dump.sameSectors") : t("dump.sameSize")})`}
              </span>
            ))}
          </span>
        )}
        {game.cached && <span>{t("dump.fromCache")}</span>}
        {game.error && <span className="text-destructive">{game.error}</span>}
      </div>
      {open && game.hashes && (
        <dl className="mt-2 grid grid-cols-[max-content_1fr] gap-x-4 gap-y-0.5 rounded-md bg-muted/50 p-2 font-mono text-xs">
          <dt className="text-muted-foreground">{t("game.size")}</dt>
          <dd>
            {formatBytes(game.hashes.size)} ({game.hashes.size})
          </dd>
          <dt className="text-muted-foreground">CRC-32</dt>
          <dd className="wrap-anywhere">{game.hashes.crc32}</dd>
          <dt className="text-muted-foreground">SHA-1</dt>
          <dd className="wrap-anywhere">{game.hashes.sha1}</dd>
          {game.hashes.md5 && (
            <>
              <dt className="text-muted-foreground">MD5</dt>
              <dd className="wrap-anywhere">{game.hashes.md5}</dd>
            </>
          )}
        </dl>
      )}
    </li>
  );
}

function RedumpLink({ name, url }: { name: string; url: string | null }) {
  if (!url) return <span className="text-foreground">{name}</span>;
  return (
    <a href={url} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-primary underline-offset-2 hover:underline">
      {name}
      <ExternalLink className="size-3" aria-hidden />
    </a>
  );
}
