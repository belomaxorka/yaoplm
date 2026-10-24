import { useQuery } from "@tanstack/react-query";
import { cn } from "cn";
import { Ban, Globe, ImageDown, Loader2 } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router";

import { useApi } from "@/api/context";
import type { ArtScope } from "@/api/generated/ArtScope";
import type { ArtType } from "@/api/generated/ArtType";
import type { Game } from "@/api/generated/Game";
import type { JobId } from "@/api/generated/JobId";
import type { PlanView } from "@/api/generated/PlanView";
import { artOf, hasCover } from "@/api/media";
import { attribution, fetchArt, waitForJob } from "@/api/rpc";
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
import { artSourceInfo, enabledSources, requestSources, takesMirror } from "@/features/art/sources";
import { JobLog, JobProgressLine } from "@/features/jobs/JobsPage";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { useJobs } from "@/jobs/context";
import { errorText } from "@/lib/phrase";
import { useSettings, useSimpleMode } from "@/settings/context";

/** What the user chose in the form. */
interface FetchForm {
  kinds: ArtType[];
  /** `null` until the user picks one: the default follows the game in `only`. */
  scope: ArtScope | null;
}

const freshForm = (): FetchForm => ({ kinds: ["COV"], scope: null });

/**
 * "Download pictures" (ART-1, ART-4): pick the types and the scope, start
 * the FetchArt job, watch its per-game log, then confirm the plan it
 * produced. The network is touched only after Start; nothing is written
 * before the plan is confirmed (SPEC §6).
 *
 * `quick` skips the form (simple mode, "Download all pictures" on a game):
 * every type the sources have, missing ones only, started on opening.
 */
