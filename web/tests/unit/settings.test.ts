import { afterEach, describe, expect, it } from "vitest";

import { profileChoice, readSettings } from "@/settings/settings";

const save = (value: object) => window.localStorage.setItem("yaoplm.settings", JSON.stringify(value));

describe("the loader profile setting (LIB-1)", () => {
  afterEach(() => window.localStorage.clear());

  it("is auto by default", () => {
    expect(readSettings().loaderProfile).toBe("auto");
    expect(readSettings().profileOverrides).toEqual({});
  });

  it("turns the old saved default into auto and keeps a deliberate choice", () => {
    save({ profile: "Opl120Beta" });
    expect(readSettings().loaderProfile).toBe("auto");
    save({ profile: "Opl110" });
    expect(readSettings().loaderProfile).toBe("Opl110");
    save({ profile: "RiptOpl" });
    expect(readSettings().loaderProfile).toBe("RiptOpl");
    // The new key wins over the old one.
    save({ profile: "Opl110", loaderProfile: "auto" });
    expect(readSettings().loaderProfile).toBe("auto");
  });

  it("keeps only valid overrides", () => {
    save({ profileOverrides: { "serial:7E1CD2C5": "Opl110", "path:C:\\PS2": "Nope", "serial:1": 3 } });
    expect(readSettings().profileOverrides).toEqual({ "serial:7E1CD2C5": "Opl110" });
  });

  it("asks the core with the setting only when it is not auto", () => {
    expect(profileChoice({ loaderProfile: "auto", profileOverrides: { "serial:A": "RiptOpl" } })).toEqual({
      overrides: [{ key: "serial:A", profile: "RiptOpl" }],
    });
    expect(profileChoice({ loaderProfile: "Opl110", profileOverrides: {} })).toEqual({ profile: "Opl110", overrides: [] });
  });
});
