import "@/i18n";

import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import {
  type ConnectionStatus,
  type EventHandlers,
  MockTransport,
  RpcError,
  type Transport,
  type TransportKind,
} from "@/api/transport";
import { App } from "@/App";
import i18n from "@/i18n";

/** Answers like the mock and lets the test say how the link to the core is. */
class LinkTransport implements Transport {
  readonly address = "nas.local:7878";
  /** Whether requests get through, apart from the event stream. */
  reachable = true;
  readonly #mock = new MockTransport();
  #handlers: EventHandlers | null = null;

  readonly kind: TransportKind;

  constructor(kind: TransportKind) {
    this.kind = kind;
  }

  call(request: Request): Promise<Response> {
    if (!this.reachable) return Promise.reject(new RpcError({ code: "Internal", message: "Failed to fetch" }));
    return this.#mock.call(request);
  }

  subscribe(handlers: EventHandlers): () => void {
    this.#handlers = handlers;
    handlers.onStatus?.("connecting");
    return () => {
      this.#handlers = null;
    };
  }

  set(status: ConnectionStatus): void {
    act(() => this.#handlers?.onStatus?.(status));
  }
}

const lostText = "No connection to YAOPLM at nas.local:7878. Actions are unavailable until it is back.";

describe("the link to the core (plan task 8)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    window.history.pushState({}, "", "/library");
  });

  it("is never shown in the desktop app, where it cannot drop", () => {
    const transport = new LinkTransport("tauri");
    render(<App transport={transport} />);
    transport.set("open");
    expect(screen.queryByText("Connected")).not.toBeInTheDocument();
    transport.set("closed");
    expect(screen.queryByTestId("connection-lost")).not.toBeInTheDocument();
  });

  it("shows up in a browser only when the core stops answering, and blocks the app until it is back", async () => {
    const transport = new LinkTransport("http");
    const { container } = render(<App transport={transport} />);
    // Loading is not a loss.
    expect(screen.queryByTestId("connection-lost")).not.toBeInTheDocument();
    transport.set("open");
    expect(screen.queryByText("Connected")).not.toBeInTheDocument();

    transport.reachable = false;
    transport.set("closed");
    expect(await screen.findByRole("alert", {}, { timeout: 4000 })).toHaveTextContent(lostText);
    // The banner comes from the link check's timer, outside act: the effect
    // that blocks the app may run a moment after it shows.
    await waitFor(() => expect(container).toHaveAttribute("inert"));
    // A dialog opened meanwhile is blocked too.
    const late = document.createElement("div");
    act(() => document.body.append(late));
    await waitFor(() => expect(late).toHaveAttribute("inert"));
    // Trying again keeps the banner up.
    transport.set("connecting");
    expect(screen.getByTestId("connection-lost")).toBeInTheDocument();

    transport.reachable = true;
    transport.set("open");
    expect(screen.queryByTestId("connection-lost")).not.toBeInTheDocument();
    expect(container).not.toHaveAttribute("inert");
    expect(late).not.toHaveAttribute("inert");
    late.remove();
  }, 10_000);

  it("stays away when only the event stream drops and requests still get through", async () => {
    const transport = new LinkTransport("http");
    const { container } = render(<App transport={transport} />);
    transport.set("open");
    transport.set("closed");
    // Past the grace period and the check: the core answered.
    await act(() => new Promise((resolve) => setTimeout(resolve, 2600)));
    expect(screen.queryByTestId("connection-lost")).not.toBeInTheDocument();
    expect(container).not.toHaveAttribute("inert");
  }, 10_000);

  it("names a core that cannot be reached when the page opens", async () => {
    const transport = new LinkTransport("http");
    transport.reachable = false;
    render(<App transport={transport} />);
    transport.set("closed");
    expect(await screen.findByRole("alert", {}, { timeout: 4000 })).toHaveTextContent(lostText);
  }, 10_000);
});
