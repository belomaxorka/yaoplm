import { useTranslation } from "react-i18next";

import type { Notice } from "@/api/generated/Notice";
import type { NoticeLevel } from "@/api/generated/NoticeLevel";
import type { PlanNotice } from "@/api/generated/PlanNotice";

/**
 * Warnings, skip reasons and storage advice all come as a stable `code`
 * plus an English `message`. The code picks a translated label; the
 * message keeps the specifics (names, sizes) the core put in it.
 */
export function useNoticeText() {
  const { t, i18n } = useTranslation();
  return (notice: Notice): { label: string; detail: string | null } => {
    const key = `notices.${notice.code}`;
    if (!i18n.exists(key)) return { label: notice.message, detail: null };
    // Codes are open-ended (the core may add one before the catalogue does),
    // so the key is looked up dynamically.
    const label: string = t(key as never);
    // The core's message repeats a label without specifics only in case.
    const same = notice.message.toLowerCase() === label.toLowerCase();
    return { label, detail: same ? null : notice.message };
  };
}

/** The worst level among notices, for a verdict badge. */
export function worstLevel(notices: readonly PlanNotice[]): NoticeLevel | null {
  let worst: NoticeLevel | null = null;
  for (const notice of notices) {
    if (notice.level === "error") return "error";
    if (notice.level === "warning") worst = "warning";
    else if (worst === null) worst = "info";
  }
  return worst;
}
