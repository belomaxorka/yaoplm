import { ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ActiveProfile } from "@/api/generated/ActiveProfile";
import type { LoaderProfile } from "@/api/generated/LoaderProfile";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { phraseText } from "@/lib/phrase";
import { useSettings } from "@/settings/context";
import { profiles } from "@/settings/settings";

/** Why the profile is in force, to follow it after a dash. */
function useReason(loader: ActiveProfile): string {
  const { t } = useTranslation();
  if (loader.source === "override") return t("loader.byOverride");
  // Only the CLI forces a profile for every device (ADR-0040).
  if (loader.source === "explicit") return t("loader.byExplicit");
  if (!loader.detected) return t("loader.unreadable");
  return phraseText(loader.detected.reason, loader.detected.reasonText);
}

/**
 * "Loader: OPL 1.2.0 beta — why", with a menu to choose the profile for
 * this device or go back to auto. Shown in both modes: a beginner needs to
 * see which OPL the device is prepared for.
 */
export function LoaderProfileLine({ loader, className }: { loader: ActiveProfile; className?: string }) {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  const reason = useReason(loader);
  const chosen = settings.profileOverrides[loader.key] ?? "auto";
  const choose = (value: string) => {
    const { [loader.key]: _old, ...others } = settings.profileOverrides;
    update({ profileOverrides: value === "auto" ? others : { ...others, [loader.key]: value as LoaderProfile } });
  };
  return (
    <div className={className} data-testid="loader-profile" data-source={loader.source}>
      <span className="text-muted-foreground">{t("loader.title")}: </span>
      <span className="font-medium" data-testid="loader-profile-name">
        {t(`settings.profiles.${loader.profile}`)}
      </span>
      <span className="text-muted-foreground" data-testid="loader-profile-reason">
        {" "}
        — {reason}
      </span>{" "}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="link" size="sm" className="h-auto p-0 align-baseline text-xs" data-testid="loader-profile-change">
            {t("loader.change")}
            <ChevronDown aria-hidden />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-max">
          <DropdownMenuLabel>{t("loader.forThisDevice")}</DropdownMenuLabel>
          <DropdownMenuSeparator />
          <DropdownMenuRadioGroup value={chosen} onValueChange={choose}>
            <DropdownMenuRadioItem value="auto" data-testid="loader-profile-auto">
              {t("loader.auto")}
            </DropdownMenuRadioItem>
            {profiles.map((profile) => (
              <DropdownMenuRadioItem key={profile} value={profile} data-testid={`loader-profile-${profile}`}>
                {t(`settings.profiles.${profile}`)}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
