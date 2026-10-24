import { Loader2 } from "lucide-react";
import { type ReactNode, useId, useState } from "react";
import { useTranslation } from "react-i18next";

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
import { PlanDetails } from "@/features/plan/PlanDetails";
import type { usePlan } from "@/features/plan/usePlan";
import { phraseText } from "@/lib/phrase";

type PlanFlow = ReturnType<typeof usePlan>;

/**
 * The confirmation screen for every write (IMP-10). Deleting the user's
 * files needs its own checkbox on top of the Confirm button (GAME-2).
 * `choice` sits beside the buttons: a choice that plans again, such as
 * replacing a file the plan stopped at (LDR-3).
 */
export function PlanDialog({
  flow,
  description,
  choice,
}: {
  flow: PlanFlow;
  description?: string;
  choice?: ReactNode;
}) {
  const { t } = useTranslation();
  // The confirmation belongs to one plan: the next one asks again.
  const [confirmedFor, setConfirmedFor] = useState<string | null>(null);
  const checkboxId = useId();
  const open = flow.plan !== null || flow.pending || flow.error !== null;
  const plan = flow.plan;
  const confirmDelete = plan !== null && confirmedFor === plan.id;
  const canRun = plan !== null && plan.executable && (!plan.deletesUserData || confirmDelete) && !flow.executing;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !flow.executing) flow.discard();
      }}
    >
      <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-2xl" data-testid="plan-dialog">
        <DialogHeader>
          <DialogTitle>{plan ? phraseText(plan.titlePhrase, plan.title) : t("plan.title")}</DialogTitle>
          <DialogDescription>{description ?? (plan ? t("plan.on", { root: plan.root }) : t("plan.building"))}</DialogDescription>
        </DialogHeader>
        <div className="relative min-h-0 flex-1 overflow-auto pr-1">
          {flow.pending && (
            <div className="flex items-center gap-2 py-8 text-sm text-muted-foreground" role="status">
              <Loader2 className="size-4 animate-spin" aria-hidden />
              {t("plan.building")}
            </div>
          )}
          {flow.error && (
            <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
              {flow.error}
            </p>
          )}
          {plan && <PlanDetails plan={plan} />}
        </div>
        {plan && (
          <DialogFooter className="items-center gap-3 sm:justify-between">
            {plan.deletesUserData ? (
              <Label htmlFor={checkboxId} className="text-destructive">
                <Checkbox
                  id={checkboxId}
                  checked={confirmDelete}
                  onCheckedChange={(value) => setConfirmedFor(value === true ? plan.id : null)}
                />
                {t("plan.confirmDelete")}
              </Label>
            ) : (
              (choice ?? <span />)
            )}
            <div className="flex gap-2">
              <Button variant="outline" onClick={flow.discard} disabled={flow.executing}>
                {t("common.cancel")}
              </Button>
              <Button
                variant={plan.deletesUserData ? "destructive" : "default"}
                disabled={!canRun}
                onClick={() => void flow.execute(confirmDelete)}
                data-testid="plan-confirm"
              >
                {flow.executing && <Loader2 className="animate-spin" aria-hidden />}
                {plan.deletesUserData ? t("plan.confirmAndDelete") : t("plan.confirm")}
              </Button>
            </div>
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  );
}
