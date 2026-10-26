import "@/i18n";

import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it } from "vitest";

import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { JobResult } from "@/api/generated/JobResult";
import type { LoaderBootHint } from "@/api/generated/LoaderBootHint";
import type { PlanNotice } from "@/api/generated/PlanNotice";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { build, device, file, offer, plan, SHA, storeSettings } from "@/features/loaders/fixtures";
import { Providers } from "@/features/loaders/TestProviders";
import { InstallLoaderDialog } from "@/features/loaders/InstallLoaderDialog";
import i18n from "@/i18n";

const foreign: PlanNotice = {
  level: "error",
  code: "loaderForeignFile",
  message: "UPDT/OPL.ELF holds another program",
};

const fmcbHint: LoaderBootHint = {
  launcher: "fmcb",
  ps2Path: "mass:/UPDT/OPL.ELF",
  stock: true,
  config: null,
  lines: [],
  title: null,
};

/** The core as the dialog sees it; `plans` answers each PlanInstallLoader in turn. */
/**
 * How the write ends: done, failed in a transaction (the job still
 * finishes, with the error in its report), or held running until a
 * CancelJob comes.
 */
type Write = "done" | "failed" | "held";

function fakeCore({
  languages = false,
  plans = [plan()],
  write = "done",
}: { languages?: boolean; plans?: Response[]; write?: Write } = {}) {
  const requests: Request[] = [];
  const jobs = new Map<number, JobInfo>();
  let planned = 0;
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
          builds: [build("opl-stable"), build("opl-beta"), build("yaopl")],
          source: "builtIn",
          fetchedAt: null,
        };
      case "LoaderRelease":
        return start("loaderRelease", offer(languages));
      case "DownloadLoader":
        return start("downloadLoader", file);
      case "PlanInstallLoader":
        return plans[Math.min(planned++, plans.length - 1)] ?? plan();
      case "PlanInit":
        return plan([], "initLibrary");
      case "ExecutePlan": {
        const started = start("executePlan", null) as Extract<Response, { type: "Job" }>;
        const job = jobs.get(started.job);
        if (job && write === "failed") {
          job.result = {
            type: "Execution",
            outcomes: ["rolledBack"],
            cancelled: false,
            error: "Could not write UPDT/OPL.ELF: device removed",
            journal: null,
            checks: [],
            notices: [],
          };
        }
        if (job && write === "held") job.status = "running";
        return started;
      }
      case "CancelJob": {
        const job = jobs.get(request.job);
        if (job) {
          job.status = "cancelled";
          job.result = { type: "Execution", outcomes: ["rolledBack"], cancelled: true, error: null, journal: null, checks: [], notices: [] };
        }
        return { type: "Ack" };
      }
      case "GetJob": {
        const info = jobs.get(request.job);
        if (!info) throw new RpcError({ code: "NotFound", message: `no job ${request.job}` });
        return { type: "JobState", ...info };
      }
      case "LoaderBootHints":
        return { type: "LoaderBootHints", hints: [fmcbHint] };
      default:
        break;
    }
    throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
  });
  const of = <T extends Request["type"]>(type: T) =>
    requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
  return { transport, of };
}

function Opener({ info }: { info: DeviceInfo }) {
  const [open, setOpen] = useState(true);
  return <InstallLoaderDialog device={info} open={open} onOpenChange={setOpen} />;
}

function show(core: ReturnType<typeof fakeCore>, info: DeviceInfo = device()) {
  render(
    <Providers transport={core.transport}>
      <Opener info={info} />
    </Providers>,
  );
}

beforeEach(async () => {
  await i18n.changeLanguage("en");
  storeSettings({});
});

