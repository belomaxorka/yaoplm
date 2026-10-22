import { cn } from "cn";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";

const dotColor = {
  connecting: "bg-amber-500",
  open: "bg-emerald-500",
  closed: "bg-red-500",
} as const;

export function ConnectionStatus() {
  const { t } = useTranslation();
  const { status } = useApi();

  return (
    <div role="status" data-state={status} className="flex items-center gap-2 text-xs text-muted-foreground">
      <span className={cn("size-2 rounded-full", dotColor[status])} aria-hidden />
      {t(`connection.${status}`)}
    </div>
  );
}
