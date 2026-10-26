import { useTranslation } from "react-i18next";

import type { Notice } from "@/api/generated/Notice";
import type { NoticeLevel } from "@/api/generated/NoticeLevel";
import type { PlanNotice } from "@/api/generated/PlanNotice";
import { phraseText } from "@/lib/phrase";
import { useSimpleMode } from "@/settings/context";

/**
 * Plan notices, game warnings, device advice and skip reasons come as a
 * phrase with their names and sizes (ADR-0029): the UI shows the phrase in
 * its language. A notice from an older journal has only the core's English.
 */
export function useNoticeText() {
  // Re-renders the caller when the language changes.
  const { t } = useTranslation();
  const simple = useSimpleMode();
  return (notice: Notice | PlanNotice): string => {
    // Where a picture comes from: simple mode names the source, not the
    // mirror's path or the URL after it.
    const source = notice.phrase?.params.source;
    if (simple && notice.code === "artFrom" && typeof source === "string") return t("plan.artFrom", { source });
    return phraseText(notice.phrase, notice.message);
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
