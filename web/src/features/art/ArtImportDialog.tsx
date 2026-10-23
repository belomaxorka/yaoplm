import { isTauri } from "@tauri-apps/api/core";
import { Ban, FileArchive, Folder, FolderInput, FolderOpen, Image as ImageIcon, Loader2, X } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import type { ArtType } from "@/api/generated/ArtType";
import type { JobId } from "@/api/generated/JobId";
import type { PlanView } from "@/api/generated/PlanView";
import { importArt, waitForJob } from "@/api/rpc";
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
import { Switch } from "@/components/ui/switch";
import { listenForDrops, PICTURE_SET_EXTENSIONS, pickFolder, pickPictureSets } from "@/features/add/desktop";
import { FolderBrowserDialog } from "@/features/add/FolderBrowserDialog";
import { isImageName } from "@/features/art/clipboard";
import { artTypes } from "@/features/art/kinds";
import { JobLog, JobProgressLine } from "@/features/jobs/JobsPage";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { useJobs } from "@/jobs/context";
import { errorText } from "@/lib/phrase";
import { useSettings } from "@/settings/context";

/**
 * "Import pictures" (ART-5): picture sets the user has (OPL Manager's art
 * backups, folders, `art.tar`) are read by an ImportArt job, which keeps
 * the pictures of the games on the device and ends with a plan to confirm.
 */
export function ArtImportDialog({
  root,
  open,
  onOpenChange,
}: {
  root: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
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
        <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-xl" data-testid="art-import-dialog">
          <DialogHeader>
            <DialogTitle>{t("artImport.title")}</DialogTitle>
            <DialogDescription>{t("artImport.description")}</DialogDescription>
          </DialogHeader>
          {/* Mounted only while open, so every opening starts afresh. */}
          <ImportBody
            root={root}
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

/** `current` with the new paths after it, each once. */
function withSets(current: string[], paths: string[]): string[] {
  return [...current, ...paths.filter((path, i) => !current.includes(path) && paths.indexOf(path) === i)];
}

function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function SetIcon({ path }: { path: string }) {
  const name = baseName(path);
  const dot = name.lastIndexOf(".");
  const Icon = isImageName(name)
    ? ImageIcon
    : dot >= 0 && PICTURE_SET_EXTENSIONS.includes(name.slice(dot + 1).toLowerCase())
      ? FileArchive
      : Folder;
  return <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden />;
}

function ImportBody({
  root,
  onRunning,
  onPlan,
  onClose,
}: {
  root: string;
  onRunning: (running: boolean) => void;
  onPlan: (plan: PlanView) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { settings } = useSettings();
  const { jobs, cancel } = useJobs();
  const ids = useId();
  const desktop = isTauri();
  const [sets, setSets] = useState<string[]>([]);
  const [kinds, setKinds] = useState<ArtType[]>([...artTypes]);
  const [replace, setReplace] = useState(false);
  const [browserOpen, setBrowserOpen] = useState(false);
  const [jobId, setJobId] = useState<JobId | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  const job = jobId === null ? undefined : jobs.find((j) => j.id === jobId);
  const active = job !== undefined && (job.status === "queued" || job.status === "running");
  const showForm = jobId === null;

  const addSets = (paths: string[]) => setSets((current) => withSets(current, paths));

  // Sets dropped onto the window while the form is shown (desktop only).
  useEffect(
    () => (showForm ? listenForDrops((paths) => setSets((current) => withSets(current, paths))) : undefined),
    [showForm],
  );

  const start = async () => {
    setStarting(true);
    setError(null);
    try {
      const id = await importArt(transport, {
        type: "ImportArt",
        root,
        sources: sets,
        profile: settings.profile,
        kinds,
        overwrite: replace,
      });
      setJobId(id);
      onRunning(true);
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

  return (
    <>
      {showForm && (
        <div className="flex min-h-0 flex-col gap-4 overflow-auto">
          <section className="flex flex-col gap-2" aria-labelledby={`${ids}-sets`}>
            <h3 id={`${ids}-sets`} className="text-sm font-medium">
              {t("artImport.sets")}
            </h3>
            {sets.length === 0 ? (
              <p className="rounded-md border border-dashed p-3 text-sm text-muted-foreground">
                {desktop ? t("artImport.noSetsDesktop") : t("artImport.noSets")}
              </p>
            ) : (
              <ul className="max-h-40 overflow-auto rounded-md border" data-testid="art-import-sets">
                {sets.map((path) => (
                  <li key={path} className="flex items-center gap-2 border-b px-2 py-1 text-sm last:border-0">
                    <SetIcon path={path} />
                    <span className="min-w-0 flex-1 truncate" title={path}>
                      {baseName(path)}
                    </span>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onClick={() => setSets((current) => current.filter((p) => p !== path))}
                      aria-label={t("artImport.remove", { name: baseName(path) })}
                    >
                      <X aria-hidden />
                    </Button>
                  </li>
                ))}
              </ul>
            )}
            <div className="flex flex-wrap gap-2">
              {desktop ? (
                <>
                  <Button variant="outline" size="sm" onClick={() => void pickPictureSets().then(addSets)}>
                    <FileArchive aria-hidden />
                    {t("artImport.addFiles")}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void pickFolder().then((folder) => folder && addSets([folder]))}
                  >
                    <FolderOpen aria-hidden />
                    {t("artImport.addFolder")}
                  </Button>
                </>
              ) : (
                <Button variant="outline" size="sm" onClick={() => setBrowserOpen(true)} data-testid="art-import-browse">
                  <FolderOpen aria-hidden />
                  {t("artImport.browse")}
                </Button>
              )}
            </div>
            <p className="text-xs text-muted-foreground">{t("artImport.setsHint")}</p>
          </section>
          <fieldset data-testid="art-import-kinds">
            <legend className="mb-2 text-sm font-medium">{t("art.kindsLegend")}</legend>
            <ul className="grid grid-cols-2 gap-x-4 gap-y-1.5 sm:grid-cols-3">
              {artTypes.map((kind) => (
                <li key={kind}>
                  <Label htmlFor={`${ids}-kind-${kind}`} className="font-normal">
                    <Checkbox
                      id={`${ids}-kind-${kind}`}
                      checked={kinds.includes(kind)}
                      onCheckedChange={(on) =>
                        setKinds((current) => (on === true ? [...current, kind] : current.filter((k) => k !== kind)))
                      }
                    />
                    {t(`art.kinds.${kind}`)}
                  </Label>
                </li>
              ))}
            </ul>
          </fieldset>
          <Label htmlFor={`${ids}-replace`} className="items-start font-normal">
            <Switch
              id={`${ids}-replace`}
              checked={replace}
              onCheckedChange={setReplace}
              className="mt-0.5"
              data-testid="art-import-replace"
            />
            <span>
              {t("artImport.replace")}
              <span className="block text-xs text-muted-foreground">
                {replace ? t("artImport.replaceOn") : t("artImport.replaceOff")}
              </span>
            </span>
          </Label>
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
          {job.status === "cancelled" && <p className="text-sm text-muted-foreground">{t("artImport.cancelled")}</p>}
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
              disabled={starting || sets.length === 0 || kinds.length === 0}
              data-testid="art-import-start"
            >
              {starting ? <Loader2 className="animate-spin" aria-hidden /> : <FolderInput aria-hidden />}
              {t("artImport.start")}
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
      <FolderBrowserDialog open={browserOpen} onOpenChange={setBrowserOpen} onPick={addSets} mode="pack" />
    </>
  );
}
