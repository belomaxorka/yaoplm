import "@/i18n";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, renderHook, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { type ReactNode, StrictMode, useState } from "react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "@/api/ApiProvider";
import type { Game } from "@/api/generated/Game";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { JobResult } from "@/api/generated/JobResult";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ArtFetchDialog } from "@/features/art/ArtFetchDialog";
import { ArtImportDialog } from "@/features/art/ArtImportDialog";
import { ArtVariantsDialog } from "@/features/art/ArtVariantsDialog";
import { useFetchableTypes } from "@/features/art/kinds";
import i18n from "@/i18n";
import { JobsProvider } from "@/jobs/JobsProvider";
import { defaultSettings, type Settings } from "@/settings/settings";
import { SettingsProvider } from "@/settings/SettingsProvider";

// Desktop pickers stand in for the server's folder browser.
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  isTauri: () => true,
}));
vi.mock("@/features/add/desktop", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/features/add/desktop")>()),
  pickPictureSets: () => Promise.resolve(["/sets/art.zip"]),
  listenForDrops: () => () => undefined,
}));

const game: Game = {
  title: "Retail Game",
  startup: "SLUS_200.62",
  media: "DVD",
  format: "ISO",
  path: "DVD/SLUS_200.62.Retail Game.iso",
  nameFormat: null,
  parts: [],
  ulIndex: null,
  size: 1024,
  imageStartup: null,
  layer1Start: null,
  cfg: null,
  art: [],
  cheats: null,
  warnings: [],
  knownTitle: null,
  dump: null,
  fragments: null,
};

/** Answers the requests these dialogs make; jobs end with `results[kind]`, or run until cancelled. */
function fakeCore(answers: { results?: Partial<Record<JobInfo["kind"], JobResult>>; attribution?: () => Promise<Response> }) {
  const requests: Request[] = [];
  const jobs = new Map<number, JobInfo>();
  const start = (kind: JobInfo["kind"]): Response => {
    const id = jobs.size + 1;
    const result = answers.results?.[kind];
    jobs.set(id, {
      id,
      kind,
      title: kind,
      root: "/lib",
      status: result ? "finished" : "running",
      done: 0,
      total: null,
      rate: null,
      item: null,
      error: null,
      log: [],
      ...(result ? { result } : {}),
    });
    return { type: "Job", job: id };
  };
  const transport = new MockTransport(async (request) => {
    requests.push(request);
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "DiscardPlan":
        return { type: "Ack" };
      case "FindArt":
      case "FetchArt":
        return start("fetchArt");
      case "ImportArt":
        return start("importArt");
      case "GetJob": {
        const info = jobs.get(request.job);
        if (!info) throw new RpcError({ code: "NotFound", message: `no job ${request.job}` });
        return { type: "JobState", ...info };
      }
      case "CancelJob": {
        const info = jobs.get(request.job);
        if (info?.status === "running") jobs.set(request.job, { ...info, status: "cancelled" });
        return { type: "Ack" };
      }
      case "Attribution":
        if (answers.attribution) return answers.attribution();
        break;
      default:
        break;
    }
    throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
  });
  const of = <T extends Request["type"]>(type: T) =>
    requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
  return { transport, of };
}

function Providers({ transport, children }: { transport: MockTransport; children: ReactNode }) {
  const [queryClient] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  return (
    <QueryClientProvider client={queryClient}>
      <ApiProvider transport={transport}>
        <SettingsProvider>
          <JobsProvider>
            <TooltipProvider>
              <MemoryRouter>{children}</MemoryRouter>
            </TooltipProvider>
          </JobsProvider>
        </SettingsProvider>
      </ApiProvider>
    </QueryClientProvider>
  );
}

/** The plan a picture job ends with; its content does not matter here. */
const artPlan: JobResult = {
  type: "Plan",
  id: "plan-1",
  kind: "art",
  title: "Write 1 picture",
  root: "/lib",
  writeBytes: 10,
  deleteBytes: 0,
  freeBytes: 1 << 20,
  executable: true,
  deletesUserData: false,
  transactions: [],
  skipped: [],
  notices: [],
};

/** Sources that have covers and screenshots, and one this build cannot ask. */
const about = (): Promise<Response> =>
  Promise.resolve({
    type: "Attribution",
    name: "YAOPLM",
    version: "0.0.0",
    license: "GPL-3.0-or-later",
    repository: "https://example.invalid",
    data: [],
    art: [
      {
        id: "libretro",
        name: "libretro-thumbnails",
        url: "https://example.invalid",
        license: null,
        licenseUrl: null,
        credits: "",
        fields: null,
        use: "",
        kinds: ["COV", "SCR"],
        available: true,
      },
      {
        id: "screenscraper",
        name: "ScreenScraper",
        url: "https://example.invalid",
        license: "CC BY-NC-SA 4.0",
        licenseUrl: null,
        credits: "",
        fields: null,
        use: "",
        kinds: ["COV", "ICO", "COV2", "LAB", "BG", "SCR", "SCR2", "LGO"],
        available: false,
      },
    ],
    titles: { built: "2026-10-01", libretroVersion: "1", entries: 1 },
  });

