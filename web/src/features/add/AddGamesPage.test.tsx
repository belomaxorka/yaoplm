import "@/i18n";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "@/api/ApiProvider";
import type { JobId } from "@/api/generated/JobId";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { PlanTransaction } from "@/api/generated/PlanTransaction";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AddGamesPage } from "@/features/add/AddGamesPage";
import i18n from "@/i18n";
import { JobsProvider } from "@/jobs/JobsProvider";
import { defaultSettings } from "@/settings/settings";
import { SettingsProvider } from "@/settings/SettingsProvider";

// The desktop pickers stand in for the server's folder browser: one click adds the sources.
const picked = vi.hoisted(() => ({ files: [] as string[] }));
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  isTauri: () => true,
}));
vi.mock("@/features/add/desktop", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/features/add/desktop")>()),
  pickFiles: () => Promise.resolve(picked.files),
  listenForDrops: () => () => undefined,
}));

type PlanImport = Extract<Request, { type: "PlanImport" }>;

/** A game ID per source, by its position. */
const idOf = (index: number) => `SLUS_000.0${index + 1}`;

/**
 * Enough of the core for the add page: import plans (with a cover after
 * each game when they name a cover set) and FetchArt jobs that run until
 * the test finishes them.
 */
class FakeCore {
  readonly requests: Request[] = [];
  readonly jobs = new Map<JobId, JobInfo>();
  /** Cover sets and plans the server no longer keeps. */
  readonly expired = new Set<string>();
  /** Holds import plans that name a cover set until `releasePlans`. */
  holdPlans = false;
  #held: (() => void)[] = [];
  #plans = 0;

  readonly transport = new MockTransport((request) => this.#handle(request));

  of<T extends Request["type"]>(type: T): Extract<Request, { type: T }>[] {
    return this.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
  }

  finish(job: JobId): void {
    const info = this.jobs.get(job);
    if (!info) throw new Error(`no job ${job}`);
    this.jobs.set(job, {
      ...info,
      status: "finished",
      result: { type: "Covers", id: `set-${job}`, found: 2, missing: 0, present: 0 },
    });
  }

  releasePlans(): void {
    this.holdPlans = false;
    for (const release of this.#held.splice(0)) release();
  }

  async #handle(request: Request): Promise<Response> {
    this.requests.push(request);
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "DiscardPlan":
        return { type: "Ack" };
      case "Attribution":
        return {
          type: "Attribution",
          name: "YAOPLM",
          version: "0.0.0",
          license: "GPL-3.0-or-later",
          repository: "https://example.invalid",
          data: [],
          art: [
            { id: "xlenore", name: "xlenore", url: "", license: null, licenseUrl: null, credits: "", fields: null, use: "", kinds: ["COV"], available: true },
            { id: "libretro", name: "libretro", url: "", license: null, licenseUrl: null, credits: "", fields: null, use: "", kinds: ["COV", "SCR", "LGO"], available: true },
          ],
          titles: { built: "2026-10-01", libretroVersion: "1", entries: 1 },
        };
      case "PlanImport":
        if (request.covers && this.holdPlans) await new Promise<void>((resolve) => this.#held.push(resolve));
        return this.#plan(request);
      case "FetchArt": {
        if (request.import && this.expired.has(request.import)) {
          throw new RpcError({
            code: "NotFound",
            message: `Plan ${request.import} expired or already ran`,
            phrase: { key: "errors.planExpired", params: { id: request.import } },
          });
        }
        const id = this.jobs.size + 1;
        this.jobs.set(id, {
          id,
          kind: "fetchArt",
          title: "Cover search",
          root: request.root,
          status: "running",
          done: 0,
          total: null,
          rate: null,
          item: null,
          error: null,
          log: [],
        });
        return { type: "Job", job: id };
      }
      case "GetJob": {
        const info = this.jobs.get(request.job);
        if (!info) throw new RpcError({ code: "NotFound", message: `no job ${request.job}` });
        return { type: "JobState", ...info };
      }
      case "CancelJob": {
        const info = this.jobs.get(request.job);
        if (info?.status === "running") this.jobs.set(request.job, { ...info, status: "cancelled" });
        return { type: "Ack" };
      }
      default:
        throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
    }
  }

  #plan(request: PlanImport): Response {
    if (request.covers && this.expired.has(request.covers)) {
      throw new RpcError({
        code: "NotFound",
        message: `The cover search ${request.covers} is no longer kept`,
        phrase: { key: "errors.coversExpired", params: { id: request.covers } },
      });
    }
    const transactions = request.sources.flatMap((source, index): PlanTransaction[] => {
      const startup = idOf(index);
      const title = request.overrides?.find((o) => o.source === source)?.title ?? `Game ${index + 1}`;
      const game = { title, startup, media: "DVD" as const, format: "ISO" as const };
      const write = { op: "writeFile" as const, to: null, bytes: 1024, removal: null };
      const add: PlanTransaction = {
        title: `Add ${title}`,
        game,
        steps: [{ ...write, path: `DVD/${startup}.${title}.iso`, source }],
        commit: 1,
        notices: [],
      };
      if (!request.covers) return [add];
      const cover: PlanTransaction = {
        title: `${title}: cover`,
        titlePhrase: { key: "titles.artFor", params: { title, kind: "cover" } },
        game,
        steps: [{ ...write, path: `ART/${startup}_COV.png`, source: `/cache/${startup}.png` }],
        commit: 1,
        notices: [],
      };
      return [add, cover];
    });
    this.#plans += 1;
    return {
      type: "Plan",
      id: `plan-${this.#plans}`,
      kind: "import",
      title: "Import",
      root: request.root,
      writeBytes: 1024 * transactions.length,
      deleteBytes: 0,
      freeBytes: 1 << 30,
      executable: true,
      deletesUserData: false,
      transactions,
      skipped: [],
      notices: [],
    };
  }
}

