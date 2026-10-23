import "@/i18n";

import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { type ConnectionStatus, type EventHandlers, MockTransport, type Transport, type TransportKind } from "@/api/transport";
import { App } from "@/App";
import i18n from "@/i18n";

/** Answers like the mock and lets the test say how the link to the core is. */
class LinkTransport implements Transport {
  readonly address = "nas.local:7878";
  readonly #mock = new MockTransport();
  #handlers: EventHandlers | null = null;

  readonly kind: TransportKind;

  constructor(kind: TransportKind) {
    this.kind = kind;
  }

  call(request: Request): Promise<Response> {
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

  it("shows up in a browser only when the link drops, and blocks the app until it is back", () => {
    const transport = new LinkTransport("http");
    const { container } = render(<App transport={transport} />);
    // Loading is not a loss.
    expect(screen.queryByTestId("connection-lost")).not.toBeInTheDocument();
    transport.set("open");
    expect(screen.queryByText("Connected")).not.toBeInTheDocument();

    transport.set("closed");
    expect(screen.getByRole("alert")).toHaveTextContent(lostText);
    expect(container).toHaveAttribute("inert");
    // Trying again keeps the banner up.
    transport.set("connecting");
    expect(screen.getByTestId("connection-lost")).toBeInTheDocument();

    transport.set("open");
    expect(screen.queryByTestId("connection-lost")).not.toBeInTheDocument();
    expect(container).not.toHaveAttribute("inert");
  });

  it("names a core that cannot be reached when the page opens", () => {
    const transport = new LinkTransport("http");
    render(<App transport={transport} />);
    transport.set("closed");
    expect(screen.getByRole("alert")).toHaveTextContent(lostText);
  });
});
