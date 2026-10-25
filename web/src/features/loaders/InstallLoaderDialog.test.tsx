import "@/i18n";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { type ReactNode, useState } from "react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it } from "vitest";

import { ApiProvider } from "@/api/ApiProvider";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { JobResult } from "@/api/generated/JobResult";
import type { LoaderBootHint } from "@/api/generated/LoaderBootHint";
import type { LoaderBuild } from "@/api/generated/LoaderBuild";
import type { PlanNotice } from "@/api/generated/PlanNotice";
import type { PlanView } from "@/api/generated/PlanView";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { TooltipProvider } from "@/components/ui/tooltip";
import { InstallLoaderDialog } from "@/features/loaders/InstallLoaderDialog";
import i18n from "@/i18n";
import { JobsProvider } from "@/jobs/JobsProvider";
import { defaultSettings, type Settings } from "@/settings/settings";
import { SettingsProvider } from "@/settings/SettingsProvider";

const SHA = "a".repeat(64);

function build(id: string, patch: Partial<LoaderBuild> = {}): LoaderBuild {
  const folder = id === "yaopl" ? "YAOPL" : "OPL";
  return {
    id,
    family: "opl",
    name: id === "opl-beta" ? "OPL 1.2.0 beta" : id === "opl-stable" ? "OPL 1.1.0" : "YAOPL",
    publisher: id === "yaopl" ? "fork" : "official",
    repo: "ps2homebrew/Open-PS2-Loader",
    profile: id === "opl-stable" ? "Opl110" : "Opl120Beta",
    recommended: id === "opl-beta",
    devices: ["usb", "mx4sio"],
    license: { spdx: "AFL-3.0", url: "https://github.com/ps2homebrew/Open-PS2-Loader/blob/master/LICENSE", authors: "OPL team" },
    summary: { en: `${id} in English`, ru: `${id} по-русски` },
    limits: {},
    docs: [{ kind: "readme", url: "https://github.com/ps2homebrew/Open-PS2-Loader#readme" }],
    channel: { tag: "latest", floating: true, prerelease: false },
    presets: [
      { id: "fmcb", path: "UPDT/OPL.ELF", title: null, devices: ["usb"], launchers: ["fmcb"] },
      { id: "ps2bbl", path: "APPS/OPNPS2LD.ELF", title: null, devices: ["usb"], launchers: ["ps2bbl"] },
      {
        id: "app",
        path: `APPS/${folder}/OPNPS2LD.ELF`,
        title: id === "yaopl" ? "YAOPL" : "Open PS2 Loader",
        devices: ["usb", "mx4sio"],
        launchers: ["oplApps", "ps2bblMx4sio", "osdmenu"],
      },
    ],
    defaultPresets: [
      { device: "usb", preset: "fmcb" },
      { device: "mx4sio", preset: "app" },
    ],
    ...patch,
  };
}

function device(patch: Partial<DeviceInfo> = {}): DeviceInfo {
  return {
    root: "F:\\",
    label: "TESTS",
    fileSystem: "FAT32",
    fsName: "FAT32",
    totalBytes: 64 << 30,
    freeBytes: 60 << 30,
    clusterSize: 32768,
    sectorSize: 512,
    serial: "3C761797",
    kind: "removable",
    bus: "usb",
    model: null,
    external: true,
    system: false,
    readOnly: false,
    partition: null,
    advice: [],
    appleDouble: null,
    unfinished: [],
    loader: { profile: "Opl120Beta", source: "detected", key: "serial:3C761797", detected: null },
    ...patch,
  };
}

const offer = (languages: boolean): JobResult => ({
  type: "LoaderOffer",
  build: "opl-beta",
  tag: "latest",
  version: "v1.2.0-Beta-2271-b3a2199",
  date: "2026-10-20",
  file: { name: "OPNPS2LD.7z", url: "https://github.com/x", size: 1_500_000, sha256: SHA, sources: ["digest"] },
  member: "OPNPS2LD.ELF",
  languages: languages
    ? { name: "OPNPS2LD-LANGS.7z", url: "https://github.com/y", size: 900_000, sha256: "b".repeat(64), sources: ["digest"] }
    : null,
  releaseUrl: "https://github.com/ps2homebrew/Open-PS2-Loader/releases/tag/latest",
  commitUrl: "https://github.com/ps2homebrew/Open-PS2-Loader/commit/b3a2199",
  changelog: [{ text: "Fix exFAT on big sticks", link: null }],
  changelogMore: false,
  license: { spdx: "AFL-3.0", url: "https://github.com/ps2homebrew/Open-PS2-Loader/blob/master/LICENSE", authors: "OPL team" },
  offline: false,
  stored: null,
});

const file: JobResult = {
  type: "LoaderFile",
  sha256: SHA,
  size: 1_200_000,
  version: "v1.2.0-Beta-2271-b3a2199",
  build: "opl-beta",
  tag: "latest",
  file: "OPNPS2LD.7z",
  member: "OPNPS2LD.ELF",
  source: "https://github.com/ps2homebrew/Open-PS2-Loader/releases/tag/latest",
  confirmed: true,
  stable: null,
  languages: false,
  storedAt: 0,
};

function plan(notices: PlanNotice[] = [], kind: PlanView["kind"] = "installLoader"): Response {
  const view: PlanView = {
    id: `plan-${notices.length}`,
    kind,
    title: "Install OPL 1.2.0 beta",
    root: "F:\\",
    writeBytes: 1_200_000,
    deleteBytes: 0,
    freeBytes: 60 << 30,
    executable: !notices.some((n) => n.level === "error"),
    deletesUserData: false,
    transactions: [],
    skipped: [],
    notices,
  };
  return { type: "Plan", ...view };
}

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
function fakeCore({ languages = false, plans = [plan()] }: { languages?: boolean; plans?: Response[] } = {}) {
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
      case "ExecutePlan":
        return start("executePlan", null);
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

function storeSettings(settings: Partial<Settings>) {
  window.localStorage.setItem("yaoplm.settings", JSON.stringify({ ...defaultSettings, ...settings }));
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
