import "@/i18n";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import { ApiProvider } from "@/api/ApiProvider";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { MockTransport, RpcError } from "@/api/transport";
import { RedumpPanel } from "@/features/verify/RedumpPanel";
import i18n from "@/i18n";

describe("Redump panel (META-2)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("rewords its error when the language changes", async () => {
    const detail = "https://redump.info/datfile/ps2/serial,version: offline";
    const transport = new MockTransport((request: Request): Response => {
      switch (request.type) {
        case "RedumpStatus":
          return { type: "Redump", dat: null };
        case "UpdateRedump":
          throw new RpcError({
            code: "Internal",
            message: detail,
            phrase: { key: "errors.network", params: { error: detail } },
          });
        default:
          throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
      }
    });
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ApiProvider transport={transport}>
          <RedumpPanel />
        </ApiProvider>
      </QueryClientProvider>,
    );
    await userEvent.click(await screen.findByRole("button", { name: "Download from Redump" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(`Network error: ${detail}`);
    await act(() => i18n.changeLanguage("ru"));
    expect(screen.getByRole("alert")).toHaveTextContent(`Ошибка сети: ${detail}`);
  });
});
