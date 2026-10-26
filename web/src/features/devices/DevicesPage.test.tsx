import "@/i18n";

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { DevicesPage } from "@/features/devices/DevicesPage";
import { detected, emulatorInfo, emulatorMode, loader, smbMode, testDevice } from "@/features/devices/testDevice";
import { build, device, installedLoader, storeSettings } from "@/features/loaders/fixtures";
import { Providers } from "@/features/loaders/TestProviders";
import i18n from "@/i18n";

/** A core with the given devices, as `ListDevices` describes them. */
function core(...infos: DeviceInfo[]) {
  const transport = new MockTransport((request: Request): Response => {
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "ListDevices":
        return { type: "Devices", devices: infos };
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

function show(...infos: DeviceInfo[]) {
  render(
    <Providers transport={core(...infos)}>
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
    expect(await screen.findByTestId("device-install-loader")).toHaveTextContent("Reinstall OPL…");
    expect(screen.getByTestId("device-replace-loader")).toHaveTextContent("Switch OPL…");
  });

  it("offers Install loader… outright on a stick without one", async () => {
    show(device());
    const card = await screen.findByTestId("device-card");
    expect(await within(card).findByTestId("device-install-loader")).toHaveTextContent("Install OPL…");
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

const smbRules = { level: "info", code: "smbRules", message: "OPL reads this folder over SMB: SMBv1 only, image names up to 159 bytes, at most 10 ul parts" } as const;

/** A folder on the computer's disk without OPL's folders, read over SMB. */
const emptyFolder = (root: string, patch: Partial<DeviceInfo> = {}) =>
  testDevice({
    root,
    mode: smbMode,
    external: true,
    kind: "fixed",
    askProfile: true,
    loader: loader({ key: `path:${root}`, detected: detected({ library: false }) }),
    advice: [smbRules],
    ...patch,
  });

describe("the device card's order and notes (release pass 1.1, 1.14, 5.5)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    storeSettings({ offerLoaderUpdates: false, simpleMode: false, root: "D:\\Games" });
  });

  // The folder's next step comes before the version question, and the
  // main button before both: that is also the Tab order.
  it("puts the main button first, then the folder step, then the question", async () => {
    show(emptyFolder("D:\\Games"));
    const card = await screen.findByTestId("device-card");
    const open = within(card).getByTestId("device-open");
    const init = within(card).getByTestId("smb-init");
    const question = within(card).getByTestId("loader-question");
    const before = (a: Element, b: Element) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(before(open, init)).toBe(true);
    expect(before(init, question)).toBe(true);
  });

  // The SMB rules are one note for the open folder, not one under every card.
  it("shows the SMB note on the open folder only", async () => {
    show(emptyFolder("D:\\Games"), emptyFolder("D:\\More"));
    const cards = await screen.findAllByTestId("device-card");
    expect(cards).toHaveLength(2);
    expect(within(cards[0]!).queryAllByText(/OPL reads this folder over SMB/)).toHaveLength(1);
    expect(within(cards[1]!).queryAllByText(/OPL reads this folder over SMB/)).toHaveLength(0);
  });
});

describe("the emulator's panel on the device card (UI-7, ADR-0048)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    storeSettings({ offerLoaderUpdates: false, simpleMode: false, root: "D:\\Games" });
  });

  // The owner's remark of 2026-10-27: the boot cards of every variant made
  // the card a screen tall. One line says what the SD is and how many boot
  // cards it has; the channels come when the panel is opened.
  it("comes folded with one line of summary, and opens to the boot channels", async () => {
    show(testDevice({ root: "G:\\", emulator: emulatorInfo, mode: emulatorMode }));
    const panel = await screen.findByTestId("emulator-panel");
    const details = within(panel).getByTestId("emulator-details");
    expect(details).not.toHaveAttribute("open");
    const summary = within(panel).getByTestId("emulator-summary");
    expect(summary).toHaveTextContent("Memory card emulator SD");
    expect(summary).toHaveTextContent("— sd2psx or sd2psXtd");
    expect(summary).toHaveTextContent("Boot cards: PS2 — 2");
    expect(within(panel).getByTestId("emulator-protected")).not.toBeVisible();
    expect(within(panel).getAllByTestId("emulator-boot-channel")[0]).not.toBeVisible();
    await userEvent.click(summary);
    expect(details).toHaveAttribute("open");
    expect(within(panel).getByTestId("emulator-protected")).toBeVisible();
    expect(within(panel).getAllByTestId("emulator-boot-channel")[0]).toHaveTextContent("FMCB 1.966");
  });
});

