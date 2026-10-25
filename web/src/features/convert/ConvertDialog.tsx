import { Replace } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

import type { ConvertTarget } from "@/api/generated/ConvertTarget";
import type { Game } from "@/api/generated/Game";
import type { Request } from "@/api/generated/Request";
import { Button } from "@/components/ui/button";
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
import { convertTargets, formatOf, suggestTarget } from "@/features/convert/convert";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { gameRef } from "@/lib/gameKey";
import { useSettings } from "@/settings/context";
import { useLoaderProfile } from "@/features/devices/useLoaderProfile";

/**
 * Convert games between ISO, ZSO and `ul` (GAME-4). The core plans it; the
 * old image is deleted only once the new one is complete, and the plan asks
 * for that deletion to be confirmed.
 */
export function ConvertDialog({
  root,
  games,
  open,
  onOpenChange,
  onExecuted,
}: {
  root: string;
  /** One game (the game page) or the games shown in the library. */
  games: readonly Game[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onExecuted?: () => void;
}) {
  const { t } = useTranslation();
  const flow = usePlan({
    onExecuted: () => {
      onOpenChange(false);
      onExecuted?.();
    },
  });
  const single = games.length === 1 ? games[0] : undefined;

  return (
    <>
      <Dialog open={open && flow.plan === null && !flow.pending && flow.error === null} onOpenChange={onOpenChange}>
        <DialogContent data-testid="convert-dialog">
          <DialogHeader>
            <DialogTitle>
              {single ? t("convert.titleOne", { title: single.title }) : t("convert.titleMany", { count: games.length })}
            </DialogTitle>
            <DialogDescription>{t("convert.description")}</DialogDescription>
          </DialogHeader>
          {/* Mounted only while open, so every opening starts from the suggestion. */}
          <ConvertBody
            root={root}
            games={games}
            onCancel={() => onOpenChange(false)}
            onPreview={(request) => void flow.request(request)}
          />
        </DialogContent>
      </Dialog>
      <PlanDialog flow={flow} />
    </>
  );
}

function ConvertBody({
  root,
  games,
  onCancel,
  onPreview,
}: {
  root: string;
  games: readonly Game[];
  onCancel: () => void;
  onPreview: (request: Extract<Request, { type: "ConvertGames" }>) => void;
}) {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const { profile: loaderProfile, mode } = useLoaderProfile();
  const ids = useId();
  const zsoReadable = loaderProfile !== "Opl110";
  const [target, setTarget] = useState<ConvertTarget>(() => suggestTarget(games, zsoReadable));
  const single = games.length === 1 ? games[0] : undefined;
  const changing = games.filter((game) => game.format !== formatOf[target]);

  return (
    <>
      <fieldset>
        <legend className="mb-2 text-sm font-medium">{t("convert.target")}</legend>
        <RadioGroup value={target} onValueChange={(value) => setTarget(value as ConvertTarget)}>
          {convertTargets.map((option) => {
            const disabled = (option === "zso" && !zsoReadable) || (single !== undefined && single.format === formatOf[option]);
            return (
              <Label key={option} htmlFor={`${ids}-${option}`} className="items-start font-normal">
                <RadioGroupItem
                  id={`${ids}-${option}`}
                  value={option}
                  disabled={disabled}
                  className="mt-0.5"
                  data-testid={`convert-target-${option}`}
                />
                <span className={disabled ? "text-muted-foreground" : undefined}>
                  {t(`convert.targets.${option}`)}
                  <span className="block text-xs text-muted-foreground">
                    {option === "zso" && !zsoReadable ? t("convert.zsoNotForProfile") : t(`convert.hints.${option}`)}
                  </span>
                </span>
              </Label>
            );
          })}
        </RadioGroup>
      </fieldset>
      {!single && (
        <p className="text-sm" data-testid="convert-count">
          {t("convert.changing", { count: changing.length, total: games.length })}
        </p>
      )}
      <p className="text-xs text-muted-foreground">{t("convert.deleteNote")}</p>
      <DialogFooter>
        <Button variant="outline" onClick={onCancel}>
          {t("common.cancel")}
        </Button>
        <Button
          disabled={changing.length === 0}
          onClick={() =>
            onPreview({
              type: "ConvertGames",
              root,
              games: changing.map(gameRef),
              target,
              profile: loaderProfile,
              nameStyle: settings.nameStyle,
              ...(mode ? { mode } : {}),
            })
          }
          data-testid="convert-preview"
        >
          <Replace aria-hidden />
          {t("convert.preview")}
        </Button>
      </DialogFooter>
    </>
  );
}
