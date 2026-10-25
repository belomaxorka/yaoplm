import { cn } from "cn";
import { CircleHelp } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ActiveProfile } from "@/api/generated/ActiveProfile";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import { Button } from "@/components/ui/button";
import { worstLevel } from "@/lib/notices";
import { phraseText } from "@/lib/phrase";
import { type ProfileOverrideValue, profiles } from "@/settings/settings";

import { useProfileChoice } from "./useLoaderProfile";

/** Why the profile is in force, to follow it after a dash. */
function useReason(loader: ActiveProfile, chosen: ProfileOverrideValue | "auto"): string {
  const { t } = useTranslation();
  if (loader.source === "override") return t(chosen === "unknown" ? "loader.byUnknown" : "loader.byOverride");
  // Only the CLI forces a profile for every device (ADR-0040).
  if (loader.source === "explicit") return t("loader.byExplicit");
  if (!loader.detected) return t("loader.unreadable");
  return phraseText(loader.detected.reason, loader.detected.reasonText);
}

/**
 * "OPL version: OPL 1.2.0 beta — why": the rules YAOPLM writes the device
 * for, not a loader found on it. Shown in both modes: a beginner needs to
 * see which OPL the device is prepared for. It is changed in the device's
 * one menu (`DeviceLines`).
 */
export function LoaderProfileLine({ loader, className }: { loader: ActiveProfile; className?: string }) {
  const { t } = useTranslation();
  const { chosen } = useProfileChoice(loader);
  const reason = useReason(loader, chosen);
  return (
    <div className={className} data-testid="loader-profile" data-source={loader.source}>
      <span className="text-muted-foreground">{t("loader.title")}: </span>
      <span className="font-medium" data-testid="loader-profile-name">
        {t(`settings.profiles.${loader.profile}`)}
      </span>
      <span className="text-muted-foreground" data-testid="loader-profile-reason">
        {" "}
        — {reason}
      </span>
    </div>
  );
}

/**
 * What the loader line cannot say in a few words (ADR-0041, ADR-0045):
 * - a device with no sign of its loader asks which OPL it is for, when the
 *   core says so (`askProfile`): prepared for OPL (OPL starts from the
 *   memory card, the console has not listed the device yet), or a folder
 *   read over SMB, where the console leaves no other sign. An empty or
 *   foreign device plugged into the PS2 does not: it gets OPL installed
 *   first, which settles the question;
 * - a profile chosen for the device (or "I don't know") that its files now
 *   contradict offers to go back to auto.
 *
 * Neither shows on a device the storage doctor finds unusable for OPL:
 * which OPL reads it does not matter there.
 */
export function LoaderProfileHint({ device, className }: { device: DeviceInfo; className?: string }) {
  const { t } = useTranslation();
  const loader = device.loader;
  const { choose } = useProfileChoice(loader);
  const detected = loader.detected;
  const box = cn("flex flex-col gap-2 rounded-md border bg-card p-3 text-sm", className);
  if (worstLevel(device.advice) === "error") return null;

  if (device.askProfile) {
    const smb = device.mode.mode === "smb";
    return (
      <div className={box} role="group" aria-labelledby={`loader-question-${loader.key}`} data-testid="loader-question">
        <div className="flex items-center gap-2 font-medium" id={`loader-question-${loader.key}`}>
          <CircleHelp className="size-4 text-muted-foreground" aria-hidden />
          {t(smb ? "loader.questionSmb" : "loader.question")}
        </div>
        <p className="text-muted-foreground">{t(smb ? "loader.questionHintSmb" : "loader.questionHint")}</p>
        <div className="flex flex-wrap gap-2">
          {profiles.map((profile) => (
            <Button key={profile} size="sm" variant="outline" onClick={() => choose(profile)} data-testid={`loader-answer-${profile}`}>
              {t(`loader.answers.${profile}`)}
            </Button>
          ))}
          <Button size="sm" variant="ghost" onClick={() => choose("unknown")} data-testid="loader-answer-unknown">
            {t("loader.unknown")}
          </Button>
        </div>
      </div>
    );
  }

  if (
    loader.source === "override" &&
    detected &&
    detected.confidence !== "default" &&
    detected.profile !== loader.profile
  ) {
    return (
      <div className={cn(box, "flex-row flex-wrap items-center")} data-testid="loader-conflict">
        <span className="min-w-0 flex-1">
          {t("loader.filesSay", {
            profile: t(`settings.profiles.${detected.profile}`),
            reason: phraseText(detected.reason, detected.reasonText),
          })}
        </span>
        <Button size="sm" variant="outline" onClick={() => choose("auto")} data-testid="loader-back-to-auto">
          {t("loader.backToAuto")}
        </Button>
      </div>
    );
  }
  return null;
}
