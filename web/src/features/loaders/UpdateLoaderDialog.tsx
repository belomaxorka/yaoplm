import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, Download, Loader2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { InstalledLoader } from "@/api/generated/InstalledLoader";
import type { JobId } from "@/api/generated/JobId";
import { cachedLoaderRelease, downloadLoader, waitForJob } from "@/api/rpc";
import { ExternalLink } from "@/components/ExternalLink";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { formatBytes, formatDate } from "@/lib/format";
import { errorText } from "@/lib/phrase";
import { useSettings } from "@/settings/context";

import { compareUrl, useLoaderBuild, versionLabel } from "./versions";

/**
 * "Update available" (LDR-4, SPEC §5.10.1): the installed build and the
 * newest of its channel as the store last read it, without the network;
 * the change list and links; then the download, held to the hash shown,
 * and the update plan. `offered` is the window the device list opened on
 * its own: its "Not now" is remembered by the caller.
 */
export function UpdateLoaderDialog({
  device,
  loader,
  open,
  onOpenChange,
  offered = false,
}: {
  device: DeviceInfo;
  loader: InstalledLoader;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  offered?: boolean;
}) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { settings } = useSettings();
  const queryClient = useQueryClient();
  const build = useLoaderBuild(loader.build);
  const [busy, setBusy] = useState<"download" | "finish" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const flow = usePlan({ onExecuted: (job) => void finish(job) });
  const offer = useQuery({
    queryKey: ["cachedLoaderRelease", loader.build],
    queryFn: () => cachedLoaderRelease(transport, loader.build ?? ""),
    enabled: open && loader.build !== null,
  });

  const close = () => {
    setError(null);
    onOpenChange(false);
  };

  async function finish(job: JobId) {
    setBusy("finish");
    try {
      const info = await waitForJob(transport, job);
      if (info.status !== "finished") {
        setError(info.error ? errorText(info.error) : t("loaderInstall.notFinished"));
        return;
      }
      await queryClient.invalidateQueries({ queryKey: ["devices"] });
      close();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  const update = async () => {
    if (!offer.data) return;
    setError(null);
    setBusy("download");
    try {
      const file = await downloadLoader(transport, offer.data, loader.language !== null, settings.loaderMirror.trim());
      void flow.request({ type: "PlanUpdateLoader", root: device.root, path: loader.path, sha256: file.sha256 });
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  };

  const latest = offer.data;
  const compare = build && latest ? compareUrl(build.repo, loader.version, latest.version) : null;
  return (
    <>
      <Dialog
        open={open && flow.plan === null && !flow.pending && flow.error === null}
        onOpenChange={(next) => {
          if (!next && busy === null) close();
        }}
      >
        <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-xl" data-testid="loader-update-dialog">
          <DialogHeader>
            <DialogTitle>{offered ? t("loaderUpdate.availableTitle") : t("loaderUpdate.title")}</DialogTitle>
            <DialogDescription>
              {t("loaderUpdate.description", { build: build?.name ?? loader.family, path: loader.path, root: device.root })}
            </DialogDescription>
          </DialogHeader>
          <div className="relative flex min-h-0 flex-1 flex-col gap-3 overflow-auto pr-1 text-sm">
            {offer.isPending && (
              <p className="flex items-center gap-2 text-muted-foreground" role="status">
                <Loader2 className="size-4 animate-spin" aria-hidden />
                {t("loaderInstall.loadingRelease")}
              </p>
            )}
            {offer.data === null && <p className="text-muted-foreground">{t("loaderUpdate.notRead")}</p>}
            {latest && (
              <>
                <div className="flex flex-wrap items-center gap-2" data-testid="loader-update-versions">
                  <span className="font-mono">{loader.version ? versionLabel(loader.version) : "?"}</span>
                  <ArrowRight className="size-4 text-muted-foreground" aria-label={t("loaderUpdate.to")} />
                  <span className="font-mono font-medium">{versionLabel(latest.version)}</span>
                </div>
                <p className="text-xs text-muted-foreground">
                  {[
                    latest.date && t("loaderInstall.released", { date: formatDate(new Date(`${latest.date}T12:00:00`).getTime()) }),
                    t("loaderInstall.size", { size: formatBytes(latest.file.size) }),
                    loader.checkedAt !== null && t("loaderUpdate.checked", { date: formatDate(loader.checkedAt) }),
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
                {latest.changelog.length > 0 ? (
                  <ul className="list-inside list-disc text-xs text-muted-foreground" data-testid="loader-update-changes">
                    {latest.changelog.map((change, index) => (
                      <li key={index} className="wrap-anywhere">
                        {change.link ? <ExternalLink href={change.link}>{change.text}</ExternalLink> : change.text}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="text-xs text-muted-foreground">{t("loaderInstall.noChanges")}</p>
                )}
                <p className="flex flex-wrap gap-x-3 text-xs">
                  <ExternalLink href={latest.releaseUrl}>{t("loaderInstall.release")}</ExternalLink>
                  {latest.commitUrl && <ExternalLink href={latest.commitUrl}>{t("loaderInstall.commit")}</ExternalLink>}
                  {compare && <ExternalLink href={compare}>{t("loaderUpdate.compare")}</ExternalLink>}
                </p>
                <p className="rounded-md border bg-muted/40 p-3 text-xs">
                  {loader.language
                    ? t("loaderUpdate.whatChangesWithLanguage", { path: loader.path, language: loader.language })
                    : t("loaderUpdate.whatChanges", { path: loader.path })}
                </p>
              </>
            )}
            {(error ?? offer.error) && (
              <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-destructive" role="alert">
                {error ?? errorText(offer.error)}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={close} disabled={busy !== null} data-testid="loader-update-later">
              {offered ? t("loaderUpdate.notNow") : t("common.cancel")}
            </Button>
            <Button onClick={() => void update()} disabled={!latest || busy !== null} data-testid="loader-update-start">
              {busy !== null ? <Loader2 className="animate-spin" aria-hidden /> : <Download aria-hidden />}
              {busy === "download" ? t("loaderInstall.downloading") : busy === "finish" ? t("loaderInstall.finishing") : t("loaderUpdate.update")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <PlanDialog flow={flow} />
    </>
  );
}
