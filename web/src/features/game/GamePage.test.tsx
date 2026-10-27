import "@/i18n";

import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { Game } from "@/api/generated/Game";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { App } from "@/App";
import { device } from "@/features/loaders/fixtures";
import i18n from "@/i18n";
import { gameKey } from "@/lib/gameKey";

const ROOT = "E:\\";

/** A file named after one ID whose SYSTEM.CNF says another (the fixture's "Wrong Name"). */
const game: Game = {
  title: "Wrong Name",
  startup: "SLES_999.99",
  media: "CD",
  format: "ISO",
  path: "E:\\CD\\SLES_999.99.Wrong Name.iso",
  nameFormat: "old",
  parts: [],
  ulIndex: null,
  size: 700_000_000,
  imageStartup: "SLUS_200.62",
  layer1Start: null,
  cfg: null,
  art: [],
  cheats: null,
  warnings: [],
  knownTitle: null,
  dump: null,
  fragments: null,
};

function core() {
  const requests: Request[] = [];
  const scan: JobInfo = {
    id: 1,
    kind: "scanLibrary",
    title: "Scan",
    root: ROOT,
    status: "finished",
    done: 1,
    total: 1,
    rate: null,
    item: null,
    error: null,
    log: [],
    result: { type: "Library", root: ROOT, profile: "Opl120Beta", folders: [], games: [game], apps: [], skipped: [], warnings: [] },
  };
  const transport = new MockTransport((request: Request): Response => {
    requests.push(request);
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "ScanLibrary":
        return { type: "Job", job: 1 };
      case "GetJob":
        return { type: "JobState", ...scan };
      case "Version":
        return { type: "Version", name: "YAOPLM", version: "0.0.0-mock" };
      case "InspectDevice":
        return { type: "Device", ...device({ root: ROOT }) };
      case "ListVmc":
        return { type: "Vmcs", cards: [] };
      default:
        throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
    }
  });
  const of = <T extends Request["type"]>(type: T) => requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
  return { transport, of };
}

function open(simpleMode: boolean) {
  window.localStorage.setItem("yaoplm.settings", JSON.stringify({ root: ROOT, simpleMode }));
  window.history.pushState({}, "", `/library/game/${encodeURIComponent(gameKey(game))}`);
  const c = core();
  render(<App transport={c.transport} />);
  return c;
}

describe("the rename dialog and the ID the image says (walkthrough finding 9)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  // On main the card warned that the file's ID and SYSTEM.CNF's differ, but
  // the dialog made the user type the ID by hand.
  it("offers the image's ID and puts it into the ID field", async () => {
    const user = userEvent.setup();
    const c = open(false);
    await screen.findByTestId("game-title");
    await user.click(screen.getByRole("button", { name: "Rename" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename game" });
    expect(within(dialog).getByText("The image says SLUS_200.62")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Use it" }));
    expect(within(dialog).getByLabelText("Game ID")).toHaveValue("SLUS_200.62");
    await user.click(within(dialog).getByRole("button", { name: "Preview changes" }));
    await waitFor(() => expect(c.of("RenameGame")).toHaveLength(1));
    expect(c.of("RenameGame")[0]).toMatchObject({ title: "Wrong Name", startup: "SLUS_200.62" });
  });

  it("offers it in simple mode too, where the ID field is hidden", async () => {
    const user = userEvent.setup();
    const c = open(true);
    await screen.findByTestId("game-title");
    await user.click(screen.getByRole("button", { name: "Rename" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename game" });
    expect(within(dialog).queryByLabelText("Game ID")).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: "Use it" }));
    await user.click(within(dialog).getByRole("button", { name: "Preview changes" }));
    await waitFor(() => expect(c.of("RenameGame")).toHaveLength(1));
    expect(c.of("RenameGame")[0]).toMatchObject({ startup: "SLUS_200.62" });
  });
});
