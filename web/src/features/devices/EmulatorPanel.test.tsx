import "@/i18n";

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import i18n from "@/i18n";
import { SettingsProvider } from "@/settings/SettingsProvider";

import { EmulatorPanel } from "./EmulatorPanel";
import { emulatorInfo } from "./testDevice";

describe("the emulator's section of a device card (ADR-0048)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("names itself by an id of its own on a page with two SDs", () => {
    render(
      <SettingsProvider>
        <EmulatorPanel emulator={emulatorInfo} />
        <EmulatorPanel emulator={{ ...emulatorInfo, folder: "Games" }} />
      </SettingsProvider>,
    );
    const panels = screen.getAllByTestId("emulator-panel");
    const ids = panels.map((panel) => panel.getAttribute("aria-labelledby"));
    expect(new Set(ids).size).toBe(2);
    for (const panel of panels) {
      expect(panel).toHaveAccessibleName("Memory card emulator SD");
    }
  });
});
