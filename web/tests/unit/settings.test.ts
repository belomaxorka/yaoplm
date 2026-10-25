import { afterEach, describe, expect, it } from "vitest";

import { deviceChoice, readSettings } from "@/settings/settings";

const save = (value: object) => window.localStorage.setItem("yaoplm.settings", JSON.stringify(value));

describe("the loader profile per device (LIB-1, ADR-0040)", () => {
  afterEach(() => window.localStorage.clear());

  it("has no overrides by default", () => {
    expect(readSettings().profileOverrides).toEqual({});
  });

  it("drops the old global profile, whatever it held", () => {
    for (const old of [{ profile: "Opl110" }, { loaderProfile: "RiptOpl" }, { loaderProfile: "auto" }]) {
      save({ ...old, profileOverrides: { "serial:A": "Opl110" } });
      const settings = readSettings();
      expect(settings).not.toHaveProperty("loaderProfile");
      expect(settings).not.toHaveProperty("profile");
      expect(settings.profileOverrides).toEqual({ "serial:A": "Opl110" });
    }
  });

  it("keeps only valid overrides", () => {
    save({ profileOverrides: { "serial:7E1CD2C5": "Opl110", "path:C:\\PS2": "Nope", "serial:1": 3 } });
    expect(readSettings().profileOverrides).toEqual({ "serial:7E1CD2C5": "Opl110" });
  });

  it("keeps \"I don't know\" and sends it as the rules every OPL reads (ADR-0041)", () => {
    save({ profileOverrides: { "serial:A": "unknown" } });
    expect(readSettings().profileOverrides).toEqual({ "serial:A": "unknown" });
    expect(deviceChoice({ profileOverrides: { "serial:A": "unknown" }, modeOverrides: {} })).toEqual({
      overrides: [{ key: "serial:A", profile: "Opl110" }],
      modeOverrides: [],
    });
  });

  it("asks the core with the overrides only, never a profile for every device", () => {
    expect(deviceChoice({ profileOverrides: { "serial:A": "RiptOpl" }, modeOverrides: {} })).toEqual({
      overrides: [{ key: "serial:A", profile: "RiptOpl" }],
      modeOverrides: [],
    });
    expect(deviceChoice({ profileOverrides: {}, modeOverrides: {} })).toEqual({ overrides: [], modeOverrides: [] });
  });
});

describe("the mode per device (ADR-0045)", () => {
  afterEach(() => window.localStorage.clear());

  it("reads old settings without modes and keeps only valid ones", () => {
    save({ profileOverrides: {} });
    expect(readSettings().modeOverrides).toEqual({});
    save({ modeOverrides: { "path:/srv/ps2": "ps2", "serial:A": "usb", "serial:B": 1 } });
    expect(readSettings().modeOverrides).toEqual({ "path:/srv/ps2": "ps2" });
  });

  it("sends the modes chosen per device by the same key", () => {
    expect(deviceChoice({ profileOverrides: {}, modeOverrides: { "serial:A": "smb" } })).toEqual({
      overrides: [],
      modeOverrides: [{ key: "serial:A", mode: "smb" }],
    });
  });
});