function storeSettings(settings: Partial<Settings>) {
  window.localStorage.setItem("yaoplm.settings", JSON.stringify({ ...defaultSettings, root: "/lib", ...settings }));
}

beforeEach(async () => {
  await i18n.changeLanguage("en");
  storeSettings({});
});

describe("ArtVariantsDialog", () => {
  function Opener({ onPick = () => undefined }: { onPick?: () => void }) {
    const [open, setOpen] = useState(true);
    return <ArtVariantsDialog root="/lib" game={game} kind="COV" open={open} onOpenChange={setOpen} onPick={onPick} />;
  }

  it("stops its search when the dialog is closed", async () => {
    const core = fakeCore({});
    render(
      <Providers transport={core.transport}>
        <Opener />
      </Providers>,
    );
    await waitFor(() => expect(core.of("GetJob").length).toBeGreaterThan(0));
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByTestId("art-variants-dialog")).not.toBeInTheDocument());
    await waitFor(() => expect(core.of("CancelJob").map((r) => r.job)).toEqual([1]));
    expect(core.of("FindArt")).toHaveLength(1);
  });

  it("starts one search per opening under StrictMode", async () => {
    const core = fakeCore({});
    render(
      <StrictMode>
        <Providers transport={core.transport}>
          <Opener />
        </Providers>
      </StrictMode>,
    );
    await waitFor(() => expect(core.of("GetJob").length).toBeGreaterThan(0));
    expect(core.of("FindArt")).toHaveLength(1);
    expect(core.of("CancelJob")).toEqual([]);
  });

  it("moves the choice with the arrow keys", async () => {
    const variant = { width: 140, height: 200, png: "" };
    const core = fakeCore({
      results: {
        fetchArt: {
          type: "ArtVariants",
          startup: game.startup,
          kind: "COV",
          variants: [
            { ...variant, source: "xlenore", origin: "https://example.invalid/a.jpg" },
            { ...variant, source: "libretro", origin: "https://example.invalid/b.png" },
          ],
        },
      },
    });
    render(
      <Providers transport={core.transport}>
        <Opener />
      </Providers>,
    );
    const [first, second] = await screen.findAllByRole("radio");
    if (!first || !second) throw new Error("two variants expected");
    expect(screen.getAllByTestId("art-variant")).toEqual([first, second]);
    expect(first).toHaveAttribute("aria-checked", "true");
    first.focus();
    // Radix moves the focus on a timer and checks the item while the key is down.
    await userEvent.keyboard("{ArrowRight>}");
    await waitFor(() => expect(second).toHaveFocus());
    await userEvent.keyboard("{/ArrowRight}");
    expect(second).toHaveAttribute("aria-checked", "true");
    expect(first).toHaveAttribute("aria-checked", "false");
    // A finished search is not cancelled on closing.
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByTestId("art-variants-dialog")).not.toBeInTheDocument());
    expect(core.of("CancelJob")).toEqual([]);
  });
});

describe("ArtFetchDialog", () => {
  function Opener() {
    const [open, setOpen] = useState(true);
    return (
      <>
        <button type="button" onClick={() => setOpen(true)}>
          Open the download
        </button>
        <ArtFetchDialog root="/lib" games={[game]} open={open} onOpenChange={setOpen} />
      </>
    );
  }

  it("comes back with the form as it was when the plan is cancelled", async () => {
    const core = fakeCore({ results: { fetchArt: artPlan }, attribution: about });
    render(
      <Providers transport={core.transport}>
        <Opener />
      </Providers>,
    );
    const dialog = await screen.findByTestId("art-fetch-dialog");
    await userEvent.click(await within(dialog).findByTestId("art-kind-SCR"));
    await userEvent.click(within(dialog).getByTestId("art-scope-all"));
    await userEvent.click(within(dialog).getByTestId("art-fetch-start"));

    const plan = await screen.findByTestId("plan-dialog");
    expect(core.of("FetchArt")[0]).toMatchObject({ scope: "all", kinds: ["COV", "SCR"] });
    await userEvent.click(within(plan).getByRole("button", { name: "Cancel" }));

    const again = await screen.findByTestId("art-fetch-dialog");
    expect(within(again).getByTestId("art-kind-SCR")).toBeChecked();
    expect(within(again).getByTestId("art-scope-all")).toBeChecked();

    // Closed by the user, it opens afresh.
    await userEvent.click(within(again).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByTestId("art-fetch-dialog")).not.toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: "Open the download" }));
    const fresh = await screen.findByTestId("art-fetch-dialog");
    expect(within(fresh).getByTestId("art-kind-COV")).toBeChecked();
    expect(within(fresh).getByTestId("art-kind-SCR")).not.toBeChecked();
    expect(within(fresh).getByTestId("art-scope-missing")).toBeChecked();
  });
});

