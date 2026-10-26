import type { TFunction } from "i18next";

import type { JobInfo } from "@/api/generated/JobInfo";
import { errorText, phraseText } from "@/lib/phrase";

/**
 * Why a loader write the wizard waited for did not put the loader on the
 * device, or `null` when it did. A transaction that fails still ends the job
 * as finished, with the error in its report: the status alone is not enough.
 */
export function writeProblem(info: JobInfo, t: TFunction): string | null {
  const report = info.result?.type === "Execution" ? info.result : null;
  if (info.status === "cancelled" || report?.cancelled) return t("loaderInstall.cancelled");
  if (info.error) return errorText(info.error);
  if (report?.error) return phraseText(report.errorPhrase, report.error);
  if (report?.journal) return t("jobs.toast.journal");
  return info.status === "finished" ? null : t("loaderInstall.notFinished");
}
