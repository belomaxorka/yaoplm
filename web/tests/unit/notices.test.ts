import "@/i18n";

import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import i18n from "@/i18n";
import { useNoticeText } from "@/lib/notices";

describe("notice texts", () => {
  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("drop a core message that only repeats the label", async () => {
    const notice = {
      code: "idKept",
      message: "game IDs do not change: settings, art, cheats and memory cards keep working",
    };
    for (const language of ["en", "ru"]) {
      await i18n.changeLanguage(language);
      const { result } = renderHook(() => useNoticeText());
      expect(result.current(notice).detail, language).toBeNull();
    }
  });

  it("keep a message with specifics, and show one without a label as is", async () => {
    await i18n.changeLanguage("ru");
    const { result } = renderHook(() => useNoticeText());
    const specific = {
      code: "oldImageDeleted",
      message: "SLUS_200.62.Retail Game.iso is deleted once the ZSO is complete",
    };
    expect(result.current(specific)).toEqual({
      label: "Старый образ удаляется после того, как новый записан полностью",
      detail: specific.message,
    });
    const unknown = { code: "codeFromANewerCore", message: "a message from a newer core" };
    expect(result.current(unknown)).toEqual({ label: unknown.message, detail: null });
  });
});
