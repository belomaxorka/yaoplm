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

const ROOT = "E:\\";

function game(overrides: Partial<Game>): Game {
  return {
    title: "Game",
    startup: "SLUS_200.62",
    media: "CD",
    format: "ISO",
    path: "E:\\CD\\SLUS_200.62.Game.iso",
    nameFormat: "old",
    parts: [],
    ulIndex: null,
    size: 700_000_000,
    imageStartup: null,
    layer1Start: null,
    cfg: null,
    art: [],
    cheats: null,
    warnings: [],
    knownTitle: null,
    dump: null,
    fragments: null,
    ...overrides,
  };
}

const games = [
  game({ title: "Zelda-like", startup: "SLUS_200.62" }),
  game({ title: "Alpha Racer", startup: "SCES_500.03", media: "DVD", format: "ZSO", path: "E:\\DVD\\SCES_500.03.Alpha Racer.zso" }),
  game({
    title: "Big Game",
    startup: "SLPM_123.45",
    media: "DVD",
    format: "UL",
    ulIndex: 0,
    path: "E:\\ul.cfg",
    warnings: [{ code: "ulPartMissing", message: "part 2 missing" }],
  }),
];

/** A core with three scanned games and nothing else. */
function core(): MockTransport {
  const scan: JobInfo = {
    id: 1,
    kind: "scanLibrary",
    title: "Scan",
    root: ROOT,
    status: "finished",
    done: 3,
    total: 3,
    rate: null,
    item: null,
    error: null,
    log: [],
    result: { type: "Library", root: ROOT, profile: "Opl120Beta", folders: [], games, apps: [], skipped: [], warnings: [] },
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
      case "InspectDevice":
        return { type: "Device", ...device({ root: ROOT }) };
      default:
        throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
    }
  });
}

function open(path: string, { simpleMode = false, view = "grid" }: { simpleMode?: boolean; view?: "grid" | "list" } = {}) {
  window.localStorage.setItem("yaoplm.settings", JSON.stringify({ root: ROOT, simpleMode, view }));
  window.history.pushState({}, "", path);
  render(<App transport={core()} />);
}

const search = () => window.location.search;