describe("InstallLoaderDialog", () => {
  it("installs the recommended build through a plan, then says how to start it", async () => {
    const user = userEvent.setup();
    const core = fakeCore();
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    // The release is shown before anything is downloaded.
    expect(await within(dialog).findByText("v1.2.0-Beta-2271-b3a2199")).toBeTruthy();
    expect(within(dialog).getByText(/Licence: AFL-3.0/)).toBeTruthy();
    expect(within(dialog).getByTestId("loader-build-opl-beta").getAttribute("data-state")).toBe("checked");
    expect(within(dialog).getByTestId("loader-method-fmcb").getAttribute("data-state")).toBe("checked");
    expect(within(dialog).getByTestId("loader-target-usb").getAttribute("data-state")).toBe("checked");
    expect(core.of("DownloadLoader")).toHaveLength(0);
    expect(core.of("PlanInstallLoader")).toHaveLength(0);

    await user.click(within(dialog).getByTestId("loader-install-start"));
    const planDialog = await screen.findByTestId("plan-dialog");
    expect(core.of("DownloadLoader")[0]).toMatchObject({ build: "opl-beta", tag: "latest", sha256: SHA });
    expect(core.of("DownloadLoader")[0]?.languages).toBeUndefined();
    expect(core.of("PlanInstallLoader")[0]).toMatchObject({
      root: "F:\\",
      build: "opl-beta",
      sha256: SHA,
      presets: ["fmcb"],
      device: "usb",
    });
    expect(core.of("PlanInstallLoader")[0]?.starterSettings).toBeUndefined();

    await user.click(within(planDialog).getByTestId("plan-confirm"));
    const hints = await screen.findByTestId("loader-boot-hints");
    expect(within(hints).getByText(/stock config starts mass:\/UPDT\/OPL\.ELF/)).toBeTruthy();
    expect(within(hints).getByText(/USB device start mode to Auto/)).toBeTruthy();
    expect(core.of("LoaderBootHints")[0]).toMatchObject({ build: "opl-beta", presets: ["fmcb"], device: "usb" });
  });

  it("says a write that failed did not install, rather than how to start it", async () => {
    const user = userEvent.setup();
    const core = fakeCore({ write: "failed" });
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    await within(dialog).findByText("v1.2.0-Beta-2271-b3a2199");
    await user.click(within(dialog).getByTestId("loader-install-start"));
    await user.click(within(await screen.findByTestId("plan-dialog")).getByTestId("plan-confirm"));
    // The wizard comes back once the plan dialog is gone.
    const after = await screen.findByTestId("loader-install-dialog");
    expect(await within(after).findByRole("alert")).toHaveTextContent("Could not write UPDT/OPL.ELF: device removed");
    expect(screen.queryByTestId("loader-boot-hints")).toBeNull();
  });

  it("lets the write it waits for be cancelled from the wizard", async () => {
    const user = userEvent.setup();
    const core = fakeCore({ write: "held" });
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    await within(dialog).findByText("v1.2.0-Beta-2271-b3a2199");
    await user.click(within(dialog).getByTestId("loader-install-start"));
    await user.click(within(await screen.findByTestId("plan-dialog")).getByTestId("plan-confirm"));
    // The wizard covers the jobs dock: its own Cancel stops the write.
    const after = await screen.findByTestId("loader-install-dialog");
    const stop = await within(after).findByTestId("loader-install-cancel-write");
    expect((stop as HTMLButtonElement).disabled).toBe(false);
    await user.click(stop);
    await waitFor(() => expect(core.of("CancelJob")).toHaveLength(1));
    expect(await within(after).findByRole("alert")).toHaveTextContent("Cancelled: the loader stays as it was");
    expect(screen.queryByTestId("loader-boot-hints")).toBeNull();
  });

  it("starts on MX4SIO for an SD card, and “I don’t know” takes both stock paths on USB", async () => {
    const user = userEvent.setup();
    const core = fakeCore();
    show(core, device({ bus: "sd" }));
    const dialog = await screen.findByTestId("loader-install-dialog");
    await within(dialog).findByTestId("loader-offer");
    expect(within(dialog).getByTestId("loader-target-mx4sio").getAttribute("data-state")).toBe("checked");
    expect(within(dialog).getByTestId("loader-method-app").getAttribute("data-state")).toBe("checked");
    // FreeMcBoot and PS2BBL read USB only.
    expect(within(dialog).queryByTestId("loader-method-fmcb")).toBeNull();
    expect(within(dialog).queryByTestId("loader-starter")).toBeNull();

    await user.click(within(dialog).getByTestId("loader-target-usb"));
    await user.click(within(dialog).getByTestId("loader-method-unknown"));
    expect(within(dialog).getByText(/UPDT\/OPL\.ELF, APPS\/OPNPS2LD\.ELF/)).toBeTruthy();
    await user.click(within(dialog).getByTestId("loader-install-start"));
    await screen.findByTestId("plan-dialog");
    expect(core.of("PlanInstallLoader")[0]).toMatchObject({ presets: ["fmcb", "ps2bbl"], device: "usb" });
  });

  it("replaces a file that is not a known OPL only when the user ticks it", async () => {
    const user = userEvent.setup();
    const core = fakeCore({ plans: [plan([foreign]), plan()] });
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    await within(dialog).findByTestId("loader-offer");
    await user.click(within(dialog).getByTestId("loader-install-start"));
    const planDialog = await screen.findByTestId("plan-dialog");
    const confirm = within(planDialog).getByTestId<HTMLButtonElement>("plan-confirm");
    expect(confirm.disabled).toBe(true);

    await user.click(within(planDialog).getByTestId("loader-replace-foreign"));
    await waitFor(() => expect(core.of("PlanInstallLoader")).toHaveLength(2));
    expect(core.of("PlanInstallLoader")[1]).toMatchObject({ presets: ["fmcb"], replaceForeign: true });
    await waitFor(() => expect(within(planDialog).getByTestId<HTMLButtonElement>("plan-confirm").disabled).toBe(false));
    // The choice stays on screen, ticked, so it can be taken back.
    expect(within(planDialog).getByTestId("loader-replace-foreign").getAttribute("data-state")).toBe("checked");
  });

  it("shows OPL 1.1.0 unavailable on exFAT with the reason, as replacing does", async () => {
    const user = userEvent.setup();
    const exfat = device({ fileSystem: "exFAT", fsName: "exFAT" });
    // "Reinstall…" of a 1.1.0 found there starts on a build that can work.
    render(
      <Providers transport={fakeCore().transport}>
        <InstallLoaderDialog device={exfat} open onOpenChange={() => {}} initial={{ build: "opl-stable", preset: "fmcb" }} />
      </Providers>,
    );
    const dialog = await screen.findByTestId("loader-install-dialog");
    const stable = await within(dialog).findByTestId("loader-build-opl-stable");
    expect(stable).toBeDisabled();
    expect(within(dialog).getByText(/OPL 1\.1\.0 cannot read exFAT/)).toBeTruthy();
    expect(within(dialog).getByTestId("loader-build-opl-beta").getAttribute("data-state")).toBe("checked");
    await user.click(stable);
    expect(within(dialog).getByTestId("loader-build-opl-beta").getAttribute("data-state")).toBe("checked");
  });

  it("with OPL on the memory card, creates only the folders", async () => {
    const user = userEvent.setup();
    const core = fakeCore();
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    await user.click(await within(dialog).findByTestId("loader-method-card"));
    expect(within(dialog).queryByTestId("loader-build-opl-beta")).toBeNull();
    await user.click(within(dialog).getByTestId("loader-install-start"));
    const planDialog = await screen.findByTestId("plan-dialog");
    expect(core.of("PlanInit")).toEqual([{ type: "PlanInit", root: "F:\\" }]);
    await user.click(within(planDialog).getByTestId("plan-confirm"));
    expect(await screen.findByText(/start OPL from the memory card as usual/)).toBeTruthy();
    expect(core.of("DownloadLoader")).toHaveLength(0);
    expect(core.of("LoaderBootHints")).toHaveLength(0);
  });

  it("offers the interface language’s pack and the starter settings in the advanced mode", async () => {
    await i18n.changeLanguage("ru");
    storeSettings({ simpleMode: false });
    const user = userEvent.setup();
    const core = fakeCore({ languages: true });
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    await within(dialog).findByTestId("loader-offer");
    expect(within(dialog).getByText("opl-beta по-русски")).toBeTruthy();
    expect(within(dialog).getByTestId("loader-method-custom")).toBeTruthy();
    const language = within(dialog).getByTestId("loader-language");
    expect(language.getAttribute("data-state")).toBe("unchecked");
    await user.click(language);
    await user.click(within(dialog).getByTestId("loader-starter"));
    await user.click(within(dialog).getByTestId("loader-install-start"));
    await screen.findByTestId("plan-dialog");
    expect(core.of("DownloadLoader")[0]?.languages).toBe(true);
    expect(core.of("PlanInstallLoader")[0]).toMatchObject({ language: "Russian", starterSettings: true });
  });

  it("asks for a path of the user’s own", async () => {
    storeSettings({ simpleMode: false });
    const user = userEvent.setup();
    const core = fakeCore();
    show(core);
    const dialog = await screen.findByTestId("loader-install-dialog");
    await within(dialog).findByTestId("loader-offer");
    await user.click(within(dialog).getByTestId("loader-method-custom"));
    const start = within(dialog).getByTestId<HTMLButtonElement>("loader-install-start");
    expect(start.disabled).toBe(true);
    await user.type(within(dialog).getByTestId("loader-path"), "APPS/MINE/OPL.ELF");
    await user.click(start);
    await screen.findByTestId("plan-dialog");
    expect(core.of("PlanInstallLoader")[0]).toMatchObject({ presets: [], path: "APPS/MINE/OPL.ELF" });
  });
});
