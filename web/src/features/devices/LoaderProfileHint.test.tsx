import "@/i18n";

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import i18n from "@/i18n";
import { SettingsProvider } from "@/settings/SettingsProvider";
import { readSettings } from "@/settings/settings";

import { LoaderProfileHint } from "./LoaderProfile";
import { detected, emulatorInfo, emulatorMode, KEY, loader, smbMode, testDevice } from "./testDevice";

function show(device: DeviceInfo) {
  return render(
    <SettingsProvider>
      <LoaderProfileHint device={device} />
    </SettingsProvider>,
  );
}

describe("the loader profile question (ADR-0041, ADR-0047)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });
  afterEach(() => window.localStorage.clear());

  it("asks when the core says so and remembers the answer", async () => {
    show(testDevice());
    expect(screen.getByTestId("loader-question")).toHaveTextContent("Which OPL do you start with this device?");
    await userEvent.click(screen.getByTestId("loader-answer-unknown"));
    expect(readSettings().profileOverrides).toEqual({ [KEY]: "unknown" });
  });

  it("an answer is a profile for this device", async () => {
    show(testDevice());
    await userEvent.click(screen.getByTestId("loader-answer-RiptOpl"));
    expect(readSettings().profileOverrides).toEqual({ [KEY]: "RiptOpl" });
  });

  it("asks an empty folder read over SMB about the OPL on the console", () => {
    show(testDevice({ mode: smbMode, loader: loader({ detected: detected({ library: false }) }) }));
    const question = screen.getByTestId("loader-question");
    expect(question).toHaveTextContent("Which OPL reads this folder over the network?");
    expect(question).toHaveTextContent("ZSO images and JPG pictures");
  });

  it("asks a memory card emulator's SD about builds with MMCE only (ADR-0048)", async () => {
    show(testDevice({ mode: emulatorMode, emulator: emulatorInfo, loader: loader({ detected: detected({ library: false }) }) }));
    const question = screen.getByTestId("loader-question");
    expect(question).toHaveTextContent("Which OPL with MMCE do you start?");
    expect(question).toHaveTextContent("upstream OPL does not");
    expect(screen.getByTestId("loader-answer-Opl120Beta")).toHaveTextContent("MMCE fork, YAOPL or wOPL");
    expect(screen.queryByTestId("loader-answer-Opl110")).toBeNull();
    expect(screen.queryByTestId("loader-answer-unknown")).toBeNull();
    await userEvent.click(screen.getByTestId("loader-answer-unknownMmce"));
    expect(readSettings().profileOverrides).toEqual({ [KEY]: "unknownMmce" });
  });

  it("does not ask when the core does not, or on a device unusable for OPL", () => {
    const error = { level: "error", code: "fsUnreadable", message: "OPL cannot read NTFS" } as const;
    for (const device of [testDevice({ askProfile: false }), testDevice({ advice: [error] })]) {
      const { container, unmount } = show(device);
      expect(container).toBeEmptyDOMElement();
      unmount();
    }
  });

  it("offers auto again when the device's files contradict the choice", async () => {
    window.localStorage.setItem("yaoplm.settings", JSON.stringify({ profileOverrides: { [KEY]: "Opl110" } }));
    show(
      testDevice({
        askProfile: false,
        loader: loader({
          profile: "Opl110",
          source: "override",
          detected: detected({
            profile: "RiptOpl",
            confidence: "indirect",
            reasonText: "APPS/APP_RIPTOPL/settings_riptopl.cfg, which only RiptOPL writes",
            reason: { key: "", params: {} },
          }),
        }),
      }),
    );
    expect(screen.getByTestId("loader-conflict")).toHaveTextContent("RiptOPL");
    await userEvent.click(screen.getByTestId("loader-back-to-auto"));
    expect(readSettings().profileOverrides).toEqual({});
  });

  it("stays quiet when the choice agrees with the files", () => {
    const { container } = show(
      testDevice({
        askProfile: false,
        loader: loader({ profile: "Opl110", source: "override", detected: detected({ profile: "Opl110", confidence: "exact" }) }),
      }),
    );
    expect(container).toBeEmptyDOMElement();
  });
});