export function ArtFetchDialog({
  root,
  games,
  open,
  onOpenChange,
  only,
  quick = false,
}: {
  root: string;
  games: readonly Game[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Limit the search to one game (the game card). */
  only?: Game;
  quick?: boolean;
}) {
  const { t } = useTranslation();
  // Kept here, not in the body: cancelling the plan brings the form back as it was.
  const [form, setForm] = useState<FetchForm>(freshForm);
  const close = () => {
    setForm(freshForm());
    onOpenChange(false);
  };
  // A quick search has no form to come back to: cancelling its plan ends it.
  const flow = usePlan({ onExecuted: close, ...(quick ? { onDiscarded: close } : {}) });
  const [running, setRunning] = useState(false);

  return (
    <>
      <Dialog
        open={open && flow.plan === null && !flow.pending}
        onOpenChange={(next) => {
          if (next) onOpenChange(true);
          else if (!running) close();
        }}
      >
        <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-xl" data-testid="art-fetch-dialog">
          <DialogHeader>
            <DialogTitle>{only ? t("art.fetchOneTitle", { title: only.title }) : t("art.fetchTitle")}</DialogTitle>
            <DialogDescription>{quick ? t("art.quickDescription") : t("art.fetchDescription")}</DialogDescription>
          </DialogHeader>
          {/* Mounted only while open; closing resets the form, so every opening starts afresh. */}
          {quick ? (
            <QuickBody
              root={root}
              games={games}
              only={only}
              onRunning={setRunning}
              onPlan={(plan) => flow.adopt(plan)}
              onClose={close}
            />
          ) : (
            <FetchBody
              root={root}
              games={games}
              only={only}
              form={form}
              onFormChange={setForm}
              onRunning={setRunning}
              onPlan={(plan) => flow.adopt(plan)}
              onClose={close}
            />
          )}
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
  form,
  onFormChange,
  onRunning,
  onPlan,
  onClose,
}: {
  root: string;
  games: readonly Game[];
  only?: Game;
  form: FetchForm;
  onFormChange: (change: (form: FetchForm) => FetchForm) => void;
  onRunning: (running: boolean) => void;
  onPlan: (plan: PlanView) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { settings } = useSettings();
  const { jobs, cancel } = useJobs();
  const ids = useId();
  const { kinds } = form;
  const scope = form.scope ?? (only && hasCover(only) ? "all" : "missing");
  const setScope = (value: ArtScope) => onFormChange((f) => ({ ...f, scope: value }));
  const setKinds = (change: (kinds: ArtType[]) => ArtType[]) => onFormChange((f) => ({ ...f, kinds: change(f.kinds) }));
  const fetchable = useFetchableTypes();
  const chosen = kinds.filter((kind) => fetchable.types.has(kind));
  const coversOnly = chosen.length === 1 && chosen[0] === "COV";
  const [jobId, setJobId] = useState<JobId | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  const inScope = only ? [only] : games;
  const missing = coversOnly
    ? inScope.filter((g) => !hasCover(g)).length
    : inScope.reduce((sum, g) => sum + chosen.filter((kind) => !artOf(g, kind)).length, 0);
  const about = useQuery({ queryKey: ["attribution"], queryFn: () => attribution(transport), staleTime: Infinity });
  const sources = enabledSources(settings, about.data?.art);
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
                .filter((kind) => fetchable.types.has(kind))
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
            {/* Switched-off sources are named in the sources box below. */}
            {fetchable.blocked && fetchable.blocked.why !== "noSources" && (
              <p
                className={cn(
                  "flex items-center gap-2 text-sm",
                  fetchable.blocked.why === "failed" ? "text-destructive" : "text-muted-foreground",
                )}
                role={fetchable.blocked.why === "failed" ? "alert" : "status"}
                data-testid="art-kinds-unavailable"
              >
                {fetchable.blocked.why === "loading" && <Loader2 className="size-4 animate-spin" aria-hidden />}
                {fetchable.blocked.text}
              </p>
            )}
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
              <ul className="list-inside list-disc text-muted-foreground">
                {sources.map((id) => (
                  <li key={id}>
                    {artSourceInfo[id].name}
                    {takesMirror(id) && settings.artSources[id].mirror.trim() && (
                      <span className="ml-1 text-xs">({t("art.mirrorUsed")})</span>
                    )}
                  </li>
                ))}
              </ul>
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

/**
 * The quick search: no choices. Starts as soon as the sources' types are
 * known, unless every type they have is on the device already.
 */
function QuickBody({
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
  const simple = useSimpleMode();
  const { jobs, cancel } = useJobs();
  const fetchable = useFetchableTypes();
  const kinds = artTypes.filter((kind) => fetchable.types.has(kind));
  const inScope = only ? [only] : games;
  const missing = inScope.reduce((sum, g) => sum + kinds.filter((kind) => !artOf(g, kind)).length, 0);
  const [jobId, setJobId] = useState<JobId | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [started, setStarted] = useState(false);
  const job = jobId === null ? undefined : jobs.find((j) => j.id === jobId);
  const active = job !== undefined && (job.status === "queued" || job.status === "running");
  const ready = fetchable.blocked === null && kinds.length > 0 && missing > 0;
  // One search per opening, also when effects run twice in development.
  const once = useRef(false);

  useEffect(() => {
    if (!ready || once.current) return;
    once.current = true;
    void (async () => {
      setStarted(true);
      onRunning(true);
      try {
        const id = await fetchArt(transport, {
          type: "FetchArt",
          root,
          profile: settings.profile,
          scope: "missing",
          kinds,
          ...(only ? { games: [only.startup] } : {}),
          sources: requestSources(settings),
        });
        setJobId(id);
        const info = await waitForJob(transport, id);
        if (info.status === "finished" && info.result?.type === "Plan") {
          const { type: _type, ...plan } = info.result;
          onPlan(plan);
        }
      } catch (err) {
        setError(errorText(err));
      } finally {
        onRunning(false);
      }
    })();
    // The search starts once, with the types and settings of that moment.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready]);

  return (
    <>
      {fetchable.blocked && (
        <p
          className={cn(
            "flex items-center gap-2 text-sm",
            fetchable.blocked.why === "loading" ? "text-muted-foreground" : "text-destructive",
          )}
          role={fetchable.blocked.why === "loading" ? "status" : "alert"}
        >
          {fetchable.blocked.why === "loading" && <Loader2 className="size-4 animate-spin" aria-hidden />}
          {fetchable.blocked.text}
        </p>
      )}
      {!fetchable.blocked && !started && missing === 0 && (
        <p className="text-sm" role="status" data-testid="art-nothing-missing">
          {t("art.nothingMissing")}
        </p>
      )}
      {error && (
        <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
          {error}
        </p>
      )}
      {(job || (started && !error)) && (
        <div className="flex min-h-0 flex-1 flex-col gap-3" data-testid="art-quick-progress">
          {job ? (
            <JobProgressLine job={job} />
          ) : (
            <p className="flex items-center gap-2 text-sm text-muted-foreground" role="status">
              <Loader2 className="size-4 animate-spin" aria-hidden />
              {t("art.searching")}
            </p>
          )}
          {job?.error && (
            <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
              {errorText(job.error)}
            </p>
          )}
          {job?.status === "cancelled" && <p className="text-sm text-muted-foreground">{t("art.cancelled")}</p>}
          {/* The per-game log is a detail for the advanced mode. */}
          {job && !simple && (
            <div className="min-h-0 flex-1 overflow-auto">
              <JobLog entries={job.log} />
            </div>
          )}
        </div>
      )}
      <DialogFooter>
        {active ? (
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