describe("the library toolbar (LIB-5, UI-5)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    window.sessionStorage.clear();
  });

  // One choice carries the direction (the owner's remark of 2026-10-27);
  // size, media and format are no longer sort keys. On main the toolbar had
  // a field select plus a direction button.
  it("sorts with one choice that carries the direction", async () => {
    open("/library?sort=startup&desc=1");
    await screen.findByText("3 of 3 games");
    expect(screen.getByRole("combobox", { name: "Sort by" })).toHaveTextContent("ID Z→A");
    expect(screen.queryByRole("button", { name: /Ascending|Descending/ })).toBeNull();
  });

  it("reads an old sort key kept in a bookmark as Title A→Z", async () => {
    open("/library?sort=size&desc=1");
    await screen.findByText("3 of 3 games");
    expect(screen.getByRole("combobox", { name: "Sort by" })).toHaveTextContent("Title A→Z");
  });

  // One "Filter" menu with boxes instead of two rows of buttons and two
  // counter buttons (release pass 2.5); the button counts what is ticked.
  it("filters through one menu that counts what is ticked", async () => {
    const user = userEvent.setup();
    open("/library");
    await screen.findByText("3 of 3 games");
    expect(screen.queryByRole("radiogroup", { name: "Media" })).toBeNull();
    expect(screen.queryByRole("radiogroup", { name: "Format" })).toBeNull();
    const button = screen.getByTestId("library-filter");
    expect(button).toHaveAccessibleName("Filter");
    await user.click(button);
    const menu = await screen.findByRole("menu");
    const boxes = within(menu).getAllByRole("menuitemcheckbox").map((item) => item.textContent?.trim());
    expect(boxes).toEqual(["CD", "DVD", "ISO", "ZSO", "UL", "1 with warnings", "3 without cover"]);
    await user.click(within(menu).getByRole("menuitemcheckbox", { name: "DVD" }));
    expect(search()).toBe("?media=DVD");
    await waitFor(() => expect(screen.getByTestId("library-count")).toHaveTextContent("2 of 3 games"));
    // The menu stays open for the next box.
    await user.click(within(screen.getByRole("menu")).getByRole("menuitemcheckbox", { name: "UL" }));
    expect(search()).toBe("?media=DVD&format=UL");
    await waitFor(() => expect(screen.getByTestId("library-count")).toHaveTextContent("1 of 3 games"));
    expect(screen.getByTestId("library-filter")).toHaveAccessibleName("Filter, 2 on");
    expect(screen.getByTestId("library-filter-count")).toHaveTextContent("2");
    await user.click(within(screen.getByRole("menu")).getByRole("menuitemcheckbox", { name: "1 with warnings" }));
    expect(search()).toBe("?media=DVD&format=UL&warnings=1");
    expect(screen.getByTestId("library-filter-count")).toHaveTextContent("3");
  });

  it("keeps the simple mode's menu to the disc type and the format", async () => {
    const user = userEvent.setup();
    open("/library?warnings=1", { simpleMode: true });
    await screen.findByText("3 of 3 games");
    expect(screen.queryByRole("combobox", { name: "Sort by" })).toBeNull();
    // The hidden filter stays in the address but does not apply or count.
    expect(screen.queryByTestId("library-filter-count")).toBeNull();
    await user.click(screen.getByTestId("library-filter"));
    const menu = await screen.findByRole("menu");
    expect(within(menu).getAllByRole("menuitemcheckbox").map((item) => item.textContent?.trim())).toEqual(["CD", "DVD", "ISO", "ZSO", "UL"]);
  });

  // "Download pictures" and "Import pictures" were one action with two
  // sources (release pass 2.5): one button, one choice.
  it("offers the pictures through one button with two paths", async () => {
    const user = userEvent.setup();
    open("/library");
    await screen.findByText("3 of 3 games");
    expect(screen.queryByTestId("download-covers")).toBeNull();
    await user.click(screen.getByTestId("library-more"));
    expect(within(await screen.findByRole("menu")).queryByTestId("import-pictures")).toBeNull();
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Pictures…" }));
    const chooser = await screen.findByTestId("pictures-dialog");
    expect(within(chooser).getByTestId("pictures-from-sources")).toHaveTextContent("From the sources");
    await user.click(within(chooser).getByTestId("pictures-from-set"));
    expect(await screen.findByTestId("art-import-dialog")).toBeInTheDocument();
    expect(screen.queryByTestId("pictures-dialog")).toBeNull();
  });

  it("goes straight to the sources in simple mode", async () => {
    const user = userEvent.setup();
    open("/library", { simpleMode: true });
    await screen.findByText("3 of 3 games");
    await user.click(screen.getByRole("button", { name: "Pictures…" }));
    expect(await screen.findByTestId("art-fetch-dialog")).toBeInTheDocument();
    expect(screen.queryByTestId("pictures-dialog")).toBeNull();
  });

  // Walkthrough finding 11 (docs/plans/m5-walkthrough-findings.md): the
  // list's column headers did not sort.
  it("sorts by the list's column headers: a click picks the field, a second one turns it", async () => {
    const user = userEvent.setup();
    open("/library", { view: "list" });
    await screen.findByText("3 of 3 games");
    expect(screen.getByTestId("sort-header-title")).toHaveAttribute("data-sort", "ascending");
    expect(screen.getByTestId("sort-header-startup")).not.toHaveAttribute("data-sort");
    await user.click(screen.getByRole("button", { name: "Sort by Game ID" }));
    expect(search()).toBe("?sort=startup");
    expect(screen.getByTestId("sort-header-startup")).toHaveAttribute("data-sort", "ascending");
    await user.click(screen.getByRole("button", { name: "Sorted by Game ID, ascending; click for descending" }));
    expect(search()).toBe("?sort=startup&desc=1");
    expect(screen.getByTestId("sort-header-startup")).toHaveAttribute("data-sort", "descending");
    expect(screen.getByRole("combobox", { name: "Sort by" })).toHaveTextContent("ID Z→A");
    await user.click(screen.getByRole("button", { name: "Sort by Title" }));
    expect(search()).toBe("");
    // Media, format and size are not sort keys: plain headers, no buttons.
    expect(screen.getAllByRole("button", { name: /^Sort(ed)? by / })).toHaveLength(2);
  });
});
