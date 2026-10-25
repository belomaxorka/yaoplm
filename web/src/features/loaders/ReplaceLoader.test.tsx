import "@/i18n";

import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it } from "vitest";

import type { JobInfo } from "@/api/generated/JobInfo";
import type { JobResult } from "@/api/generated/JobResult";
import type { LoaderImpact } from "@/api/generated/LoaderImpact";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { build, device, file, installedLoader, offer, plan, storeSettings } from "@/features/loaders/fixtures";
import { InstallLoaderDialog } from "@/features/loaders/InstallLoaderDialog";
import { Providers } from "@/features/loaders/TestProviders";
import i18n from "@/i18n";

const zsoGame = (n: number) => ({
  startup: `SLUS_200.6${n}`,
  title: `Game ${n}`,
  path: `DVD/SLUS_200.6${n}.Game ${n}.zso`,
  size: 1 << 30,
  fragments: null,
});

function impactFor(target: string, blocked = false): LoaderImpact {
  const empty = { zso: [], fragmented: [], unknownFragments: [], art: [] };
  if (target === "opl-stable") {
    return {
      build: target,
      from: "Opl120Beta",
      to: "Opl110",
      blocked: blocked ? { key: "notices.replaceExfatOpl110", params: {} } : null,
      ...empty,
      ...(blocked ? {} : { zso: [zsoGame(1), zsoGame(2)] }),
    };
  }
  return { build: target, from: "Opl120Beta", to: "Opl120Beta", blocked: null, ...empty };
}

