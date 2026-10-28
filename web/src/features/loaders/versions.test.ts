import { describe, expect, it } from "vitest";

import { commitOf, compareUrl, versionLabel, versionShort } from "./versions";

describe("loader versions", () => {
  it("shortens numbered builds and keeps the rest", () => {
    expect(versionLabel("v1.2.0-Beta-2271-b3a2199")).toBe("v1.2.0-Beta-2271 (b3a2199)");
    expect(versionLabel("v2.0.0-nightly.12-0123abc-yaopl")).toBe("v2.0.0-nightly.12 (0123abc)");
    expect(versionShort("v2.0.0-nightly.12-0123abc-yaopl-nosio2man")).toBe("v2.0.0-nightly.12");
    for (const version of ["v1.1.0", "v2.0.0-yaopl", "v2.0.0-rc1-yaopl"]) {
      expect(versionLabel(version)).toBe(version);
      expect(versionShort(version)).toBe(version);
      expect(commitOf(version)).toBeNull();
    }
  });

  it("compares YAOPL's beta-numbered build with its nightly", () => {
    expect(compareUrl("yaopl/yaopl", "v1.2.0-Beta-2420-03edb47-yaopl", "v2.0.0-nightly.3-0123abc-yaopl")).toBe(
      "https://github.com/yaopl/yaopl/compare/03edb47...0123abc",
    );
    expect(compareUrl("yaopl/yaopl", "v2.0.0-nightly.3-0123abc-yaopl", "v2.0.0-yaopl")).toBeNull();
  });
});
