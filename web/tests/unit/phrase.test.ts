import "@/i18n";

import { afterEach, describe, expect, it } from "vitest";

import type { Phrase } from "@/api/generated/Phrase";
import { RpcError } from "@/api/transport";
import i18n from "@/i18n";
import { errorText, phraseText } from "@/lib/phrase";

const phrase = (key: string, params: Phrase["params"] = {}): Phrase => ({ key, params });

describe("phrases from the core", () => {
  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("pick the plural form and match the core's English", async () => {
    expect(phraseText(phrase("titles.importGames", { count: 1 }), "")).toBe("Import 1 game");
    await i18n.changeLanguage("ru");
    expect(phraseText(phrase("titles.importGames", { count: 1 }), "")).toBe("Добавить 1 игру");
    expect(phraseText(phrase("titles.importGames", { count: 3 }), "")).toBe("Добавить 3 игры");
    expect(phraseText(phrase("titles.importGames", { count: 25 }), "")).toBe("Добавить 25 игр");
  });

  it("translate nested phrases and format sizes", async () => {
    await i18n.changeLanguage("ru");
    const undo = phrase("titles.recoveryUndo", {
      title: phrase("titles.deleteGame", { title: "Retail Game" }),
    });
    expect(phraseText(undo, "")).toBe("Откатить: Удалить Retail Game");
    const space = phrase("errors.noSpace", { needed: 5 * 2 ** 30, free: 2 ** 20 });
    expect(phraseText(space, "")).toBe("Не хватает места: нужно 5,0 ГиБ, свободно 1,0 МиБ");
  });

  it("fall back to the core's English for a key the catalogue lacks", async () => {
    await i18n.changeLanguage("ru");
    expect(phraseText(phrase("titles.fromANewerCore"), "Do something new")).toBe("Do something new");
    expect(phraseText(null, "Import 2 games")).toBe("Import 2 games");
  });

  it("show API errors in the current language", async () => {
    await i18n.changeLanguage("ru");
    const error = new RpcError({
      code: "NotFound",
      message: "Plan 7 expired or already ran",
      phrase: phrase("errors.planExpired", { id: "7" }),
    });
    expect(errorText(error)).toBe("План 7 устарел или уже выполнен");
    expect(errorText({ code: "Internal", message: "boom" })).toBe("boom");
    expect(errorText(new Error("plain"))).toBe("plain");
  });
});
