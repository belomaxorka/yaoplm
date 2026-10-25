import "@/i18n";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { ApiProvider } from "@/api/ApiProvider";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { PlanView } from "@/api/generated/PlanView";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import i18n from "@/i18n";
import { JobsProvider } from "@/jobs/JobsProvider";
import { SettingsProvider } from "@/settings/SettingsProvider";
import { readSettings } from "@/settings/settings";

import { DeviceModeLine, SmbFolderHint } from "./DeviceMode";
import { detected, KEY, loader, mode, smbMode, testDevice } from "./testDevice";

const systemRoot = {
  level: "warning",
  code: "systemRoot",
  message: "The whole system disk is open: for sharing over the network, a separate folder such as C:\\PS2 is better",
} as const;

/** A core that records the plans asked for. */
function core() {
  const planned: Extract<Request, { type: "PlanInit" }>[] = [];
  const transport = new MockTransport((request: Request): Response => {
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "PlanInit": {
        planned.push(request);
        const plan: PlanView = {
          id: "p1",
          kind: "initLibrary",
          title: "Create the OPL folders",
          root: request.root,
          writeBytes: 0,
          deleteBytes: 0,
          freeBytes: 1 << 30,
          executable: true,
          deletesUserData: false,
          transactions: [],
          skipped: [],
          notices: [],
        };
        return { type: "Plan", ...plan };
      }
      case "DiscardPlan":
        return { type: "Ack" };
      default:
        throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
    }
  });
  return { transport, planned };
}

function show(ui: React.ReactNode, transport = core().transport) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider transport={transport}>
        <SettingsProvider>
          <JobsProvider>
            <MemoryRouter>{ui}</MemoryRouter>
          </JobsProvider>
        </SettingsProvider>
      </ApiProvider>
    </QueryClientProvider>,
  );
}

const emptyFolder = (patch: Partial<DeviceInfo> = {}) =>
  testDevice({
    root: "D:\\Games",
    mode: smbMode,
    external: false,
    kind: "fixed",
    loader: loader({ detected: detected({ library: false }) }),
    ...patch,
  });

describe("how the PS2 reads a device (ADR-0045)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });
  afterEach(() => window.localStorage.clear());

  it("shows the mode and its place, and switches it for this device", async () => {
    show(<DeviceModeLine device={emptyFolder()} />);
    const line = screen.getByTestId("device-mode");
    expect(line).toHaveTextContent("Connection to the PS2: over the network (SMB) — a folder on the computer’s disk");
    await userEvent.click(screen.getByTestId("device-mode-change"));
    await userEvent.click(await screen.findByTestId("device-mode-ps2"));
    expect(readSettings().modeOverrides).toEqual({ [KEY]: "ps2" });
  });

  it("a choice is shown as one and goes back to auto", async () => {
    window.localStorage.setItem("yaoplm.settings", JSON.stringify({ modeOverrides: { [KEY]: "smb" } }));
    show(<DeviceModeLine device={testDevice({ mode: mode({ mode: "smb", source: "override" }) })} />);
    expect(screen.getByTestId("device-mode-reason")).toHaveTextContent("chosen for this device");
    await userEvent.click(screen.getByTestId("device-mode-change"));
    await userEvent.click(await screen.findByTestId("device-mode-auto"));
    expect(readSettings().modeOverrides).toEqual({});
  });

  it("an empty folder for SMB gets OPL's folders, not a loader", async () => {
    const fake = core();
    show(<SmbFolderHint device={emptyFolder()} />, fake.transport);
    expect(screen.getByTestId("smb-init")).toHaveTextContent("OPL’s folders are not here yet");
    await userEvent.click(screen.getByTestId("smb-init-button"));
    await screen.findByTestId("plan-dialog");
    expect(fake.planned).toEqual([{ type: "PlanInit", root: "D:\\Games" }]);
  });

  it("an empty stick for the PS2, a prepared folder or a read-only one gets nothing here", () => {
    const readOnly = { level: "error", code: "readOnly", message: "The volume is read-only" } as const;
    for (const device of [
      testDevice({ loader: loader({ detected: detected({ library: false }) }) }),
      emptyFolder({ loader: loader() }),
      emptyFolder({ advice: [readOnly] }),
      emptyFolder({ root: "/", system: true, advice: [readOnly, systemRoot] }),
    ]) {
      const { container, unmount } = show(<SmbFolderHint device={device} />);
      expect(container).toBeEmptyDOMElement();
      unmount();
    }
  });

  it("a whole system disk gets a folder of its own", async () => {
    const fake = core();
    show(<SmbFolderHint device={emptyFolder({ root: "C:\\", system: true, advice: [systemRoot] })} />, fake.transport);
    expect(screen.queryByTestId("smb-init")).toBeNull();
    await userEvent.click(screen.getByTestId("smb-create-folder-button"));
    const name = await screen.findByTestId("smb-folder-name");
    expect(name).toHaveValue("PS2");
    await userEvent.clear(name);
    await userEvent.type(name, "a/b");
    expect(screen.getByTestId("smb-folder-submit")).toBeDisabled();
    await userEvent.clear(name);
    await userEvent.type(name, "Games");
    await userEvent.click(screen.getByTestId("smb-folder-submit"));
    await screen.findByTestId("plan-dialog");
    expect(fake.planned).toEqual([{ type: "PlanInit", root: "C:\\", folder: "Games" }]);
  });

  it("a server's roots are its administrator's: only the warning", () => {
    const fake = core();
    Object.defineProperty(fake.transport, "kind", { value: "http" });
    const { container } = show(
      <SmbFolderHint device={emptyFolder({ root: "/", system: true, advice: [systemRoot] })} />,
      fake.transport,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
