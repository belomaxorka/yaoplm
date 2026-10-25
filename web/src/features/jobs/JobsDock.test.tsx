import "@/i18n";

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { App } from "@/App";
import i18n from "@/i18n";

describe("the jobs dock", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    window.history.pushState({}, "", "/settings");
  });

  it("cancels a write in simple mode, where there is no jobs page", async () => {
    const requests: Request[] = [];
    const transport = new MockTransport((request): Response => {
      requests.push(request);
      switch (request.type) {
        case "ListJobs":
          return {
            type: "Jobs",
            jobs: [
              { id: 7, kind: "executePlan", title: "Add 2 games", root: "E:\\", status: "running", done: 0, total: null, rate: null, item: null, error: null, log: [] },
            ],
          };
        case "CancelJob":
          return { type: "Ack" };
        case "Version":
          return { type: "Version", name: "YAOPLM", version: "0.0.0-mock" };
        default:
          throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
      }
    });
    render(<App transport={transport} />);
    expect(screen.queryByRole("link", { name: "All jobs" })).not.toBeInTheDocument();
    await userEvent.click(await screen.findByRole("button", { name: "Cancel: Add 2 games" }));
    await waitFor(() => expect(requests.filter((r) => r.type === "CancelJob")).toEqual([{ type: "CancelJob", job: 7 }]));
  });

  it("keeps the running write and every queued one in reach, in the order they run", async () => {
    // Writes run one at a time, oldest first: the running one is the oldest.
    const job = (id: number, title: string, status: "running" | "queued") => ({
      id,
      kind: "executePlan" as const,
      title,
      root: "E:\\",
      status,
      done: 0,
      total: null,
      rate: null,
      item: null,
      error: null,
      log: [],
    });
    const requests: Request[] = [];
    const transport = new MockTransport((request): Response => {
      requests.push(request);
      switch (request.type) {
        case "ListJobs":
          return {
            type: "Jobs",
            jobs: [
              job(3, "Add 16 games", "running"),
              job(4, "Write 12 pictures", "queued"),
              job(5, "Rename Okami", "queued"),
              job(6, "Delete WRC 4", "queued"),
              job(7, "Move Ico to DVD", "queued"),
            ],
          };
        case "CancelJob":
          return { type: "Ack" };
        case "Version":
          return { type: "Version", name: "YAOPLM", version: "0.0.0-mock" };
        default:
          throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
      }
    });
    render(<App transport={transport} />);
    const dock = await screen.findByTestId("jobs-dock");
    const titles = [...dock.querySelectorAll("li")].map((li) => li.textContent);
    expect(titles[0]).toContain("Add 16 games");
    expect(titles).toHaveLength(5);
    expect(titles.at(-1)).toContain("Move Ico to DVD");
    await userEvent.click(screen.getByRole("button", { name: "Cancel: Add 16 games" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel: Move Ico to DVD" }));
    await waitFor(() =>
      expect(requests.filter((r) => r.type === "CancelJob")).toEqual([
        { type: "CancelJob", job: 3 },
        { type: "CancelJob", job: 7 },
      ]),
    );
  });
});
