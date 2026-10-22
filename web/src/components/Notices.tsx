import { cn } from "cn";
import { AlertTriangle, Info, OctagonX } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Notice } from "@/api/generated/Notice";
import type { NoticeLevel } from "@/api/generated/NoticeLevel";
import type { PlanNotice } from "@/api/generated/PlanNotice";
import { useNoticeText } from "@/lib/notices";

const icons = {
  info: Info,
  warning: AlertTriangle,
  error: OctagonX,
} as const;

const colours = {
  info: "text-muted-foreground",
  warning: "text-amber-600 dark:text-amber-400",
  error: "text-destructive",
} as const;

export function NoticeIcon({ level, className }: { level: NoticeLevel; className?: string }) {
  const Icon = icons[level];
  return <Icon className={cn("size-4 shrink-0", colours[level], className)} aria-hidden />;
}

export function NoticeLine({
  notice,
  level = "warning",
  compact = false,
}: {
  notice: Notice;
  level?: NoticeLevel;
  compact?: boolean;
}) {
  const text = useNoticeText();
  const { t } = useTranslation();
  const { label, detail } = text(notice);
  return (
    <li className="flex items-start gap-2 text-sm" data-code={notice.code} data-level={level}>
      <NoticeIcon level={level} className="mt-0.5" />
      <span className="min-w-0">
        <span className="sr-only">{t(`noticeLevel.${level}`)}: </span>
        <span>{label}</span>
        {detail && !compact && <span className="block text-xs text-muted-foreground">{detail}</span>}
      </span>
    </li>
  );
}

export function NoticeList({
  notices,
  level,
  className,
  compact,
}: {
  notices: readonly (Notice | PlanNotice)[];
  /** For plain warnings without a level of their own. */
  level?: NoticeLevel;
  className?: string;
  compact?: boolean;
}) {
  if (notices.length === 0) return null;
  return (
    <ul className={cn("flex flex-col gap-1.5", className)}>
      {notices.map((notice, index) => (
        <NoticeLine
          key={`${notice.code}-${index}`}
          notice={notice}
          level={"level" in notice ? notice.level : (level ?? "warning")}
          compact={compact}
        />
      ))}
    </ul>
  );
}
