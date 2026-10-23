import { Ban, Globe, ImageDown, Loader2 } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router";

import { useApi } from "@/api/context";
import type { ArtScope } from "@/api/generated/ArtScope";
import type { ArtType } from "@/api/generated/ArtType";
import type { Game } from "@/api/generated/Game";
import type { JobId } from "@/api/generated/JobId";
import type { PlanView } from "@/api/generated/PlanView";
import { artOf, hasCover } from "@/api/media";
import { fetchArt, waitForJob } from "@/api/rpc";
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
import { artTypes, useFetchableTypes } from "@/features/art/kinds";
import { artSourceInfo, enabledSources, requestSources } from "@/features/art/sources";
import { JobLog, JobProgressLine } from "@/features/jobs/JobsPage";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { useJobs } from "@/jobs/context";
import { errorText } from "@/lib/phrase";
import { useSettings } from "@/settings/context";

/**
 * "Download covers" (ART-1, ART-4): pick the scope, start the FetchArt
 * job, watch its per-game log, then confirm the plan it produced. The
 * network is touched only after Start; nothing is written before the plan
 * is confirmed (SPEC §6).
 */
export function ArtFetchDialog({
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
  /** Limit the search to one game (the game card). */
  only?: Game;
}) {
  const { t } = useTranslation();
  const flow = usePlan({ onExecuted: () => onOpenChange(false) });
  const [running, setRunning] = useState(false);

  return (
    <>
      <Dialog
        open={open && flow.plan === null && !flow.pending}
        onOpenChange={(next) => {
          if (!next && running) return;
          onOpenChange(next);
        }}
      >
        <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-xl" data-testid="art-fetch-dialog">
          <DialogHeader>
            <DialogTitle>{only ? t("art.fetchOneTitle", { title: only.title }) : t("art.fetchTitle")}</DialogTitle>
            <DialogDescription>{t("art.fetchDescription")}</DialogDescription>
          </DialogHeader>
          {/* Mounted only while open, so every opening starts afresh. */}
          <FetchBody
            root={root}
            games={games}
            only={only}
            onRunning={setRunning}
            onPlan={(plan) => flow.adopt(plan)}
            onClose={() => onOpenChange(false)}
          />
        </DialogContent>
      </Dialog>
      <PlanDialog flow={flow} />
    </>
  );
}

