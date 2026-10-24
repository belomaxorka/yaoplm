import "@/i18n";

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { JobLogEntry } from "@/api/generated/JobLogEntry";
import type { PlanView } from "@/api/generated/PlanView";
import { JobLog } from "@/features/jobs/JobsPage";
import { PlanDetails } from "@/features/plan/PlanDetails";
import i18n from "@/i18n";
import { SettingsContext } from "@/settings/context";
import { defaultSettings } from "@/settings/settings";

/** Writing a picture: the backup folders around it, then the picture. */
const plan: PlanView = {
  id: "p1",
  kind: "art",
  title: "Write 1 picture",
  root: "D:\\stick",
  writeBytes: 36_000,
  deleteBytes: 0,
  freeBytes: 1 << 30,
  executable: true,
  deletesUserData: false,
  transactions: [
    {
      title: "Prepare folders",
      game: null,
      steps: [{ op: "createDir", path: ".yaoplm/backup", to: null, bytes: null, removal: null, source: null }],
      commit: 1,
      notices: [],
      cfgChanges: [],
    },
    {
      title: "Grand Theft Auto III: screenshot",
      game: { title: "Grand Theft Auto III", startup: "SLUS_200.62", media: "CD", format: "ISO" },
      steps: [{ op: "writeFile", path: "ART/SLUS_200.62_SCR.png", to: null, bytes: 36_000, removal: null, source: "cache" }],
      commit: 1,
      notices: [],
      cfgChanges: [],
    },
  ],
  skipped: [],
  notices: [],
};

const log: JobLogEntry[] = [{ kind: "transaction", at: 0, index: 0, title: "Copy the game", state: "done" }];

function show(simpleMode: boolean, children: React.ReactNode) {
  return render(
    <SettingsContext value={{ settings: { ...defaultSettings, simpleMode }, update: () => {} }}>{children}</SettingsContext>,
  );
}

describe("plans and logs keep their details out of the way", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("fold the steps of each operation away in advanced mode", () => {
    const { container } = show(false, <PlanDetails plan={plan} />);
    expect(screen.getByText("2 operations")).toBeTruthy();
    const details = container.querySelectorAll("details[data-testid=plan-steps]");
    expect(details).toHaveLength(2);
    for (const d of details) expect((d as HTMLDetailsElement).open).toBe(false);
    expect(screen.getAllByText("Steps: 1")).toHaveLength(2);
  });

  it("show only the games and the summary in simple mode", () => {
    const { container } = show(true, <PlanDetails plan={plan} />);
    expect(screen.getByTestId("plan-summary")).toBeTruthy();
    expect(screen.getByText("1 operation")).toBeTruthy();
    expect(screen.getByText("Grand Theft Auto III: screenshot")).toBeTruthy();
    expect(screen.queryByText("Prepare folders")).toBeNull();
    expect(container.querySelector("[data-testid=plan-steps]")).toBeNull();
  });

  it("fold a job log away, and leave it out in simple mode", () => {
    const advanced = show(false, <JobLog entries={log} />);
    const details = advanced.container.querySelector("details[data-testid=job-log-details]") as HTMLDetailsElement;
    expect(details.open).toBe(false);
    advanced.unmount();
    const own = show(false, <JobLog entries={log} collapsible={false} />);
    expect(own.container.querySelector("details")).toBeNull();
    expect(own.container.querySelector("[data-testid=job-log]")).toBeTruthy();
    own.unmount();
    const simple = show(true, <JobLog entries={log} />);
    expect(simple.container.innerHTML).toBe("");
  });
});
