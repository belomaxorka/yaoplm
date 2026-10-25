import type { JobInfo } from "@/api/generated/JobInfo";

/**
 * A write job that did all its plan said. A transaction that fails still
 * ends the job as finished, with the error in its report: the status alone
 * does not tell.
 */
export function wroteAll(info: JobInfo): boolean {
  const report = info.result?.type === "Execution" ? info.result : null;
  return info.status === "finished" && !info.error && !report?.error && !report?.cancelled && !report?.journal;
}
