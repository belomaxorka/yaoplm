import "@/i18n";

import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { JobResult } from "@/api/generated/JobResult";
import type { LoaderOffer } from "@/api/generated/LoaderOffer";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { DevicesPage } from "@/features/devices/DevicesPage";
import { emulatorInfo, emulatorMode, smbMode } from "@/features/devices/testDevice";
import { build, device, file, installedLoader, offer, plan, storeSettings } from "@/features/loaders/fixtures";
import { Providers } from "@/features/loaders/TestProviders";
import i18n from "@/i18n";
import { readSettings } from "@/settings/settings";

const NEW = "v1.2.0-Beta-2280-0123abc";

/** The newest beta as the store last read it. */
function newest(): LoaderOffer {
  const { type: _type, ...release } = offer(false) as Extract<JobResult, { type: "LoaderOffer" }>;
  return {
    ...release,
    version: NEW,
    file: { ...release.file, sha256: "d".repeat(64) },
    commitUrl: "https://github.com/ps2homebrew/Open-PS2-Loader/commit/0123abc",
    changelog: [{ text: "Fix exFAT on big sticks", link: null }],
  };
}

/** The core behind the devices screen; `devices` is what ListDevices answers. */
function fakeCore(devices: () => DeviceInfo[], { failWrite = false }: { failWrite?: boolean } = {}) {
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
      case "ListDevices":
        return { type: "Devices", devices: devices() };
      case "LoaderCatalog":
        return { type: "LoaderCatalog", schema: 1, builds: [build("opl-beta")], source: "builtIn", fetchedAt: null };
      case "CachedLoaderRelease":
        return { type: "CachedLoaderRelease", offer: newest() };
      case "UpdateLoaderCatalog":
        // Nothing on main answers here: the check goes on without it.
        throw new RpcError({ code: "Internal", message: "offline" });
      case "LoaderRelease":
        return start("loaderRelease", { type: "LoaderOffer", ...newest() });
      case "DownloadLoader":
        return start("downloadLoader", file);
      case "PlanUpdateLoader":
        return plan([], "updateLoader");
      case "PlanRollbackLoader":
        return plan([], "rollbackLoader");
      case "ExecutePlan":
        // A failed transaction still ends the job as finished, with the
        // error in its report.
        return start(
          "executePlan",
          failWrite
            ? {
                type: "Execution",
                outcomes: ["rolledBack"],
                cancelled: false,
                error: "Could not write UPDT/OPL.ELF: device removed",
                journal: null,
                checks: [],
                notices: [],
              }
            : null,
        );
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

function show(core: ReturnType<typeof fakeCore>) {
  return render(
    <Providers transport={core.transport}>
      <DevicesPage />
    </Providers>,
  );
}

beforeEach(async () => {
  await i18n.changeLanguage("en");
  storeSettings({});
});

describe("loader updates on the device card", () => {
  it("offers an update by itself once, and remembers Not now", async () => {
    const user = userEvent.setup();
    const core = fakeCore(() => [device({ installed: [installedLoader()] })]);
    const view = show(core);
    const dialog = await screen.findByTestId("loader-update-dialog");
    expect(within(dialog).getByRole("heading", { name: "Update available" })).toBeTruthy();
    const versions = await within(dialog).findByTestId("loader-update-versions");
    expect(versions.textContent).toContain("v1.2.0-Beta-2271 (b3a2199)");
    expect(versions.textContent).toContain("v1.2.0-Beta-2280 (0123abc)");
    expect(
      within(dialog).getByRole("link", { name: /What changed since your version/ }).getAttribute("href"),
    ).toBe("https://github.com/ps2homebrew/Open-PS2-Loader/compare/b3a2199...0123abc");
    // Nothing went to the network: the window reads the store.
    expect(core.of("LoaderRelease")).toHaveLength(0);

    await user.click(within(dialog).getByTestId("loader-update-later"));
    await waitFor(() => expect(screen.queryByTestId("loader-update-dialog")).toBeNull());
    expect(readSettings().loaderUpdatesOffered).toEqual([`serial:3C761797|${"c".repeat(64)}`]);

    view.unmount();
    show(core);
    expect(await screen.findByTestId("installed-loader-update")).toBeTruthy();
    expect(screen.queryByTestId("loader-update-dialog")).toBeNull();
  });

  it("remembers the pair when the offered window's update runs", async () => {
    const user = userEvent.setup();
    let updated = false;
    const core = fakeCore(() => [
      device({
        installed: [
          updated
            ? installedLoader({ version: NEW, sha256: "e".repeat(64), freshness: "current", latest: NEW })
            : installedLoader(),
        ],
      }),
    ]);
    show(core);
    const dialog = await screen.findByTestId("loader-update-dialog");
    await waitFor(() => expect(within(dialog).getByTestId<HTMLButtonElement>("loader-update-start").disabled).toBe(false));
    await user.click(within(dialog).getByTestId("loader-update-start"));
    const planDialog = await screen.findByTestId("plan-dialog");
    updated = true;
    await user.click(within(planDialog).getByTestId("plan-confirm"));
    await waitFor(() => expect(screen.queryByTestId("loader-update-dialog")).toBeNull());
    expect(readSettings().loaderUpdatesOffered).toEqual([`serial:3C761797|${"c".repeat(64)}`]);
    expect(await screen.findByTestId("installed-loader-current")).toBeTruthy();
  });

  it("never offers when the setting is off, and updates from the card", async () => {
    storeSettings({ offerLoaderUpdates: false });
    const user = userEvent.setup();
    const core = fakeCore(() => [device({ installed: [installedLoader({ language: "Russian" })] })]);
    show(core);
    const card = await screen.findByTestId("installed-loader");
    expect(card.getAttribute("data-freshness")).toBe("update");
    expect(screen.queryByTestId("loader-update-dialog")).toBeNull();

    await user.click(within(card).getByTestId("installed-loader-update-to"));
    const dialog = await screen.findByTestId("loader-update-dialog");
    expect(within(dialog).getByRole("heading", { name: "Update the loader" })).toBeTruthy();
    expect(dialog.textContent).toContain("Only UPDT/OPL.ELF and the Russian menu language are replaced");
    await waitFor(() => expect(within(dialog).getByTestId<HTMLButtonElement>("loader-update-start").disabled).toBe(false));
    await user.click(within(dialog).getByTestId("loader-update-start"));
    const planDialog = await screen.findByTestId("plan-dialog");
    // Held to the hash the window showed; the menu language comes along.
    expect(core.of("DownloadLoader")[0]).toMatchObject({ build: "opl-beta", tag: "latest", sha256: "d".repeat(64), languages: true });
    expect(core.of("PlanUpdateLoader")).toEqual([
      { type: "PlanUpdateLoader", root: "F:\\", path: "UPDT/OPL.ELF", sha256: (file as { sha256: string }).sha256 },
    ]);
    const listed = core.of("ListDevices").length;
    await user.click(within(planDialog).getByTestId("plan-confirm"));
    await waitFor(() => expect(core.of("ListDevices").length).toBeGreaterThan(listed));
    await waitFor(() => expect(screen.queryByTestId("loader-update-dialog")).toBeNull());
  });

  it("checks for updates on request and puts the previous build back", async () => {
    const user = userEvent.setup();
    const core = fakeCore(() => [
      device({ installed: [installedLoader({ freshness: "current", latest: "v1.2.0-Beta-2271-b3a2199" })] }),
    ]);
    show(core);
    const card = await screen.findByTestId("installed-loader");
    expect(within(card).getByTestId("installed-loader-current").textContent).toBe("Up to date");
    expect(within(card).queryByTestId("installed-loader-update-to")).toBeNull();

    const listed = core.of("ListDevices").length;
    await user.click(within(card).getByTestId("installed-loader-check"));
    await waitFor(() => expect(core.of("LoaderRelease")).toEqual([{ type: "LoaderRelease", build: "opl-beta" }]));
    await waitFor(() => expect(core.of("ListDevices").length).toBeGreaterThan(listed));
    expect(await within(card).findByText("Nothing newer")).toBeTruthy();

    await user.click(within(card).getByRole("button", { name: /Put back v1.2.0-Beta-2260/ }));
    await screen.findByTestId("plan-dialog");
    expect(core.of("PlanRollbackLoader")).toEqual([{ type: "PlanRollbackLoader", root: "F:\\", path: "UPDT/OPL.ELF" }]);
  });

  it("shows what it cannot name without update buttons", async () => {
    const core = fakeCore(() => [
      device({
        installed: [
          installedLoader({ build: null, version: null, knownBy: "name", freshness: "unknown", latest: null, backup: null }),
        ],
      }),
    ]);
    storeSettings({ simpleMode: false });
    show(core);
    const card = await screen.findByTestId("installed-loader");
    expect(card.textContent).toContain("version unknown");
    expect(card.textContent).toContain("Build not recognised");
    expect(within(card).queryByTestId("installed-loader-check")).toBeNull();
    expect(within(card).queryByTestId("installed-loader-rollback")).toBeNull();
    // Only the family is known: "Replace…" alone (§5.10.1), in the card's menu.
    await userEvent.click(screen.getByTestId("device-more"));
    expect(screen.queryByTestId("device-install-loader")).toBeNull();
    expect((await screen.findByTestId("device-replace-loader")).textContent).toContain("Switch OPL…");
  });

  it("stays open with the reason when the update's write fails", async () => {
    storeSettings({ offerLoaderUpdates: false });
    const user = userEvent.setup();
    const core = fakeCore(() => [device({ installed: [installedLoader()] })], { failWrite: true });
    show(core);
    const card = await screen.findByTestId("installed-loader");
    await user.click(within(card).getByTestId("installed-loader-update-to"));
    const dialog = await screen.findByTestId("loader-update-dialog");
    await waitFor(() => expect(within(dialog).getByTestId<HTMLButtonElement>("loader-update-start").disabled).toBe(false));
    await user.click(within(dialog).getByTestId("loader-update-start"));
    await user.click(within(await screen.findByTestId("plan-dialog")).getByTestId("plan-confirm"));
    const after = await screen.findByTestId("loader-update-dialog");
    expect(await within(after).findByRole("alert")).toHaveTextContent("Could not write UPDT/OPL.ELF: device removed");
  });

  it("leaves the loader in a folder read over SMB alone (ADR-0047)", async () => {
    const core = fakeCore(() => [device({ mode: smbMode, installed: [installedLoader()] })]);
    show(core);
    const card = await screen.findByTestId("installed-loader");
    expect(card.getAttribute("data-freshness")).toBe("update");
    // A copy of what the console runs from its card or stick: shown, never
    // offered or acted on.
    expect(screen.queryByTestId("loader-update-dialog")).toBeNull();
    expect(within(card).queryByTestId("installed-loader-update-to")).toBeNull();
    expect(within(card).queryByTestId("installed-loader-check")).toBeNull();
    expect(within(card).queryByTestId("installed-loader-rollback")).toBeNull();
    expect(screen.queryByTestId("device-install-loader")).toBeNull();
    expect(screen.queryByTestId("device-replace-loader")).toBeNull();
  });

  it("puts no loader on a memory card emulator's SD in any mode (ADR-0048)", async () => {
    // Even switched to "plugged into the console": the emulator's files keep
    // it the emulator's SD, whose OPL runs from inside its boot card.
    for (const mode of [emulatorMode, device().mode]) {
      const core = fakeCore(() => [device({ mode, emulator: emulatorInfo, installed: [installedLoader()] })]);
      const { unmount } = show(core);
      const card = await screen.findByTestId("installed-loader");
      expect(screen.getByTestId("emulator-panel")).toHaveTextContent("Memory card emulator SD");
      expect(screen.queryByTestId("loader-update-dialog")).toBeNull();
      expect(within(card).queryByTestId("installed-loader-update-to")).toBeNull();
      expect(within(card).queryByTestId("installed-loader-rollback")).toBeNull();
      expect(screen.queryByTestId("device-install-loader")).toBeNull();
      expect(screen.queryByTestId("device-replace-loader")).toBeNull();
      unmount();
    }
  });

  for (const simple of [true, false]) {
    it(`the emulator's card in ${simple ? "simple" : "advanced"} mode: channels and protection always, details only in advanced`, async () => {
      storeSettings({ simpleMode: simple });
      show(fakeCore(() => [device({ mode: emulatorMode, emulator: emulatorInfo })]));
      const panel = await screen.findByTestId("emulator-panel");
      expect(panel).toHaveTextContent("The emulator’s files are left alone");
      expect(panel).toHaveTextContent("OPL runs from the boot card");
      const channels = within(panel).getAllByTestId("emulator-boot-channel");
      expect(channels[0]).toHaveTextContent("Channel 1");
      expect(channels[0]).toHaveTextContent("FMCB 1.966");
      expect(channels[1]).toHaveTextContent("no name in BootCard.ini");
      expect(within(panel).queryByTestId("emulator-settings") === null).toBe(simple);
      expect(within(panel).queryByTestId("emulator-cards") === null).toBe(simple);
      if (!simple) expect(within(panel).getByTestId("emulator-settings")).toHaveTextContent("[PS2] CardSize=64");
      // What each image holds shows in both modes; the OPL builds inside,
      // with MMCE, in advanced mode only.
      expect(channels[0]).toHaveTextContent("FreeMcBoot");
      expect(channels[1]).toHaveTextContent("SD Builder’s FMCB 1.963");
      const [first] = channels;
      if (!first) throw new Error("no boot channel");
      const loaders = within(first).queryAllByTestId("emulator-boot-loader");
      expect(loaders).toHaveLength(simple ? 0 : 2);
      if (!simple) {
        expect(loaders[1]).toHaveTextContent("OPL v1.2.0-Beta-2216-bb415d2");
        expect(loaders[1]).toHaveTextContent("MMCE");
        expect(loaders[0]).not.toHaveTextContent("MMCE");
      }
    });
  }
});