describe("the main button on the device card (UI-6, UI-7)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    storeSettings({ offerLoaderUpdates: false, simpleMode: false, root: "F:\\" });
  });

  // The owner's decision of 2026-10-27: the dark button marks the device
  // that is open now. On main it marked any card without a step, so the
  // first card kept it while the second was open.
  it("darkens Open library on the open device only", async () => {
    show(prepared, { ...prepared, root: "G:\\" });
    const cards = await screen.findAllByTestId("device-card");
    expect(cards).toHaveLength(2);
    expect(within(cards[0]!).getByTestId("device-open")).toHaveAttribute("data-variant", "default");
    expect(within(cards[1]!).getByTestId("device-open")).toHaveAttribute("data-variant", "outline");
  });

  // Installing OPL is the stick's step, like creating the folders: it is
  // the main button, open or not, and Open library steps back.
  it("makes Install OPL… the main button on a stick without a loader", async () => {
    show(device(), device({ root: "G:\\" }));
    const cards = await screen.findAllByTestId("device-card");
    for (const card of cards) {
      expect(await within(card).findByTestId("device-install-loader")).toHaveAttribute("data-variant", "default");
      expect(within(card).getByTestId("device-open")).toHaveAttribute("data-variant", "outline");
    }
  });

  it("keeps the folder step as the main button of the open folder", async () => {
    storeSettings({ offerLoaderUpdates: false, simpleMode: false, root: "D:\\Games" });
    show(emptyFolder("D:\\Games"));
    const card = await screen.findByTestId("device-card");
    expect(within(card).getByTestId("smb-init-button")).toHaveAttribute("data-variant", "default");
    expect(within(card).getByTestId("device-open")).toHaveAttribute("data-variant", "outline");
  });
});

describe("facts and reasons on the device card (UI-4, UI-7)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    storeSettings({ offerLoaderUpdates: false, simpleMode: false, root: "D:\\Games" });
  });

  // The owner's decision of 2026-10-27: both modes keep the reason of the
  // connection and of the OPL version behind the ⓘ icon; the advanced
  // mode differs by what it lists, not by how long a line is. Its facts
  // are one line, joined by a middle dot. On main the advanced mode spelt
  // every reason out after a dash and spaced the facts apart.
  it("keeps the reasons behind the icon in advanced mode too, and joins the facts", async () => {
    show(device({ model: "USB3.0 FLASH DRIVE", partition: { table: "mbr", number: 1, offset: 1048576, mbrType: 12 } }));
    const card = await screen.findByTestId("device-card");
    await within(card).findByTestId("loader-profile");
    expect(within(card).getByTestId("device-mode-reason")).toHaveClass("sr-only");
    expect(within(card).getByTestId("loader-profile-reason")).toHaveClass("sr-only");
    expect(within(card).getAllByRole("button", { name: "Why" })).toHaveLength(2);
    const facts = within(card).getByTestId("device-facts");
    expect(facts).toHaveTextContent("Model: USB3.0 FLASH DRIVE");
    expect(facts).toHaveTextContent("Serial: 3C761797");
    expect(facts).toHaveTextContent("Cluster: 32 KiB");
    expect(facts).toHaveTextContent("Partition: MBR #1");
  });
});
