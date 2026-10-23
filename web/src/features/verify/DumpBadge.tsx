import { cn } from "cn";
import { ShieldAlert, ShieldCheck, ShieldQuestionMark, ShieldX } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { UnknownReason } from "@/api/generated/UnknownReason";
import type { VerifyStatus } from "@/api/generated/VerifyStatus";
import { Badge } from "@/components/ui/badge";
import { statusColours } from "@/features/verify/dump";

const icons = {
  verified: ShieldCheck,
  badDump: ShieldAlert,
  unknown: ShieldQuestionMark,
  failed: ShieldX,
} as const;

export function DumpIcon({ status, className }: { status: VerifyStatus; className?: string }) {
  const Icon = icons[status];
  return <Icon className={cn("size-4 shrink-0", statusColours[status], className)} aria-hidden />;
}

/** The Redump verdict of a game (GAME-5); the reason says why it is unknown. */
export function DumpBadge({
  status,
  reason,
  title,
  byTrack = false,
}: {
  status: VerifyStatus;
  reason?: UnknownReason | null;
  /** Extra text for the tooltip, e.g. the Redump name. */
  title?: string | null;
  /** The verdict compares the raw CD track the game was added from. */
  byTrack?: boolean;
}) {
  const { t } = useTranslation();
  const label = t(`dump.status.${status}`);
  const why = reason ? t(`dump.reason.${reason}`) : null;
  const how = byTrack ? t("dump.byTrack") : null;
  return (
    <Badge
      variant="outline"
      className={cn("gap-1", statusColours[status])}
      title={[why, title, how].filter(Boolean).join(" · ") || undefined}
      data-testid="dump-badge"
      data-status={status}
    >
      <DumpIcon status={status} className="size-3" />
      {label}
    </Badge>
  );
}
