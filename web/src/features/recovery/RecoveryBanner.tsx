import { History } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { useDevice } from "@/features/recovery/useDevice";

/**
 * A journal on the device means an earlier operation did not finish
 * (ADR-0021). Nothing else may be written until it is undone or finished.
 */
export function RecoveryBanner({ root }: { root: string }) {
  const { t } = useTranslation();
  const device = useDevice(root);
  const flow = usePlan();
  const unfinished = device.data?.unfinished ?? [];
  // A resumed copy is checked as OPL reads the device (ADR-0047).
  const mode = device.data ? { mode: device.data.mode.mode } : {};
  if (unfinished.length === 0) return null;

  return (
    <>
      <Alert variant="destructive" data-testid="recovery-banner">
        <History />
        <AlertTitle>{t("recovery.title", { count: unfinished.length })}</AlertTitle>
        <AlertDescription>
          <p>{t("recovery.description")}</p>
          <ul className="font-mono text-xs">
            {unfinished.map((id) => (
              <li key={id}>{id}</li>
            ))}
          </ul>
          <div className="mt-2 flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={() => void flow.request({ type: "PlanRecovery", root, action: "rollback", ...mode })}
            >
              {t("recovery.rollback")}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => void flow.request({ type: "PlanRecovery", root, action: "resume", ...mode })}
            >
              {t("recovery.resume")}
            </Button>
          </div>
        </AlertDescription>
      </Alert>
      <PlanDialog flow={flow} />
    </>
  );
}
