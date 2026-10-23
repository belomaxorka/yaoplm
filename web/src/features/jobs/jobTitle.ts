import type { JobInfo } from "@/api/generated/JobInfo";
import { phraseText } from "@/lib/phrase";

/** The job's title in the UI's language: the core sends it as a phrase. */
export function jobTitle(job: JobInfo): string {
  return phraseText(job.titlePhrase, job.title);
}
