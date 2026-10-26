import { cn } from "cn";
import { ChevronDown, FolderPlus } from "lucide-react";
import { useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";

import { useApi } from "@/api/context";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { DeviceMode } from "@/api/generated/DeviceMode";
import { waitForJob } from "@/api/rpc";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { wroteAll } from "@/jobs/outcome";
import { phraseText } from "@/lib/phrase";
import { useSettings } from "@/settings/context";
import { deviceModes, mmceProfiles, profiles } from "@/settings/settings";

import { LoaderProfileLine } from "./LoaderProfile";
import { emulatorStep, smbStep } from "./smbStep";
import { useProfileChoice } from "./useLoaderProfile";

/** The folder a whole system disk's library goes into by default. */
const SHARE_FOLDER = "PS2";

/**
 * What the device is for, in two lines (ADR-0047): "Connection to the PS2:
 * over the network (SMB) — a folder on the computer's disk" and "OPL
 * version: OPL 1.2.0 beta — why". One "Change" menu sets both for this
 * device or goes back to auto. While the version question shows, it stands
 * for the version line. Shown wherever the question can be.
 */
export function DeviceLines({
  device,
  className,
  profileLine = true,
}: {
  device: DeviceInfo;
  className?: string;
  /** The version line; off where a loader block on the card names the same build. */
  profileLine?: boolean;
}) {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  const key = device.loader.key;
  const chosenMode: DeviceMode | "auto" = settings.modeOverrides[key] ?? "auto";
  const chooseMode = (value: string) => {
    const { [key]: _old, ...others } = settings.modeOverrides;
    update({ modeOverrides: value === "auto" ? others : { ...others, [key]: value as DeviceMode } });
  };
  const profile = useProfileChoice(device.loader);
  const { mode } = device;
  // On a memory card emulator's SD only builds with MMCE see the games
  // (ADR-0048): neither OPL 1.1.0 nor its "I don't know" is offered there.
  const emulator = mode.mode === "emulator";
  const choices = emulator
    ? mmceProfiles.map((value) => ({ value, label: t(`loader.answersEmulator.${value}`) }))
    : profiles.map((value) => ({ value, label: t(`settings.profiles.${value}`) }));
  const unknown = emulator ? "unknownMmce" : "unknown";
  const reason =
    mode.source === "override"
      ? t("devices.modeByOverride")
      : mode.source === "explicit"
        ? t("devices.modeByExplicit")
        : phraseText(mode.place, mode.placeText);
  return (
    <>
      <div className={className} data-testid="device-mode" data-mode={mode.mode} data-source={mode.source}>
        <span className="text-muted-foreground">{t("devices.connection")}: </span>
        <span className="font-medium" data-testid="device-mode-name">
          {t(`devices.modes.${mode.mode}`)}
        </span>
        <span className="text-muted-foreground" data-testid="device-mode-reason">
          {" "}
          — {reason}
        </span>{" "}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="link" size="sm" className="h-auto p-0 align-baseline text-xs" data-testid="device-choice-change">
              {t("loader.change")}
              <ChevronDown aria-hidden />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-max">
            <DropdownMenuLabel>{t("devices.modeForThisDevice")}</DropdownMenuLabel>
            <DropdownMenuRadioGroup value={chosenMode} onValueChange={chooseMode}>
              <DropdownMenuRadioItem value="auto" data-testid="device-mode-auto">
                {t("devices.modeAuto")}
              </DropdownMenuRadioItem>
              {deviceModes.map((value) => (
                <DropdownMenuRadioItem key={value} value={value} data-testid={`device-mode-${value}`}>
                  {t(`devices.modeOptions.${value}`)}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
            <DropdownMenuSeparator />
            <DropdownMenuLabel>{t("loader.forThisDevice")}</DropdownMenuLabel>
            <DropdownMenuRadioGroup value={profile.chosen} onValueChange={profile.choose}>
              <DropdownMenuRadioItem value="auto" data-testid="loader-profile-auto">
                {t("loader.auto")}
              </DropdownMenuRadioItem>
              {choices.map(({ value, label }) => (
                <DropdownMenuRadioItem key={value} value={value} data-testid={`loader-profile-${value}`}>
                  {label}
                </DropdownMenuRadioItem>
              ))}
              <DropdownMenuRadioItem value={unknown} data-testid={`loader-profile-${unknown}`}>
                {t(emulator ? "loader.unknownMmce" : "loader.unknown")}
              </DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {!device.askProfile && profileLine && <LoaderProfileLine loader={device.loader} className={className} />}
    </>
  );
}

/** `name` inside the folder `root`, with the separator `root` uses. */
function childPath(root: string, name: string): string {
  const separator = root.includes("\\") ? "\\" : "/";
  return root.endsWith(separator) ? `${root}${name}` : `${root}${separator}${name}`;
}

/**
 * What `smbStep` says, with its button: "Create OPL folders" (LIB-3), or
 * "Create folder…", which plans a library folder inside the disk and opens
 * it once written. A server's roots are its administrator's: there the
 * system disk keeps only the doctor's warning. A memory card emulator's SD
 * gets "Create OPL folders" once the version question is answered
 * (`emulatorStep`, ADR-0048).
 */
export function SmbFolderHint({ device, className }: { device: DeviceInfo; className?: string }) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { update } = useSettings();
  const navigate = useNavigate();
  const [naming, setNaming] = useState(false);
  const opening = useRef<string | null>(null);
  const flow = usePlan({
    onExecuted: (job) => {
      const folder = opening.current;
      opening.current = null;
      if (folder === null) return;
      void waitForJob(transport, job).then((info) => {
        // Rolled back: nothing to open; the notification says why.
        if (!wroteAll(info)) return;
        update({ root: childPath(device.root, folder) });
        void navigate("/library");
      });
    },
    onDiscarded: () => {
      opening.current = null;
    },
  });
  const emulator = emulatorStep(device) === "init";
  const step = emulator ? "init" : smbStep(device);
  if (step === null || (step === "folder" && transport.kind === "http")) return null;
  const hint = emulator ? "devices.emulatorInitHint" : step === "folder" ? "devices.smbFolderHint" : "devices.smbInitHint";

  return (
    <div
      className={cn("flex flex-wrap items-center gap-2 rounded-md border bg-card p-3 text-sm", className)}
      data-testid={emulator ? "emulator-init" : step === "folder" ? "smb-create-folder" : "smb-init"}
    >
      <FolderPlus className="size-4 shrink-0 text-muted-foreground" aria-hidden />
      <span className="min-w-0 flex-1">{t(hint)}</span>
      {step === "folder" ? (
        <Button size="sm" onClick={() => setNaming(true)} data-testid="smb-create-folder-button">
          {t("devices.createFolder")}
        </Button>
      ) : (
        <Button
          size="sm"
          onClick={() => void flow.request({ type: "PlanInit", root: device.root })}
          data-testid="smb-init-button"
        >
          {t("devices.createOplFolders")}
        </Button>
      )}
      <FolderNameDialog
        root={device.root}
        open={naming}
        onOpenChange={setNaming}
        onSubmit={(folder) => {
          setNaming(false);
          opening.current = folder;
          void flow.request({ type: "PlanInit", root: device.root, folder });
        }}
      />
      <PlanDialog flow={flow} />
    </div>
  );
}

function FolderNameDialog({
  root,
  open,
  onOpenChange,
  onSubmit,
}: {
  root: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (folder: string) => void;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [name, setName] = useState(SHARE_FOLDER);
  const folder = name.trim();
  // One folder at the root: the core refuses separators too.
  const valid = folder.length > 0 && !/[\\/:]/.test(folder) && folder !== "." && folder !== "..";
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (next) setName(SHARE_FOLDER);
        onOpenChange(next);
      }}
    >
      <DialogContent>
        <form
          className="contents"
          onSubmit={(event) => {
            event.preventDefault();
            if (valid) onSubmit(folder);
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("devices.folderDialogTitle")}</DialogTitle>
            <DialogDescription>{t("devices.folderDialogDescription", { root })}</DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor={`${id}-name`}>{t("devices.folderName")}</Label>
            <Input
              id={`${id}-name`}
              value={name}
              onChange={(event) => setName(event.target.value)}
              aria-invalid={!valid}
              data-testid="smb-folder-name"
            />
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={!valid} data-testid="smb-folder-submit">
              {t("devices.folderSubmit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
