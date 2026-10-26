import { useMutation, useQueryClient } from "@tanstack/react-query";
import { cn } from "cn";
import { ArrowUpCircle, CheckCircle2, Cpu, Loader2, RefreshCw, Undo2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { InstalledLoader } from "@/api/generated/InstalledLoader";
import { loaderRelease, updateLoaderCatalog } from "@/api/rpc";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { formatDate } from "@/lib/format";
import { errorText } from "@/lib/phrase";
import { useSettings, useSimpleMode } from "@/settings/context";

import { UpdateLoaderDialog } from "./UpdateLoaderDialog";
import { useLoaderBuild, versionLabel, versionShort } from "./versions";

/**
 * A loader on the device card (LDR-4, SPEC §5.10.1): the build, its
 * version and path, where it stands against the newest build of its
 * channel YAOPLM read, and, where a loader can work on the device,
 * "Update to …", "Check for updates" and putting the previous ELF back.
 * Nothing here goes to the network but "Check for updates".
 */
export function InstalledLoaderRow({
  device,
  loader,
  actions,
}: {
  device: DeviceInfo;
  loader: InstalledLoader;
  /** A loader can work on this device: the buttons show. */
  actions: boolean;
}) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { settings } = useSettings();
  const simple = useSimpleMode();
  const queryClient = useQueryClient();
  const build = useLoaderBuild(loader.build);
  const [updating, setUpdating] = useState(false);
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["devices"] });
  const rollback = usePlan({ onExecuted: () => void refresh() });
  // "Check for updates" reads the catalogue and the release (SPEC §5.10.1);
  // a catalogue that cannot be read leaves the one in force.
  const check = useMutation({
    mutationFn: async () => {
      const mirror = settings.loaderMirror.trim();
      await updateLoaderCatalog(transport, mirror).catch(() => undefined);
      await loaderRelease(transport, loader.build ?? "", mirror);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["loaderCatalog"] });
      void refresh();
    },
  });

  const name = build?.name ?? loader.family;
  const backup = loader.backup;
  return (
    <div
      className="mt-2 flex flex-col gap-1.5 rounded-md border bg-muted/30 px-3 py-2 text-sm"
      data-testid="installed-loader"
      data-path={loader.path}
      data-freshness={loader.freshness}
    >
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <Cpu className="size-4 shrink-0 text-muted-foreground" aria-hidden />
        <span className="font-medium">{name}</span>
        <span className="font-mono text-xs" data-testid="installed-loader-version">
          {loader.version ? versionLabel(loader.version) : t("loaderUpdate.versionUnknown")}
        </span>
        <span className="font-mono text-xs text-muted-foreground wrap-anywhere">{loader.path}</span>
        {loader.freshness === "current" && (
          <Badge variant="outline" className="text-emerald-700 dark:text-emerald-400" data-testid="installed-loader-current">
            <CheckCircle2 aria-hidden />
            {t("loaderUpdate.current")}
          </Badge>
        )}
        {loader.freshness === "update" && loader.latest && (
          <Badge className="bg-amber-600 text-white dark:bg-amber-500 dark:text-black" data-testid="installed-loader-update">
            <ArrowUpCircle aria-hidden />
            {t("loaderUpdate.available", { version: versionShort(loader.latest) })}
          </Badge>
        )}
      </div>
      {loader.manifestStale && (
        <p className="text-xs text-amber-700 dark:text-amber-400">{t("loaderUpdate.manifestStale")}</p>
      )}
      {!simple && loader.knownBy !== "manifest" && (
        <p className="text-xs text-muted-foreground">{t(`loaderUpdate.knownBy.${loader.knownBy}`)}</p>
      )}
      {loader.build !== null && loader.checkedAt !== null && (
        <p className="text-xs text-muted-foreground">{t("loaderUpdate.lastCheck", { date: formatDate(loader.checkedAt) })}</p>
      )}
      {actions && (
        <div className="flex flex-wrap items-center gap-2">
          {loader.freshness === "update" && loader.latest && (
            <Button size="sm" onClick={() => setUpdating(true)} data-testid="installed-loader-update-to">
              <ArrowUpCircle aria-hidden />
              {t("loaderUpdate.updateTo", { version: versionShort(loader.latest) })}
            </Button>
          )}
          {/* Quiet links: the card's one main button is "Open library". */}
          {loader.build !== null && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => check.mutate()}
              disabled={check.isPending}
              data-testid="installed-loader-check"
            >
              <RefreshCw className={cn(check.isPending && "animate-spin")} aria-hidden />
              {t("loaderUpdate.check")}
            </Button>
          )}
          {/* After a rollback the backup is the build on offer: "Update to …" says it once. */}
          {backup && backup.version !== loader.latest && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void rollback.request({ type: "PlanRollbackLoader", root: device.root, path: loader.path })}
              data-testid="installed-loader-rollback"
            >
              {rollback.pending ? <Loader2 className="animate-spin" aria-hidden /> : <Undo2 aria-hidden />}
              {backup.version
                ? t("loaderUpdate.rollbackTo", { version: versionShort(backup.version) })
                : t("loaderUpdate.rollbackFile")}
            </Button>
          )}
          {check.isSuccess && loader.freshness === "current" && (
            <span className="text-xs text-muted-foreground" role="status">
              {t("loaderUpdate.newest")}
            </span>
          )}
        </div>
      )}
      {check.error && (
        <p className="text-xs text-destructive" role="alert">
          {errorText(check.error)}
        </p>
      )}
      <PlanDialog flow={rollback} />
      {updating && <UpdateLoaderDialog device={device} loader={loader} open={updating} onOpenChange={setUpdating} />}
    </div>
  );
}
