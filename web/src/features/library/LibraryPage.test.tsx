import "@/i18n";

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { detected, loader, smbMode, testDevice } from "@/features/devices/testDevice";
import { LibraryPage } from "@/features/library/LibraryPage";
import { storeSettings } from "@/features/loaders/fixtures";
import { Providers } from "@/features/loaders/TestProviders";
import i18n from "@/i18n";

/** A core whose open device is gone: unplugged, or not one of a server's roots. */
function goneCore() {
  const transport = new MockTransport((request: Request): Response => {
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "ListDevices":
        return { type: "Devices", devices: [] };
      case "InspectDevice":
        throw new RpcError({ code: "NotFound", message: "E:\\ does not exist" });
      case "ScanLibrary":
        throw new RpcError({ code: "NotFound", message: "E:\\ does not exist" });
      case "DiscardPlan":
        return { type: "Ack" };
      default:
        throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
    }
  });
  return transport;
}

describe("the library on a device that is gone", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    storeSettings({ root: "E:\\" });
  });

  // An unplugged stick, or a root the server may not open, is not a scan
  // that failed (release pass 1.11): nothing here can work until another
  // device is chosen. On main the library showed "The scan failed: …" with
  // "Scan again" and "Add games" enabled.
  it("says the device is not available and leads to the device list, with no actions", async () => {
    render(
      <Providers transport={goneCore()}>
        <LibraryPage />
      </Providers>,
    );
    const gone = await screen.findByTestId("library-device-gone");
    expect(gone).toHaveTextContent("E:\\");
    expect(gone).toHaveTextContent("E:\\ does not exist");
    expect(screen.getByRole("button", { name: "Choose a device" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Add games" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Scan again" })).toBeNull();
    expect(screen.queryByRole("searchbox")).toBeNull();
  });
});

/** A core whose open folder has no OPL folders and no games yet. */
function emptyFolderCore() {
  const info = testDevice({ root: "E:\\", mode: smbMode, external: false, kind: "fixed", loader: loader({ detected: detected({ library: false }) }) });
  const transport = new MockTransport((request: Request): Response => {
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "ListDevices":
        return { type: "Devices", devices: [info] };
      case "InspectDevice":
        return { type: "Device", ...info };
      case "ScanLibrary":
        return { type: "Job", job: 1 };
      case "GetJob":
        return {
          type: "JobState",
          id: 1,
          kind: "scanLibrary",
          title: "Library scan",
          root: "E:\\",
          status: "finished",
          done: 0,
          total: null,
          rate: null,
          item: null,
          error: null,
          log: [],
          result: {
            type: "Library",
            root: "E:\\",
            profile: "Opl120Beta",
            folders: ["CD", "DVD", "ART", "CFG", "VMC", "CHT", "APPS", "THM", "LNG"].map((name) => ({ name, present: false })),
            games: [],
            apps: [],
            skipped: [],
            warnings: [],
          },
        };
      case "DiscardPlan":
        return { type: "Ack" };
      default:
        throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
    }
  });
  return transport;
}

describe("the library of a folder without OPL's folders", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    storeSettings({ root: "E:\\" });
  });

  // One next step (release pass 1.9, UI-6): "Create OPL folders"; "Add
  // games" comes once the folders exist. On main the empty state offered
  // both, with "Add games" drawn as the main one.
  it("offers Create OPL folders alone", async () => {
    render(
      <Providers transport={emptyFolderCore()}>
        <LibraryPage />
      </Providers>,
    );
    expect(await screen.findByTestId("smb-init-button")).toBeInTheDocument();
    // The scan is in: nothing but the folder step.
    await screen.findByTestId("library-count");
    expect(screen.queryByText("No games yet")).toBeNull();
    // The toolbar's "Add games" stays; the empty state does not repeat it.
    expect(screen.getAllByRole("link", { name: "Add games" })).toHaveLength(1);
  });
});
