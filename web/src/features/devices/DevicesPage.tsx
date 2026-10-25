import { useQuery } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { cn } from "cn";
import { ArrowLeftRight, CheckCircle2, Cpu, FolderOpen, HardDrive, RefreshCw, Usb } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";

import { useApi } from "@/api/context";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { InstalledLoader } from "@/api/generated/InstalledLoader";
import { listDevices } from "@/api/rpc";
import { EmptyState } from "@/components/EmptyState";
import { NoticeIcon, NoticeList } from "@/components/Notices";
import { PageScroll } from "@/components/PageScroll";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import { Switch } from "@/components/ui/switch";
import { pickFolder } from "@/features/add/desktop";
import { InstalledLoaderRow } from "@/features/loaders/InstalledLoader";
import { InstallLoaderDialog } from "@/features/loaders/InstallLoaderDialog";
import { UpdateLoaderDialog } from "@/features/loaders/UpdateLoaderDialog";
import { formatBytes } from "@/lib/format";
import { worstLevel } from "@/lib/notices";
import { errorText } from "@/lib/phrase";
import { useSettings, useSimpleMode } from "@/settings/context";
import { deviceChoice } from "@/settings/settings";

import { DeviceLines, SmbFolderHint } from "./DeviceMode";
import { LoaderProfileHint } from "./LoaderProfile";
import { smbStep } from "./smbStep";

/**
 * Device picker (DEV-1, DEV-2): every volume with its loader profile
 * (LIB-1), how the PS2 reads it (ADR-0047) and the storage doctor's verdict
 * for both. The server lists its roots.
 */
export function DevicesPage() {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { settings, update } = useSettings();
  const navigate = useNavigate();
  const [showAll, setShowAll] = useState(transport.kind === "http");
  const switchId = useId();
  const choice = deviceChoice(settings);
  const devices = useQuery({
    queryKey: ["devices", choice],
    queryFn: () => listDevices(transport, choice),
  });
  const shown = (devices.data ?? []).filter((d) => showAll || (d.external && !d.system));
  // "Update available" opens by itself once per device and installed build
  // (SPEC §5.10.1); whatever closes it counts as "Not now". Once open it
  // stays until closed, while the update it starts changes the list.
  const pending = settings.offerLoaderUpdates ? pendingOffer(shown, settings.loaderUpdatesOffered) : undefined;
  const [offer, setOffer] = useState<Offer | null>(null);
  if (offer === null && pending) setOffer(pending);

  const open = (root: string) => {
    update({ root });
    void navigate("/library");
  };

  return (
    <PageScroll>
      <div className="mx-auto flex max-w-4xl flex-col gap-4">
        <div className="flex flex-wrap items-center gap-3">
          <p className="flex-1 text-sm text-muted-foreground">
            {t("devices.intro")}
          </p>
          {transport.kind !== "http" && (
            <Label htmlFor={switchId} className="font-normal">
              <Switch id={switchId} checked={showAll} onCheckedChange={setShowAll} />
              {t("devices.showAll")}
            </Label>
          )}
          <Button variant="outline" size="sm" onClick={() => void devices.refetch()} disabled={devices.isFetching}>
            <RefreshCw className={cn(devices.isFetching && "animate-spin")} aria-hidden />
            {t("devices.refresh")}
          </Button>
          {isTauri() && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                void pickFolder().then((folder) => {
                  if (folder) open(folder);
                });
              }}
            >
              <FolderOpen aria-hidden />
              {t("devices.openFolder")}
            </Button>
          )}
        </div>

        {devices.isPending && (
          <p className="text-sm text-muted-foreground" role="status">
            {t("devices.loading")}
          </p>
        )}
        {devices.error && (
          <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
            {errorText(devices.error)}
          </p>
        )}
        {devices.data && shown.length === 0 && (
          <EmptyState
            icon={Usb}
            title={t("devices.emptyTitle")}
            description={showAll ? t("devices.emptyAllDescription") : t("devices.emptyDescription")}
          />
        )}
        <ul className="flex flex-col gap-3">
          {shown.map((device) => (
            <DeviceCard key={device.root} device={device} current={settings.root === device.root} onOpen={() => open(device.root)} />
          ))}
        </ul>
      </div>
      {offer && (
        <UpdateLoaderDialog
          key={offer.key}
          device={offer.device}
          loader={offer.loader}
          offered
          open
          onOpenChange={(next) => {
            if (next) return;
            update({ loaderUpdatesOffered: [...settings.loaderUpdatesOffered, offer.key].slice(-100) });
            setOffer(null);
          }}
        />
      )}
    </PageScroll>
  );
}

