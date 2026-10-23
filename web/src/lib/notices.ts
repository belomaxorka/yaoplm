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
    // A message without names or sizes in it is the English label: it adds
    // nothing, in English or in a translation.
    const english: string = t(key as never, { lng: "en" });
    const message = plain(notice.message);
    const same = message === plain(english) || message === plain(label);
    return { label, detail: same ? null : notice.message };
  };
}

/** Text without case, punctuation or runs of spaces, for comparing. */
function plain(text: string): string {
  return text
    .toLowerCase()
    .replace(/[.,:;!?’']+/g, "")
    .replace(/\s+/g, " ")
    .trim();
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
