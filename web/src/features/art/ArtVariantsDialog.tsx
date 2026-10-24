import { cn } from "cn";
import { Ban, Check, Loader2 } from "lucide-react";
import { RadioGroup as RadioGroupPrimitive } from "radix-ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import type { ArtType } from "@/api/generated/ArtType";
import type { ArtVariant } from "@/api/generated/ArtVariant";
import type { Game } from "@/api/generated/Game";
import type { JobId } from "@/api/generated/JobId";
import { findArt, waitForJob } from "@/api/rpc";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { RadioGroup } from "@/components/ui/radio-group";
import { artAspect } from "@/features/art/kinds";
import { requestSources } from "@/features/art/sources";
import { JobLog, JobProgressLine } from "@/features/jobs/JobsPage";
import { useJobs } from "@/jobs/context";
import { errorText } from "@/lib/phrase";
import { useSettings } from "@/settings/context";

/** The file name at the end of a URL or a path, with `%20` and the like decoded. */
function fileName(origin: string): string {
  const name = origin.replace(/^.*[\\/]/, "");
  try {
    return decodeURIComponent(name);
  } catch {
    return name;
  }
}

/**
 * "Download" on a picture slot (ART-2, SPEC §5.4.1): every picture of the
 * type the sources have for the game, already resized, to pick from; the
 * first is preselected. The pick goes on as a plan to confirm.
 */
export function ArtVariantsDialog({
  root,
  game,
  kind,
  open,
  onOpenChange,
  onPick,
}: {
  root: string;
  game: Game;
  kind: ArtType;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (variant: ArtVariant) => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-2xl" data-testid="art-variants-dialog">
        <DialogHeader>
          <DialogTitle>{t("art.variantsTitle", { kind: t(`art.kinds.${kind}`), title: game.title })}</DialogTitle>
          <DialogDescription>{t("art.variantsDescription")}</DialogDescription>
        </DialogHeader>
        {/* Mounted only while open, so every opening asks again. */}
        {open && <VariantsBody root={root} game={game} kind={kind} onClose={() => onOpenChange(false)} onPick={onPick} />}
      </DialogContent>
    </Dialog>
  );
}

function VariantsBody({
  root,
  game,
  kind,
  onClose,
  onPick,
}: {
  root: string;
  game: Game;
  kind: ArtType;
  onClose: () => void;
  onPick: (variant: ArtVariant) => void;
}) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { settings } = useSettings();
  const { jobs, cancel } = useJobs();
  const [jobId, setJobId] = useState<JobId | null>(null);
  const [variants, setVariants] = useState<ArtVariant[] | null>(null);
  const [chosen, setChosen] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const poll = new AbortController();
    let job: JobId | null = null;
    const run = async () => {
      try {
        const id = await findArt(transport, {
          type: "FindArt",
          root,
          startup: game.startup,
          kind,
          profile: settings.profile,
          sources: requestSources(settings),
        });
        if (poll.signal.aborted) {
          void cancel(id).catch(() => undefined);
          return;
        }
        job = id;
        setJobId(id);
        const info = await waitForJob(transport, id, poll.signal);
        job = null;
        if (poll.signal.aborted) return;
        if (info.status === "finished" && info.result?.type === "ArtVariants") setVariants(info.result.variants);
        else if (info.error) setError(errorText(info.error));
        else setVariants([]);
      } catch (err) {
        if (!poll.signal.aborted) setError(errorText(err));
      }
    };
    // Started a tick later: StrictMode's development double effect cleans up
    // synchronously and drops the first start, so one opening runs one job.
    const timer = setTimeout(() => void run(), 0);
    // Closing the dialog stops a search nobody waits for.
    return () => {
      clearTimeout(timer);
      poll.abort();
      if (job !== null) void cancel(job).catch(() => undefined);
    };
  }, [transport, root, game.startup, kind, settings, cancel]);

  const job = jobId === null ? undefined : jobs.find((j) => j.id === jobId);
  const running = variants === null && error === null;
  const pick = variants?.[chosen];
  return (
    <>
      {running && job && <JobProgressLine job={job} />}
      {running && !job && (
        <p className="flex items-center gap-2 text-sm text-muted-foreground" role="status">
          <Loader2 className="size-4 animate-spin" aria-hidden />
          {t("art.variantsSearching")}
        </p>
      )}
      {error && (
        <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
          {error}
        </p>
      )}
      {variants && variants.length === 0 && (
        <div className="flex min-h-0 flex-col gap-2 text-sm">
          <p className="text-muted-foreground" data-testid="art-variants-none">
            {t("art.variantsNone")}
          </p>
          {job && job.log.length > 0 && <JobLog entries={job.log} />}
        </div>
      )}
      {variants && variants.length > 0 && (
        // Radix gives the cards roving focus: arrow keys move the choice.
        <RadioGroup
          value={String(chosen)}
          onValueChange={(value) => setChosen(Number(value))}
          aria-label={t(`art.kinds.${kind}`)}
          className="relative min-h-0 grid-cols-2 gap-3 overflow-auto p-1 sm:grid-cols-3"
        >
          {variants.map((variant, index) => (
            <RadioGroupPrimitive.Item
              key={`${variant.source}:${variant.origin}`}
              value={String(index)}
              className={cn(
                "flex w-full flex-col gap-1.5 rounded-lg border p-2 text-left transition-colors outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                index === chosen ? "border-primary ring-2 ring-primary" : "hover:bg-muted/60",
              )}
              title={variant.origin}
              data-testid="art-variant"
            >
              <img
                src={`data:image/png;base64,${variant.png}`}
                alt=""
                className="w-full rounded border bg-[repeating-conic-gradient(var(--muted)_0_25%,transparent_0_50%)] bg-[length:16px_16px] object-contain"
                style={{ aspectRatio: artAspect[kind] }}
              />
              <span className="flex items-center gap-1 text-xs font-medium">
                {index === chosen && <Check className="size-3" aria-hidden />}
                {variant.source}
              </span>
              <span className="truncate font-mono text-[0.65rem] text-muted-foreground">{fileName(variant.origin)}</span>
            </RadioGroupPrimitive.Item>
          ))}
        </RadioGroup>
      )}
      <DialogFooter>
        {running && jobId !== null ? (
          <Button variant="outline" onClick={() => void cancel(jobId)}>
            <Ban aria-hidden />
            {t("jobs.cancel")}
          </Button>
        ) : (
          <Button variant="outline" onClick={onClose}>
            {t("common.close")}
          </Button>
        )}
        <Button
          disabled={!pick}
          onClick={() => {
            if (!pick) return;
            onPick(pick);
            onClose();
          }}
          data-testid="art-variant-use"
        >
          {t("art.useVariant")}
        </Button>
      </DialogFooter>
    </>
  );
}
