import "@/i18n";

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { JobId } from "@/api/generated/JobId";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { JobKind } from "@/api/generated/JobKind";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { App } from "@/App";
import i18n from "@/i18n";
import { device } from "@/features/loaders/fixtures";

const ROOT = "E:\\";

/** Scans that run until the test finishes them, and jobs it emits by hand. */
class FakeCore {
  readonly requests: Request[] = [];
  readonly jobs = new Map<JobId, JobInfo>();
  readonly transport = new MockTransport((request) => this.#handle(request));

  of<T extends Request["type"]>(type: T): Extract<Request, { type: T }>[] {
    return this.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
  }

  scans(): JobInfo[] {
    return [...this.jobs.values()].filter((job) => job.kind === "scanLibrary");
  }

  finishScans(): void {
    for (const scan of this.scans()) {
      if (scan.status !== "running") continue;
      this.jobs.set(scan.id, {
        ...scan,
        status: "finished",
        result: { type: "Library", root: ROOT, profile: "Opl120Beta", folders: [], games: [], apps: [], skipped: [], warnings: [] },
      });
    }
  }

  /** A job of `kind` that the core reports as queued, run and finished. */
  runJob(kind: JobKind): void {
    const id = this.jobs.size + 100;
    this.jobs.set(id, this.#job(id, kind, "finished"));
    act(() => {
      this.transport.emit({ type: "Queued", job: id, kind, title: kind, root: ROOT });
      this.transport.emit({ type: "Started", job: id });
      this.transport.emit({ type: "Finished", job: id });
    });
  }

  #job(id: JobId, kind: JobKind, status: JobInfo["status"]): JobInfo {
    return { id, kind, title: kind, root: ROOT, status, done: 0, total: null, rate: null, item: null, error: null, log: [] };
  }

  #handle(request: Request): Response {
    this.requests.push(request);
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [...this.jobs.values()] };
      // The open device answers: without it the library shows "not available".
      case "InspectDevice":
        return { type: "Device", ...device({ root: ROOT }) };
      case "ScanLibrary": {
        const id = this.jobs.size + 1;
        this.jobs.set(id, this.#job(id, "scanLibrary", "running"));
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
}

describe("library scans", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    // The jobs page is in the advanced mode.
    window.localStorage.setItem("yaoplm.settings", JSON.stringify({ root: ROOT, simpleMode: false }));
    window.history.pushState({}, "", "/library");
  });

  it("run once per opening and once per finished write queue, and never show as jobs", async () => {
    const core = new FakeCore();
    render(<App transport={core.transport} />);
    await waitFor(() => expect(core.of("ScanLibrary")).toHaveLength(1));

    // Leaving the library mid-scan neither cancels the scan nor starts a second one.
    await userEvent.click(screen.getByRole("link", { name: "Jobs" }));
    await userEvent.click(screen.getByRole("link", { name: "Library" }));
    core.finishScans();
    await screen.findByText("No games yet");
    expect(core.of("ScanLibrary")).toHaveLength(1);
    expect(core.of("CancelJob")).toHaveLength(0);

    // A write job ends: one rescan, even with the user on another screen.
    await userEvent.click(screen.getByRole("link", { name: "Jobs" }));
    core.runJob("executePlan");
    await waitFor(() => expect(core.of("ScanLibrary")).toHaveLength(2));

    // Another write ends while that rescan runs: the stale scan stops on the core.
    core.runJob("executePlan");
    await waitFor(() => expect(core.of("ScanLibrary")).toHaveLength(3));
    await waitFor(() => expect(core.of("CancelJob").map((r) => r.job)).toEqual([core.scans()[1]?.id]));
    core.finishScans();

    // A slot's picture search is the card's own business (ART-2): not a job either.
    core.runJob("findArt");

    // The jobs page lists the writes, not the scans or the searches.
    const cards = await screen.findAllByTestId("job-card");
    expect(cards.map((card) => card.dataset.kind)).toEqual(["executePlan", "executePlan"]);
    expect(screen.queryByRole("link", { name: /active job/ })).not.toBeInTheDocument();
  });
});
