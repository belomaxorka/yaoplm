import "@/i18n";

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { testDevice } from "@/features/devices/testDevice";
import { storeSettings } from "@/features/loaders/fixtures";
import { Providers } from "@/features/loaders/TestProviders";
import { RecoveryBanner } from "@/features/recovery/RecoveryBanner";
import i18n from "@/i18n";

/** A core whose device carries one unfinished journal. */
function core() {
  const info = testDevice({ root: "E:\\", unfinished: ["20261026-120000-a1b2"] });
  return new MockTransport((request: Request): Response => {
    switch (request.type) {
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "InspectDevice":
        return { type: "Device", ...info };
      default:
        throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
    }
  });
}

describe("the recovery banner (release pass 1.13)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  // One main button (UI-6): finishing the write is the usual way out;
  // undoing stays beside it. The journal's name is a detail.
  for (const simple of [true, false]) {
    it(`leads with Finish, ${simple ? "and keeps the journal's name out of simple mode" : "naming the journal in advanced mode"}`, async () => {
      storeSettings({ root: "E:\\", simpleMode: simple });
      render(
        <Providers transport={core()}>
          <RecoveryBanner root="E:\\" />
        </Providers>,
      );
      const finish = await screen.findByRole("button", { name: "Finish" });
      expect(finish).toHaveAttribute("data-variant", "default");
      expect(screen.getByRole("button", { name: "Undo" })).toHaveAttribute("data-variant", "outline");
      if (simple) expect(screen.queryByText("20261026-120000-a1b2")).toBeNull();
      else expect(screen.getByText("20261026-120000-a1b2")).toBeInTheDocument();
    });
  }
});