describe("ArtImportDialog", () => {
  function Opener() {
    const [open, setOpen] = useState(true);
    return (
      <>
        <button type="button" onClick={() => setOpen(true)}>
          Open the import
        </button>
        <ArtImportDialog root="/lib" open={open} onOpenChange={setOpen} />
      </>
    );
  }

  it("comes back with the form as it was when the plan is cancelled", async () => {
    const core = fakeCore({ results: { importArt: artPlan } });
    render(
      <Providers transport={core.transport}>
        <Opener />
      </Providers>,
    );
    const dialog = await screen.findByTestId("art-import-dialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Add archives…" }));
    // The picker answers asynchronously; Start stays disabled until it has.
    await within(dialog).findByTestId("art-import-sets");
    await userEvent.click(within(dialog).getByTestId("art-import-replace"));
    await userEvent.click(within(dialog).getByRole("checkbox", { name: "Logo" }));
    await userEvent.click(within(dialog).getByTestId("art-import-start"));

    const plan = await screen.findByTestId("plan-dialog");
    expect(core.of("ImportArt")[0]).toMatchObject({ sources: ["/sets/art.zip"], overwrite: true });
    expect(core.of("ImportArt")[0]?.kinds).not.toContain("LGO");
    await userEvent.click(within(plan).getByRole("button", { name: "Cancel" }));

    const again = await screen.findByTestId("art-import-dialog");
    expect(within(again).getByTestId("art-import-sets")).toHaveTextContent("art.zip");
    expect(within(again).getByTestId("art-import-replace")).toBeChecked();
    expect(within(again).getByRole("checkbox", { name: "Logo" })).not.toBeChecked();

    // Closed by the user, it opens afresh.
    await userEvent.click(within(again).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByTestId("art-import-dialog")).not.toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: "Open the import" }));
    const fresh = await screen.findByTestId("art-import-dialog");
    expect(within(fresh).queryByTestId("art-import-sets")).not.toBeInTheDocument();
    expect(within(fresh).getByTestId("art-import-replace")).not.toBeChecked();
    expect(within(fresh).getByRole("checkbox", { name: "Logo" })).toBeChecked();
  });
});

describe("useFetchableTypes", () => {
  function hook(attribution: () => Promise<Response>) {
    const core = fakeCore({ attribution });
    return renderHook(() => useFetchableTypes(), {
      wrapper: ({ children }) => <Providers transport={core.transport}>{children}</Providers>,
    });
  }

  it("says why nothing can be downloaded", async () => {
    const loading = hook(() => new Promise<Response>(() => undefined));
    expect(loading.result.current.blocked).toEqual({ why: "loading", text: "Checking which picture types the sources have…" });
    expect(loading.result.current.types.size).toBe(0);

    const failed = hook(() => Promise.reject(new RpcError({ code: "Internal", message: "offline" })));
    await waitFor(() =>
      expect(failed.result.current.blocked).toEqual({
        why: "failed",
        text: "Cannot tell which picture types the sources have: offline",
      }),
    );

    storeSettings({
      simpleMode: false,
      artSources: {
        xlenore: { enabled: false, mirror: "" },
        oplm: { enabled: false, mirror: "" },
        libretro: { enabled: false, mirror: "" },
        screenscraper: { enabled: false, mirror: "" },
      },
    });
    const off = hook(about);
    expect(off.result.current.blocked).toEqual({ why: "noSources", text: "Every source is switched off." });
    // Simple mode hides the switches: the text says where they are.
    storeSettings({
      simpleMode: true,
      artSources: {
        xlenore: { enabled: false, mirror: "" },
        oplm: { enabled: false, mirror: "" },
        libretro: { enabled: false, mirror: "" },
        screenscraper: { enabled: false, mirror: "" },
      },
    });
    expect(hook(about).result.current.blocked?.text).toBe(
      "Every picture source is switched off. They are in the settings, with simple mode off.",
    );
  });

  it("lists the types of the enabled sources", async () => {
    const ready = hook(about);
    await waitFor(() => expect(ready.result.current.blocked).toBeNull());
    expect([...ready.result.current.types].sort()).toEqual(["COV", "SCR"]);
  });
});
