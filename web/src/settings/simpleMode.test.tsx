import "@/i18n";

import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { Game } from "@/api/generated/Game";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { defaultSettings } from "@/settings/settings";
import { MockTransport, RpcError } from "@/api/transport";
import { App } from "@/App";
import i18n from "@/i18n";
import { gameKey } from "@/lib/gameKey";
import { device } from "@/features/loaders/fixtures";

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
  warnings: [
    { code: "test", message: "A warning" },
    {
      code: "vmcInvalid",
      message: "VMC slot 1: VMC/SLUS_200.62_0.bin is unusable: not a PS2 memory card image",
      phrase: { key: "notices.vmcInvalid", params: { slot: 1, name: "SLUS_200.62_0", reason: { key: "reasons.vmcBadMagic", params: {} } } },
    },
    {
      code: "vmcMissing",
      message: "VMC slot 2: VMC/SLUS_200.62_1.bin does not exist",
      phrase: { key: "notices.vmcMissing", params: { slot: 2, name: "SLUS_200.62_1" } },
    },
  ],
  knownTitle: null,
  dump: null,
  fragments: { kind: "known", total: 1, files: [1], limit: 64 },
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
      case "ReadCfg":
        return {
          type: "Cfg",
          startup: game.startup,
          path: "E:\\CFG\\SLUS_200.62.cfg",
          exists: true,
          editable: true,
          settings: {
            compatibility: 0,
            dma: null,
            altStartup: "",
            vmc: ["", ""],
            gsm: { perGame: false, enabled: false, vmode: 0, xOffset: 0, yOffset: 0, fieldFix: false },
            cheats: { perGame: false, enabled: false, mode: 0 },
            padEmu: { perGame: false, enabled: false, settings: 0 },
          },
          entries: [],
          cardNames: ["SLUS_200.62_0", "SLUS_200.62_1"],
        };
      case "ListVmc":
        return { type: "Vmcs", cards: [] };
      // The open device answers: without it the library shows "not available".
      case "InspectDevice":
        return { type: "Device", ...device({ root: ROOT }) };
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
    // Dump checks on add are off by default (owner, 2026-10-26).
    expect(defaultSettings.checkOnAdd).toBe(false);
    // No keyboard card (owner, 2026-10-26): the keys are the usual ones.
    expect(screen.queryByText("Keyboard")).toBeNull();
    // Dump checks are an advanced tool (ADR-0034): no Redump card in simple mode.
    expect(screen.queryByText("Dump checks (Redump)")).toBeNull();
    // Cards explain nothing under their titles (release pass 2.2).
    expect(screen.queryByText(/The loader profile is read from each device/)).toBeNull();
    expect(screen.queryByText(/Where OPL builds for installing come from/)).toBeNull();

    await userEvent.click(switches[0]!);
    expect(screen.queryByText("Keyboard")).toBeNull();
    expect(screen.getByText("Dump checks (Redump)")).toBeInTheDocument();
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
      expect(screen.getByTestId("download-covers")).toBeInTheDocument();
      expect(screen.getByRole("link", { name: "Add games" })).toBeInTheDocument();
      const advanced = [
        screen.queryByRole("button", { name: /with warnings/ }),
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
      // Counters and rescanning are advanced tools (release pass 2.1): simple
      // mode keeps the count for screen readers only.
      if (simple) expect(screen.getByTestId("library-count")).toHaveClass("sr-only");
      else expect(screen.getByTestId("library-count")).not.toHaveClass("sr-only");
      if (simple) expect(screen.queryByRole("button", { name: "Rescan" })).toBeNull();
      else expect(screen.getByRole("button", { name: "Rescan" })).toBeInTheDocument();
    });

    it(`game card: ${simple ? "modes and slot 1, no conversion, dump check or technical details" : "everything"}`, async () => {
      open(`/library/game/${encodeURIComponent(gameKey(game))}`, simple);
      await screen.findByTestId("game-title");
      const actions = screen.getByRole("group", { name: "Actions" });
      expect(within(actions).getByRole("button", { name: "Rename" })).toBeInTheDocument();
      expect(within(actions).getByRole("button", { name: "Delete" })).toBeInTheDocument();
      // Moving, converting and checking wait in a menu in both modes (owner,
      // 2026-10-26): the card's own actions are renaming and deleting.
      expect(within(actions).queryByRole("button", { name: "Move to DVD" })).toBeNull();
      await userEvent.click(within(actions).getByTestId("game-more"));
      expect(await screen.findByTestId("game-move")).toHaveTextContent("Move to DVD");
      // Converting is an advanced tool (ADR-0034); checking the dump sits by
      // the "Dump" fact, not in the menu (owner, 2026-10-26).
      expect(within(screen.getByRole("menu")).queryByTestId("game-verify")).not.toBeInTheDocument();
      if (simple) expect(screen.queryByTestId("game-convert")).not.toBeInTheDocument();
      else expect(screen.getByTestId("game-convert")).toBeInTheDocument();
      await userEvent.keyboard("{Escape}");
      await waitFor(() => expect(screen.queryByTestId("game-move")).not.toBeInTheDocument());
      if (simple) expect(screen.queryByTestId("game-verify")).not.toBeInTheDocument();
      else expect(within(screen.getByTestId("game-dump").parentElement!).getByTestId("game-verify")).toHaveTextContent("Check dump");
      expect(screen.getByTestId("game-download-art")).toBeInTheDocument();
      // A VMC warning belongs to its slot (release pass 1.10): the list
      // repeats only what no slot on screen shows (slot 2 in simple mode).
      const warnings = screen.getByTestId("game-warnings");
      expect(warnings).toHaveTextContent("A warning");
      expect(warnings).not.toHaveTextContent("VMC slot 1");
      if (simple) expect(warnings).toHaveTextContent("VMC slot 2");
      else expect(warnings).not.toHaveTextContent("VMC slot 2");
      // Game settings (ADR-0036): the modes and slot 1 in both modes.
      expect(await screen.findByTestId("cfg-modes")).toBeInTheDocument();
      expect(screen.getByTestId("cfg-vmc-0")).toBeInTheDocument();
      expect(screen.getByRole("checkbox", { name: /Mode 6/ })).toBeInTheDocument();
      expect(screen.queryByRole("checkbox", { name: /Mode 7/ })).not.toBeInTheDocument();
      const advanced = [
        screen.queryByText(game.path),
        screen.queryByTestId("game-fragments"),
        screen.queryByTestId("cfg-vmc-1"),
        screen.queryByTestId("cfg-gsm"),
        screen.queryByTestId("cfg-cheats"),
        screen.queryByTestId("cfg-pademu"),
        screen.queryByTestId("cfg-alt-startup"),
        screen.queryByTestId("cfg-keys"),
      ];
      for (const control of advanced) {
        if (simple) expect(control).not.toBeInTheDocument();
        else expect(control).toBeInTheDocument();
      }
    });
  }
});
