import "@/i18n";

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { JobInfo } from "@/api/generated/JobInfo";
import { JobProgressLine } from "@/features/jobs/JobsPage";
import i18n from "@/i18n";

const job = (status: JobInfo["status"]): JobInfo => ({
  id: 8,
  kind: "executePlan",
  title: "Import 8 games",
  root: "D:\\stick",
  status,
  done: 4_400_000,
  total: status === "running" ? 8_800_000 : 4_400_000,
  rate: 855_000,
  item: "Shadow of the Colossus: spine",
  error: null,
  log: [],
});

describe("the progress line of a job", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("names the item and the speed while the job runs", () => {
    render(<JobProgressLine job={job("running")} />);
    expect(screen.getByText("Shadow of the Colossus: spine")).toBeTruthy();
    expect(screen.getByText(/\/s$/)).toBeTruthy();
  });

  // The walkthrough of 2026-10-24: a finished import still read "Shadow of
  // the Colossus: spine", as if that picture were still being written.
  it("drops them once the job has ended", () => {
    for (const status of ["finished", "failed", "cancelled"] as const) {
      const { unmount } = render(<JobProgressLine job={job(status)} />);
      expect(screen.queryByText("Shadow of the Colossus: spine")).toBeNull();
      expect(screen.queryByText(/\/s$/)).toBeNull();
      expect(screen.getByText(/\(100%\)/)).toBeTruthy();
      unmount();
    }
  });
});
