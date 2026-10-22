import "@/i18n";

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import { MockTransport } from "@/api/transport";
import { App } from "@/App";
import i18n from "@/i18n";

function renderAt(path: string) {
  window.history.pushState({}, "", path);
  return render(<App transport={new MockTransport()} />);
}

describe("App", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    document.documentElement.classList.remove("dark");
  });

  it("shows the empty library", () => {
    renderAt("/");
    expect(screen.getByRole("heading", { level: 1, name: "Library" })).toBeInTheDocument();
    expect(screen.getByText("No library open")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Connected");
  });

  it("switches language to Russian", async () => {
    renderAt("/");
    await userEvent.click(screen.getByRole("radio", { name: "Русский" }));
    expect(screen.getByRole("heading", { level: 1, name: "Библиотека" })).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("ru");
    expect(window.localStorage.getItem("yaoplm.language")).toBe("ru");
  });

  it("toggles the dark theme", async () => {
    renderAt("/");
    await userEvent.click(screen.getByRole("button", { name: "Switch to dark theme" }));
    expect(document.documentElement).toHaveClass("dark");
    expect(window.localStorage.getItem("yaoplm.theme")).toBe("dark");
  });

  it("shows the core version in settings", async () => {
    renderAt("/settings");
    expect(await screen.findByText("0.0.0-mock")).toBeInTheDocument();
    expect(screen.getByText("Demo")).toBeInTheDocument();
  });
});
