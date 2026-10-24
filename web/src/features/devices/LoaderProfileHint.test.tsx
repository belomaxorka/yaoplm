import "@/i18n";

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import type { ActiveProfile } from "@/api/generated/ActiveProfile";
import type { DetectedProfile } from "@/api/generated/DetectedProfile";
import { SettingsProvider } from "@/settings/SettingsProvider";
import { readSettings } from "@/settings/settings";

import { LoaderProfileHint } from "./LoaderProfile";

const KEY = "serial:7E1CD2C5";

function detected(patch: Partial<DetectedProfile> = {}): DetectedProfile {
  return {
    profile: "Opl120Beta",
    confidence: "default",
    reason: { key: "reasons.profileNoSigns", params: {} },
    reasonText: "no signs of a loader on the device",
    loaders: [],
    notices: [],
    library: true,
    ...patch,
  };
}

function loader(patch: Partial<ActiveProfile> = {}): ActiveProfile {
  return { profile: "Opl120Beta", source: "detected", key: KEY, detected: detected(), ...patch };
}

function show(active: ActiveProfile, usable = true) {
  return render(
    <SettingsProvider>
      <LoaderProfileHint loader={active} usable={usable} />
    </SettingsProvider>,
  );
}

describe("the loader profile question (ADR-0041)", () => {
  afterEach(() => window.localStorage.clear());

  it("asks on a prepared device without signs and remembers the answer", async () => {
    show(loader());
    expect(screen.getByTestId("loader-question")).toBeVisible();
    await userEvent.click(screen.getByTestId("loader-answer-unknown"));
    expect(readSettings().profileOverrides).toEqual({ [KEY]: "unknown" });
  });

  it("an answer is a profile for this device", async () => {
    show(loader());
    await userEvent.click(screen.getByTestId("loader-answer-RiptOpl"));
    expect(readSettings().profileOverrides).toEqual({ [KEY]: "RiptOpl" });
  });

  it("does not ask an empty or foreign device, one with signs, or one unusable for OPL", () => {
    const cases: [ActiveProfile, boolean][] = [
      [loader({ detected: detected({ library: false }) }), true],
      [loader({ detected: detected({ confidence: "indirect" }) }), true],
      [loader({ detected: null }), true],
      [loader(), false],
    ];
    for (const [active, usable] of cases) {
      const { container, unmount } = show(active, usable);
      expect(container).toBeEmptyDOMElement();
      unmount();
    }
  });

  it("offers auto again when the device's files contradict the choice", async () => {
    window.localStorage.setItem("yaoplm.settings", JSON.stringify({ profileOverrides: { [KEY]: "Opl110" } }));
    show(
      loader({
        profile: "Opl110",
        source: "override",
        detected: detected({
          profile: "RiptOpl",
          confidence: "indirect",
          reasonText: "APPS/APP_RIPTOPL/settings_riptopl.cfg, which only RiptOPL writes",
          reason: { key: "", params: {} },
        }),
      }),
    );
    expect(screen.getByTestId("loader-conflict")).toHaveTextContent("RiptOPL");
    await userEvent.click(screen.getByTestId("loader-back-to-auto"));
    expect(readSettings().profileOverrides).toEqual({});
  });

  it("stays quiet when the choice agrees with the files", () => {
    const { container } = show(
      loader({ profile: "Opl110", source: "override", detected: detected({ profile: "Opl110", confidence: "exact" }) }),
    );
    expect(container).toBeEmptyDOMElement();
  });
});