function renderPage(core: FakeCore) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider transport={core.transport}>
        <SettingsProvider>
          <JobsProvider>
            <TooltipProvider>
              <MemoryRouter>
                <AddGamesPage />
              </MemoryRouter>
            </TooltipProvider>
          </JobsProvider>
        </SettingsProvider>
      </ApiProvider>
    </QueryClientProvider>,
  );
}

/** Adds the sources and asks for the plan; resolves once it is shown. */
async function planFor(sources: string[]) {
  picked.files = sources;
  await userEvent.click(screen.getByRole("button", { name: "Add files…" }));
  // The picker answers asynchronously; until then the button is disabled.
  await waitFor(() => expect(screen.getByTestId("preview-plan")).toBeEnabled());
  await userEvent.click(screen.getByTestId("preview-plan"));
  return screen.findByTestId("import-plan");
}

const confirm = () => screen.getByTestId("plan-confirm");
const search = () => screen.getByTestId("cover-search");
// The cover search polls its job with a backoff of up to a second.
const slow = { timeout: 4000 };

describe("AddGamesPage covers", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    window.localStorage.setItem("yaoplm.settings", JSON.stringify({ ...defaultSettings, root: "/lib" }));
  });

  it("waits for the cover search and its re-plan, and survives covers switched off and on", async () => {
    const core = new FakeCore();
    renderPage(core);
    const plan = await planFor(["/src/one.iso"]);

    expect(search()).toHaveAttribute("data-status", "running");
    expect(confirm()).toBeDisabled();
    await waitFor(() => expect(core.of("FetchArt")).toHaveLength(1));
    expect(core.of("FetchArt")[0]?.import).toBe("plan-1");
    // Every type the sources have, not only front covers.
    expect(core.of("FetchArt")[0]?.kinds).toEqual(["COV", "SCR", "LGO"]);

    // Off: the job is cancelled and the plan without covers can run.
    await userEvent.click(screen.getByTestId("covers-on-add"));
    await waitFor(() => expect(core.of("CancelJob").map((r) => r.job)).toEqual([1]));
    expect(screen.queryByTestId("cover-search")).not.toBeInTheDocument();
    expect(confirm()).toBeEnabled();

    // On again: a new search, not the old one coming back.
    await userEvent.click(screen.getByTestId("covers-on-add"));
    expect(search()).toHaveAttribute("data-status", "running");
    expect(confirm()).toBeDisabled();
    await waitFor(() => expect(core.of("FetchArt")).toHaveLength(2));

    // Found: the plan on screen is stale until the one with the covers comes back.
    core.holdPlans = true;
    core.finish(2);
    await waitFor(() => expect(search()).toHaveAttribute("data-status", "done"), slow);
    expect(confirm()).toBeDisabled();
    await waitFor(() => expect(core.of("PlanImport").at(-1)?.covers).toBe("set-2"));
    expect(confirm()).toBeDisabled();
    expect(within(plan).getByText("Updating the plan…")).toBeInTheDocument();

    core.releasePlans();
    await waitFor(() => expect(within(plan).getAllByTestId("plan-transaction")).toHaveLength(2));
    await waitFor(() => expect(confirm()).toBeEnabled());
    expect(core.of("FetchArt")).toHaveLength(2);
  }, 15_000);

  it("keeps a title being typed focused when the covers are inserted", async () => {
    const core = new FakeCore();
    renderPage(core);
    const plan = await planFor(["/src/one.iso", "/src/two.iso"]);
    const second = within(plan).getAllByLabelText("Title")[1];
    if (!second) throw new Error("no title field for the second game");
    second.focus();

    await waitFor(() => expect(core.of("FetchArt")).toHaveLength(1));
    core.finish(1);
    // [one, two] becomes [one, cover, two, cover]: the second game's card stays the same.
    await waitFor(() => expect(within(plan).getAllByTestId("plan-transaction")).toHaveLength(4), slow);
    expect(second).toBeInTheDocument();
    expect(second).toHaveFocus();
  }, 15_000);

  it("starts a new search when the cover set has expired, keeping the plan on screen", async () => {
    const core = new FakeCore();
    renderPage(core);
    const plan = await planFor(["/src/one.iso"]);
    const title = within(plan).getByLabelText("Title");
    title.focus();
    await waitFor(() => expect(core.of("FetchArt")).toHaveLength(1));

    // Neither may the plan vanish nor an error flash while the set is replaced.
    const blinks = new Set<string>();
    const watch = new MutationObserver(() => {
      if (!document.querySelector('[data-testid="import-plan"]')) blinks.add("plan gone");
      if (document.querySelector('[role="alert"]')) blinks.add("alert");
    });
    watch.observe(document.body, { childList: true, subtree: true, attributes: true, characterData: true });

    core.expired.add("set-1");
    core.finish(1);
    // The plan naming set-1 fails: the plan on screen is re-planned without
    // covers, and the new search starts from that plan.
    await waitFor(() => expect(core.of("PlanImport").map((r) => r.covers)).toEqual([undefined, "set-1", undefined]));
    await waitFor(() => expect(core.of("FetchArt")).toHaveLength(2), slow);
    expect(core.of("FetchArt")[1]?.import).toBe("plan-2");
    expect(confirm()).toBeDisabled();
    core.finish(2);
    await waitFor(() => expect(core.of("PlanImport").at(-1)?.covers).toBe("set-2"), slow);
    await waitFor(() => expect(within(plan).getAllByTestId("plan-transaction")).toHaveLength(2));
    await waitFor(() => expect(confirm()).toBeEnabled());
    watch.disconnect();

    expect([...blinks]).toEqual([]);
    expect(screen.getByTestId("import-plan")).toBe(plan);
    expect(title).toBeInTheDocument();
    expect(title).toHaveFocus();
  }, 15_000);

  it("does not search from a plan that expired with its cover set", async () => {
    const core = new FakeCore();
    renderPage(core);
    const plan = await planFor(["/src/one.iso"]);
    await waitFor(() => expect(core.of("FetchArt")).toHaveLength(1));
    core.finish(1);
    await waitFor(() => expect(within(plan).getAllByTestId("plan-transaction")).toHaveLength(2), slow);
    await waitFor(() => expect(confirm()).toBeEnabled());

    const alerts = new Set<string>();
    const watch = new MutationObserver(() => {
      if (document.querySelector('[role="alert"]')) alerts.add("alert");
    });
    watch.observe(document.body, { childList: true, subtree: true, attributes: true, characterData: true });

    // An hour later both the set and the plan built with it are gone.
    core.expired.add("set-1");
    core.expired.add("plan-2");
    const title = within(plan).getByLabelText("Title");
    await userEvent.type(title, " II");
    await waitFor(() => expect(core.of("FetchArt")).toHaveLength(2), slow);
    expect(core.of("FetchArt")[1]?.import).toBe("plan-3");
    expect(core.of("PlanImport").map((r) => r.covers)).toEqual([undefined, "set-1", "set-1", undefined]);
    core.finish(2);
    await waitFor(() => expect(core.of("PlanImport").at(-1)?.covers).toBe("set-2"), slow);
    await waitFor(() => expect(confirm()).toBeEnabled());
    watch.disconnect();

    expect([...alerts]).toEqual([]);
    expect(title).toHaveFocus();
  }, 15_000);

  it("discards the plan and its search when the last source is removed", async () => {
    const core = new FakeCore();
    renderPage(core);
    await planFor(["/src/one.iso"]);
    await waitFor(() => expect(core.of("FetchArt")).toHaveLength(1));

    await userEvent.click(screen.getByRole("button", { name: "Remove one.iso" }));
    expect(screen.queryByTestId("import-plan")).not.toBeInTheDocument();
    expect(screen.queryByText("Updating the plan…")).not.toBeInTheDocument();
    await waitFor(() => expect(core.of("DiscardPlan").map((r) => r.plan)).toEqual(["plan-1"]));
    await waitFor(() => expect(core.of("CancelJob").map((r) => r.job)).toEqual([1]));
  });
});
