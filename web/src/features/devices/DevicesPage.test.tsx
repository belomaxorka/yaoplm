import "@/i18n";

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { DevicesPage } from "@/features/devices/DevicesPage";
import { build, device, installedLoader, storeSettings } from "@/features/loaders/fixtures";
import { Providers } from "@/features/loaders/TestProviders";
import i18n from "@/i18n";

/** A core with one device, as `ListDevices` describes it. */
function core(info: DeviceInfo) {
  const transport = new MockTransport((request: Request): Response => {
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "ListDevices":
        return { type: "Devices", devices: [info] };
      case "LoaderCatalog":
        return { type: "LoaderCatalog", schema: 1, builds: [build("opl-stable"), build("opl-beta")], source: "builtIn", fetchedAt: null };
      case "DiscardPlan":
        return { type: "Ack" };
      default:
        throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
    }
  });
  return transport;
}

/** A stick with the beta installed, known by the version inside its ELF. */
const prepared = device({
  installed: [installedLoader({ freshness: "current", knownBy: "version" })],
  loader: {
    profile: "Opl120Beta",
    source: "detected",
    key: "serial:3C761797",
    detected: {
      profile: "Opl120Beta",
      confidence: "exact",
      reason: {
        key: "reasons.profileByElfVersion",
        params: { loader: "OPL", version: "v1.2.0-Beta-2271-b3a2199", path: "UPDT/OPL.ELF" },
      },
      reasonText: "OPL v1.2.0-Beta-2271-b3a2199 at UPDT/OPL.ELF, by the version inside the file",
      loaders: [],
      notices: [],
      library: true,
    },
  },
});

function show(info: DeviceInfo) {
  render(
    <Providers transport={core(info)}>
      <DevicesPage />
    </Providers>,
  );
}

describe("the device card with a loader on it (release pass 1.3)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    storeSettings({ offerLoaderUpdates: false });
  });

  // One main button and one story (release-ui-consistency rules 3, 5): on
  // main the card showed "Open library", "Reinstall loader…", "Replace
  // loader…", "Check for updates" and "Put back …" side by side, and named
  // the same build twice, in the "OPL version" line and in the loader block.
  it("keeps reinstalling and replacing in a menu, and names the build once", async () => {
    show(prepared);
    const card = await screen.findByTestId("device-card");
    await within(card).findByTestId("installed-loader");
    expect(within(card).getByTestId("device-open")).toBeInTheDocument();
    expect(within(card).queryByTestId("device-install-loader")).toBeNull();
    expect(within(card).queryByTestId("device-replace-loader")).toBeNull();
    expect(within(card).queryByTestId("loader-profile")).toBeNull();
    expect(within(card).getByTestId("installed-loader-version")).toHaveTextContent("v1.2.0-Beta-2271");

    await userEvent.click(within(card).getByTestId("device-more"));
    expect(await screen.findByTestId("device-install-loader")).toHaveTextContent("Reinstall loader…");
    expect(screen.getByTestId("device-replace-loader")).toHaveTextContent("Replace loader…");
  });

  it("offers Install loader… outright on a stick without one", async () => {
    show(device());
    const card = await screen.findByTestId("device-card");
    expect(await within(card).findByTestId("device-install-loader")).toHaveTextContent("Install loader…");
    expect(within(card).queryByTestId("device-more")).toBeNull();
    // Nothing to name: the profile line says what the device is prepared for.
    expect(within(card).getByTestId("loader-profile")).toBeInTheDocument();
  });

  it("keeps the OPL version line when the user chose it, since the files may disagree", async () => {
    show({ ...prepared, loader: { ...prepared.loader, source: "override", profile: "Opl110" } });
    const card = await screen.findByTestId("device-card");
    await within(card).findByTestId("installed-loader");
    expect(within(card).getByTestId("loader-profile")).toHaveTextContent("OPL 1.1.0");
  });
});