/**
 * No loader can work from a system disk, a read-only volume or a device OPL
 * cannot use (§5.10.1). A folder read over SMB needs none: the console starts
 * OPL from its memory card or stick, and a loader there is a copy (ADR-0047).
 */
function installable(device: DeviceInfo): boolean {
  return (
    device.mode.mode === "ps2" && !device.system && !device.readOnly && worstLevel(device.advice) !== "error"
  );
}

interface Offer {
  key: string;
  device: DeviceInfo;
  loader: InstalledLoader;
}

/** The first update on these devices not offered yet. */
function pendingOffer(devices: readonly DeviceInfo[], offered: readonly string[]): Offer | undefined {
  for (const device of devices.filter(installable)) {
    for (const loader of device.installed) {
      const key = `${device.loader.key}|${loader.sha256}`;
      if (loader.freshness === "update" && !offered.includes(key)) return { key, device, loader };
    }
  }
  return undefined;
}

function DeviceCard({ device, current, onOpen }: { device: DeviceInfo; current: boolean; onOpen: () => void }) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const simple = useSimpleMode();
  const verdict = worstLevel(device.advice);
  const [installing, setInstalling] = useState(false);
  const [replacing, setReplacing] = useState(false);
  const canInstall = installable(device);
  const known = device.installed.find((l) => l.build !== null);
  // §5.10.1: no loader, "Install…"; a known build, "Reinstall…" and
  // "Replace…"; only the family known, "Replace…" alone.
  const present = known ?? device.installed[0];
  // Simple mode keeps the advice that explains the verdict; what the loader
  // detection found (several loaders, JPG art) shows in both, unless a
  // profile was chosen instead.
  const advice = [
    ...(simple ? device.advice.filter((notice) => notice.level !== "info") : device.advice),
    ...(device.loader.source === "detected" ? (device.loader.detected?.notices ?? []) : []),
  ];
  const used = device.totalBytes > 0 ? Math.round(((device.totalBytes - device.freeBytes) / device.totalBytes) * 100) : 0;
  // A folder read over SMB may sit on the system disk (ADR-0047): the badge
  // is a fact there, not a verdict.
  const smb = device.mode.mode === "smb";
  // One main button: the folder's next step comes before opening it. A
  // server's system disk shows no step (its roots are the administrator's).
  const step = smbStep(device);
  const stepShown = step === "init" || (step === "folder" && transport.kind !== "http");
  return (
    <li
      className={cn("rounded-lg border bg-card p-4", current && "ring-2 ring-ring/40")}
      data-testid="device-card"
      data-verdict={verdict ?? "ok"}
    >
      <div className="flex flex-wrap items-start gap-3">
        <HardDrive className="mt-1 size-6 shrink-0 text-muted-foreground" aria-hidden />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-base font-semibold wrap-anywhere">{device.root}</span>
            {device.label && <span className="text-sm text-muted-foreground">{device.label}</span>}
            <Badge variant="outline">{device.fileSystem === "other" ? device.fsName : device.fileSystem}</Badge>
            {!simple && device.bus && <Badge variant="outline">{t(`devices.bus.${device.bus}`)}</Badge>}
            {!simple && <Badge variant="outline">{t(`devices.kind.${device.kind}`)}</Badge>}
            {device.system && <Badge variant={smb ? "outline" : "destructive"}>{t("devices.systemDisk")}</Badge>}
            {device.readOnly && <Badge variant="destructive">{t("devices.readOnly")}</Badge>}
            {current && <Badge>{t("devices.current")}</Badge>}
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
            <Progress value={used} className="h-1.5 w-full max-w-xs" aria-label={t("devices.used", { percent: used })} />
            <span className="font-mono text-xs whitespace-nowrap text-muted-foreground tabular-nums">
              {t("devices.free", { free: formatBytes(device.freeBytes), total: formatBytes(device.totalBytes) })}
            </span>
          </div>
          <div className="mt-2 flex flex-col gap-1 text-sm">
            <DeviceLines device={device} />
          </div>
          <SmbFolderHint device={device} className="mt-2" />
          <LoaderProfileHint device={device} className="mt-2" />
          {device.installed.map((loader) => (
            <InstalledLoaderRow key={loader.path} device={device} loader={loader} actions={canInstall} />
          ))}
          {!simple && (
            <dl className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
              {device.model && (
                <div>
                  <dt className="inline">{t("devices.model")}: </dt>
                  <dd className="inline">{device.model}</dd>
                </div>
              )}
              {device.serial && (
                <div>
                  <dt className="inline">{t("devices.serial")}: </dt>
                  <dd className="inline font-mono">{device.serial}</dd>
                </div>
              )}
              {device.clusterSize !== null && (
                <div>
                  <dt className="inline">{t("devices.cluster")}: </dt>
                  <dd className="inline">{formatBytes(device.clusterSize, 0)}</dd>
                </div>
              )}
              {device.partition?.table && (
                <div>
                  <dt className="inline">{t("devices.partition")}: </dt>
                  <dd className="inline">
                    {t(`devices.tables.${device.partition.table}`)} #{device.partition.number}
                  </dd>
                </div>
              )}
            </dl>
          )}
        </div>
        {/* Beside the details when there is room, under them when there is not. */}
        <div className="ml-auto flex flex-col items-end gap-2">
          <span
            className={cn(
              "flex items-center gap-1.5 text-sm font-medium",
              verdict === "error" && "text-destructive",
              verdict === "warning" && "text-amber-700 dark:text-amber-400",
              (verdict === null || verdict === "info") && "text-emerald-700 dark:text-emerald-400",
            )}
          >
            {verdict === null || verdict === "info" ? (
              <CheckCircle2 className="size-4" aria-hidden />
            ) : (
              <NoticeIcon level={verdict} />
            )}
            {t(`devices.verdict.${verdict ?? "ok"}`)}
          </span>
          <Button onClick={onOpen} variant={verdict === "error" || stepShown ? "outline" : "default"} data-testid="device-open">
            {t("devices.open")}
          </Button>
          {canInstall && (present === undefined || known) && (
            <Button variant="outline" size="sm" onClick={() => setInstalling(true)} data-testid="device-install-loader">
              <Cpu aria-hidden />
              {present ? t("devices.reinstallLoader") : t("devices.installLoader")}
            </Button>
          )}
          {canInstall && present && (
            <Button variant="outline" size="sm" onClick={() => setReplacing(true)} data-testid="device-replace-loader">
              <ArrowLeftRight aria-hidden />
              {t("devices.replaceLoader")}
            </Button>
          )}
        </div>
      </div>
      {advice.length > 0 && <NoticeList notices={advice} className="mt-3 border-t pt-3" />}
      {canInstall && (
        <InstallLoaderDialog
          device={device}
          open={installing}
          onOpenChange={setInstalling}
          initial={known ? { build: known.build, preset: known.preset } : undefined}
        />
      )}
      {canInstall && present && replacing && (
        <InstallLoaderDialog device={device} open={replacing} onOpenChange={setReplacing} replace={present} />
      )}
    </li>
  );
}