function FetchBody({
  root,
  games,
  only,
  onRunning,
  onPlan,
  onClose,
}: {
  root: string;
  games: readonly Game[];
  only?: Game;
  onRunning: (running: boolean) => void;
  onPlan: (plan: PlanView) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { settings } = useSettings();
  const { jobs, cancel } = useJobs();
  const ids = useId();
  const [scope, setScope] = useState<ArtScope>(only && hasCover(only) ? "all" : "missing");
  const fetchable = useFetchableTypes();
  const [kinds, setKinds] = useState<ArtType[]>(["COV"]);
  const chosen = kinds.filter((kind) => fetchable.has(kind));
  const coversOnly = chosen.length === 1 && chosen[0] === "COV";
  const [jobId, setJobId] = useState<JobId | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  const inScope = only ? [only] : games;
  const missing = coversOnly
    ? inScope.filter((g) => !hasCover(g)).length
    : inScope.reduce((sum, g) => sum + chosen.filter((kind) => !artOf(g, kind)).length, 0);
  const sources = enabledSources(settings);
  const job = jobId === null ? undefined : jobs.find((j) => j.id === jobId);
  const active = job !== undefined && (job.status === "queued" || job.status === "running");

  const start = async () => {
    setStarting(true);
    setError(null);
    try {
      const id = await fetchArt(transport, {
        type: "FetchArt",
        root,
        profile: settings.profile,
        scope,
        kinds: chosen,
        ...(only ? { games: [only.startup] } : {}),
        sources: requestSources(settings),
      });
      setJobId(id);
      onRunning(true);
      // Progress and the per-game log arrive as events; the plan with the result.
      const info = await waitForJob(transport, id);
      if (info.status === "finished" && info.result?.type === "Plan") {
        const { type: _type, ...plan } = info.result;
        onPlan(plan);
      }
    } catch (err) {
      setError(errorText(err));
    } finally {
      setStarting(false);
      onRunning(false);
    }
  };

  const showForm = jobId === null;
  return (
    <>
      {showForm && (
        <div className="flex flex-col gap-4">
          <fieldset data-testid="art-kinds">
            <legend className="mb-2 text-sm font-medium">{t("art.kindsLegend")}</legend>
            <ul className="grid grid-cols-2 gap-x-4 gap-y-1.5 sm:grid-cols-4">
              {artTypes
                .filter((kind) => fetchable.has(kind))
                .map((kind) => (
                  <li key={kind}>
                    <Label htmlFor={`${ids}-kind-${kind}`} className="font-normal">
                      <Checkbox
                        id={`${ids}-kind-${kind}`}
                        checked={kinds.includes(kind)}
                        onCheckedChange={(on) =>
                          setKinds((current) => (on === true ? [...current, kind] : current.filter((k) => k !== kind)))
                        }
                        data-testid={`art-kind-${kind}`}
                      />
                      {t(`art.kinds.${kind}`)}
                    </Label>
                  </li>
                ))}
            </ul>
            <p className="mt-1.5 text-xs text-muted-foreground">{t("art.kindsHint")}</p>
          </fieldset>
          {/* One game: the dialog title names it, there is nothing to choose. */}
          <fieldset hidden={only !== undefined}>
            <legend className="mb-2 text-sm font-medium">{t("art.scope")}</legend>
            <RadioGroup value={scope} onValueChange={(value) => setScope(value as ArtScope)}>
              <Label htmlFor={`${ids}-missing`} className="items-start font-normal">
                <RadioGroupItem id={`${ids}-missing`} value="missing" className="mt-0.5" data-testid="art-scope-missing" />
                <span>
                  {coversOnly ? t("art.scopeMissing", { count: missing }) : t("art.scopeMissingArt", { count: missing })}
                  <span className="block text-xs text-muted-foreground">{t("art.scopeMissingHint")}</span>
                </span>
              </Label>
              <Label htmlFor={`${ids}-all`} className="items-start font-normal">
                <RadioGroupItem id={`${ids}-all`} value="all" className="mt-0.5" data-testid="art-scope-all" />
                <span>
                  {t("art.scopeAll", { count: inScope.length })}
                  <span className="block text-xs text-muted-foreground">{t("art.scopeAllHint")}</span>
                </span>
              </Label>
            </RadioGroup>
          </fieldset>
          <div className="rounded-md border bg-muted/40 p-3 text-sm">
            <div className="mb-1 flex items-center gap-2 font-medium">
              <Globe className="size-4" aria-hidden />
              {t("art.sources")}
            </div>
            {sources.length === 0 ? (
              <p className="text-destructive">{t("art.noSources")}</p>
            ) : (
              <ol className="list-inside list-decimal text-muted-foreground">
                {sources.map((id) => (
                  <li key={id}>
                    {artSourceInfo[id].name}
                    {settings.artSources[id].mirror.trim() && <span className="ml-1 text-xs">({t("art.mirrorUsed")})</span>}
                  </li>
                ))}
              </ol>
            )}
            <p className="mt-2 text-xs text-muted-foreground">
              {t("art.sourcesHint")}{" "}
              <Link to="/settings" className="underline underline-offset-2">
                {t("nav.settings")}
              </Link>
            </p>
          </div>
        </div>
      )}
      {error && (
        <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
          {error}
        </p>
      )}
      {job && (
        <div className="flex min-h-0 flex-1 flex-col gap-3">
          <JobProgressLine job={job} />
          {job.error && (
            <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
              {errorText(job.error)}
            </p>
          )}
          {job.status === "cancelled" && <p className="text-sm text-muted-foreground">{t("art.cancelled")}</p>}
          <div className="min-h-0 flex-1 overflow-auto">
            <JobLog entries={job.log} />
          </div>
        </div>
      )}
      <DialogFooter>
        {showForm ? (
          <>
            <Button variant="outline" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={() => void start()}
              disabled={starting || sources.length === 0 || chosen.length === 0 || (scope === "missing" && missing === 0)}
              data-testid="art-fetch-start"
            >
              {starting ? <Loader2 className="animate-spin" aria-hidden /> : <ImageDown aria-hidden />}
              {t("art.start")}
            </Button>
          </>
        ) : active ? (
          <Button variant="outline" onClick={() => void cancel(job.id)}>
            <Ban aria-hidden />
            {t("jobs.cancel")}
          </Button>
        ) : (
          <Button variant="outline" onClick={onClose}>
            {t("common.close")}
          </Button>
        )}
      </DialogFooter>
    </>
  );
}
