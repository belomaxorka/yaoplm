import type { TFunction } from "i18next";

import type { JobInfo } from "@/api/generated/JobInfo";

/** Plans carry the core's own title; the other kinds have a fixed one. */
export function jobTitle(t: TFunction, job: JobInfo): string {
  switch (job.kind) {
    case "scanLibrary":
      return t("jobs.scanTitle");
    case "fetchArt":
      return t("jobs.fetchTitle");
    case "verifyGames":
      return t("jobs.verifyTitle");
    case "updateRedump":
      return t("jobs.redumpTitle");
    case "executePlan":
      return job.title;
  }
}