/** The core as the replace dialog sees it. */
function fakeCore({ blocked = false }: { blocked?: boolean } = {}) {
  const requests: Request[] = [];
  const jobs = new Map<number, JobInfo>();
  const start = (kind: JobInfo["kind"], result: JobResult | null): Response => {
    const id = jobs.size + 1;
    jobs.set(id, {
      id,
      kind,
      title: kind,
      root: null,
      status: "finished",
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
  const transport = new MockTransport((request) => {
    requests.push(request);
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "DiscardPlan":
        return { type: "Ack" };
      case "LoaderCatalog":
        return {
          type: "LoaderCatalog",
          schema: 1,
          builds: [
            build("opl-stable", { limits: { en: "No exFAT or ZSO.", ru: "Нет exFAT и ZSO." } }),
            build("opl-beta"),
            build("yaopl"),
          ],
          source: "builtIn",
          fetchedAt: null,
        };
      case "LoaderRelease":
        return start("loaderRelease", offer(false));
      case "LoaderReleases":
        return start("loaderReleases", {
          type: "LoaderReleases",
          build: request.build,
          offline: false,
          releases: [
            { tag: "latest", version: "v1.2.0-Beta-2370-33a2b19-yaopl", date: "2026-10-23", prerelease: true, default: true },
            {
              tag: "v1.2.0-Beta-2360-1234abc-yaopl",
              version: "v1.2.0-Beta-2360-1234abc-yaopl",
              date: "2026-10-20",
              prerelease: true,
              default: false,
            },
          ],
        });
      case "LoaderImpact":
        return start("loaderImpact", { type: "LoaderImpact", ...impactFor(request.build, blocked) });
      case "DownloadLoader":
        return start("downloadLoader", file);
      case "PlanReplaceLoader":
        return plan([], "replaceLoader");
      case "GetJob": {
        const info = jobs.get(request.job);
        if (!info) throw new RpcError({ code: "NotFound", message: `no job ${request.job}` });
        return { type: "JobState", ...info };
      }
      default:
        break;
    }
    throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
  });
  const of = <T extends Request["type"]>(type: T) =>
    requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
  return { transport, of };
}

function Opener() {
  const [open, setOpen] = useState(true);
  return (
    <InstallLoaderDialog
      device={device({ installed: [installedLoader({ freshness: "current" })] })}
      open={open}
      onOpenChange={setOpen}
      replace={installedLoader({ freshness: "current" })}
    />
  );
}

function show(core: ReturnType<typeof fakeCore>) {
  render(
    <Providers transport={core.transport}>
      <Opener />
    </Providers>,
  );
}

beforeEach(async () => {
  await i18n.changeLanguage("en");
  storeSettings({});
});

describe("Replace loader", () => {
  it("starts on another build, says what changes and converts the ticked ZSO first", async () => {
    const user = userEvent.setup();
    const core = fakeCore();
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    expect(within(dialog).getByRole("heading", { name: "Replace the loader" })).toBeTruthy();
    expect(dialog.textContent).toContain("Now: OPL 1.2.0 beta v1.2.0-Beta-2271 (b3a2199) in UPDT/OPL.ELF");
    // The installed build is marked; the recommended one is the installed one, so the next.
    const beta = await within(dialog).findByTestId("loader-build-opl-beta");
    expect(beta.closest("label")?.textContent).toContain("Installed");
    expect(within(dialog).getByTestId("loader-build-opl-stable").getAttribute("data-state")).toBe("checked");
    expect(dialog.textContent).toContain("Limits: No exFAT or ZSO.");
    expect(within(dialog).getByTestId("loader-method-fmcb").getAttribute("data-state")).toBe("checked");
    expect(within(dialog).queryByTestId("loader-method-card")).toBeNull();

    const impact = await within(dialog).findByTestId("loader-impact");
    expect(impact.textContent).toContain("ZSO games the new build cannot show: 2");
    const boxes = within(impact).getAllByTestId("loader-impact-zso");
    await user.click(boxes[1]!);
    await waitFor(() => expect(within(dialog).getByTestId<HTMLButtonElement>("loader-install-start").disabled).toBe(false));
    expect(within(dialog).getByTestId("loader-install-start").textContent).toContain("Download and replace");
    await user.click(within(dialog).getByTestId("loader-install-start"));
    await screen.findByTestId("plan-dialog");
    expect(core.of("PlanReplaceLoader")[0]).toMatchObject({
      root: "F:\\",
      build: "opl-stable",
      presets: ["fmcb"],
      device: "usb",
      profile: "Opl120Beta",
      convert: ["DVD/SLUS_200.61.Game 1.zso"],
      reencodeArt: true,
      retire: [],
    });
    expect(core.of("LoaderImpact")[0]).toMatchObject({ root: "F:\\", build: "opl-stable", profile: "Opl120Beta" });
  });

  it("refuses a build that cannot read the device", async () => {
    const core = fakeCore({ blocked: true });
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    const blocked = await within(dialog).findByTestId("loader-impact-blocked");
    expect(blocked.textContent).toContain("OPL 1.1.0 cannot read exFAT");
    await within(dialog).findByTestId("loader-offer");
    expect(within(dialog).getByTestId<HTMLButtonElement>("loader-install-start").disabled).toBe(true);
  });

  it("changes nothing for YAOPL, and moves the old loader away only when ticked", async () => {
    const user = userEvent.setup();
    const core = fakeCore();
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    await user.click(await within(dialog).findByTestId("loader-build-yaopl"));
    expect(await within(dialog).findByTestId("loader-impact-nothing")).toBeTruthy();
    expect(within(dialog).queryByTestId("loader-retire")).toBeNull();
    // Elsewhere: the beta at UPDT/OPL.ELF would stay unless ticked.
    await user.click(within(dialog).getByTestId("loader-method-app"));
    await user.click(within(dialog).getByTestId("loader-retire"));
    await waitFor(() => expect(within(dialog).getByTestId<HTMLButtonElement>("loader-install-start").disabled).toBe(false));
    await user.click(within(dialog).getByTestId("loader-install-start"));
    await screen.findByTestId("plan-dialog");
    expect(core.of("PlanReplaceLoader")[0]).toMatchObject({
      build: "yaopl",
      presets: ["app"],
      convert: [],
      retire: ["UPDT/OPL.ELF"],
    });
  });

  it("offers another release of the channel in the advanced mode", async () => {
    storeSettings({ simpleMode: false });
    const user = userEvent.setup();
    const core = fakeCore();
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    await user.click(await within(dialog).findByTestId("loader-build-yaopl"));
    await within(dialog).findByTestId("loader-offer");
    await user.click(within(dialog).getByTestId("loader-other-version"));
    await user.click(await within(dialog).findByTestId("loader-tag-v1.2.0-Beta-2360-1234abc-yaopl"));
    await waitFor(() =>
      expect(core.of("LoaderRelease").some((r) => r.tag === "v1.2.0-Beta-2360-1234abc-yaopl")).toBe(true),
    );
    expect(core.of("LoaderReleases")[0]).toMatchObject({ build: "yaopl" });
  });
});
