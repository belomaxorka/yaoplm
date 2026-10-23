import "@/i18n";

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { Game } from "@/api/generated/Game";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { App } from "@/App";
import i18n from "@/i18n";
import { gameKey } from "@/lib/gameKey";

const ROOT = "E:\\";

const game: Game = {
  title: "Retail Game",
  startup: "SLUS_200.62",
  media: "CD",
  format: "ISO",
  path: "E:\\CD\\SLUS_200.62.Retail Game.iso",
  nameFormat: "old",
  parts: [],
  ulIndex: null,
  size: 700_000_000,
  imageStartup: null,
  layer1Start: null,
  cfg: { path: "CFG/SLUS_200.62.cfg", oplSyntax: true, compatibility: 0, vmc: [null, null] },
  art: [],
  cheats: null,
  warnings: [{ code: "test", message: "A warning" }],
  knownTitle: null,
  dump: null,
};

/** A core with one scanned game; anything else is not there. */
function core(): MockTransport {
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
  return new MockTransport((request: Request): Response => {
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "ScanLibrary":
        return { type: "Job", job: 1 };
      case "GetJob":
        return { type: "JobState", ...scan };
      case "Version":
        return { type: "Version", name: "YAOPLM", version: "0.0.0-mock" };
      default:
        throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
    }
  });
}

function open(path: string, simpleMode: boolean) {
  window.localStorage.setItem("yaoplm.settings", JSON.stringify({ root: ROOT, simpleMode }));
  window.history.pushState({}, "", path);
  render(<App transport={core()} />);
}

describe("simple mode (UI-4)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("is on by default and comes first in the settings", async () => {
    window.history.pushState({}, "", "/settings");
    render(<App transport={core()} />);
    const switches = await screen.findAllByRole("switch");
    expect(switches[0]).toHaveAccessibleName("Simple mode");
    expect(switches[0]).toBeChecked();
    expect(screen.queryByTestId("art-source-xlenore")).not.toBeInTheDocument();
    expect(screen.queryByText("Check IDs inside images while scanning")).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Jobs" })).not.toBeInTheDocument();

    await userEvent.click(switches[0]!);
    expect(screen.getByTestId("art-source-xlenore")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Jobs" })).toBeInTheDocument();
  });

  for (const simple of [true, false]) {
    it(`library: ${simple ? "search, media and format only" : "every tool"}`, async () => {
      open("/library?warnings=1", simple);
      // jsdom lays nothing out, so the virtual grid shows no cards: the count says the game is there.
      await screen.findByText("1 of 1 games");
      expect(screen.getByRole("searchbox", { name: "Search" })).toBeInTheDocument();
      expect(screen.getByRole("radiogroup", { name: "Media" })).toBeInTheDocument();
      expect(screen.getByRole("radiogroup", { name: "Format" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Rescan" })).toBeInTheDocument();
      expect(screen.getByTestId("download-covers")).toBeInTheDocument();
      expect(screen.getByRole("link", { name: "Add games" })).toBeInTheDocument();
      const advanced = [
        screen.queryByRole("button", { name: "Only games with warnings" }),
        screen.queryByTestId("filter-no-cover"),
        screen.queryByRole("combobox", { name: "Sort by" }),
        screen.queryByRole("radiogroup", { name: "View" }),
        screen.queryByTestId("library-more"),
      ];
      for (const control of advanced) {
        if (simple) expect(control).not.toBeInTheDocument();
        else expect(control).toBeInTheDocument();
      }
      // The hidden filter stays in the address but does not apply.
      expect(window.location.search).toBe("?warnings=1");
      expect(screen.getByTestId("library-count")).toHaveTextContent("1 of 1 games");
    });

    it(`game card: ${simple ? "no conversion, dump check or technical details" : "everything"}`, async () => {
      open(`/library/game/${encodeURIComponent(gameKey(game))}`, simple);
      await screen.findByTestId("game-title");
      const actions = screen.getByRole("group", { name: "Actions" });
      expect(within(actions).getByRole("button", { name: "Rename" })).toBeInTheDocument();
      expect(within(actions).getByRole("button", { name: "Move to DVD" })).toBeInTheDocument();
      expect(within(actions).getByRole("button", { name: "Delete" })).toBeInTheDocument();
      expect(screen.getByTestId("game-download-art")).toBeInTheDocument();
      expect(screen.getByTestId("game-warnings")).toBeInTheDocument();
      const advanced = [
        screen.queryByTestId("game-convert"),
        screen.queryByTestId("game-verify"),
        screen.queryByText(game.path),
        screen.queryByText("Game settings (CFG)"),
      ];
      for (const control of advanced) {
        if (simple) expect(control).not.toBeInTheDocument();
        else expect(control).toBeInTheDocument();
      }
    });
  }
});
