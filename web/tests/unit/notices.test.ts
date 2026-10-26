import "@/i18n";

import { renderHook } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { afterEach, describe, expect, it } from "vitest";

import type { PlanNotice } from "@/api/generated/PlanNotice";
import i18n from "@/i18n";
import { useNoticeText } from "@/lib/notices";
import { SettingsContext } from "@/settings/context";
import { defaultSettings } from "@/settings/settings";

// The hook reads the interface mode (simple mode shortens where a picture comes from).
const wrapper = ({ children }: { children: ReactNode }) =>
  createElement(SettingsContext, { value: { settings: defaultSettings, update: () => {} } }, children);

const notice = (code: string, message: string, phrase?: PlanNotice["phrase"]): PlanNotice => ({
  level: "warning",
  code,
  message,
  ...(phrase ? { phrase } : {}),
});

describe("notice texts", () => {
  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("show the phrase with its names and sizes in the current language", async () => {
    const space = notice("noSpace", "Not enough free space: 5.0 GiB needed, 1.0 MiB available", {
      key: "notices.noSpace",
      params: { needed: 5 * 2 ** 30, free: 2 ** 20 },
    });
    const { result, rerender } = renderHook(() => useNoticeText(), { wrapper });
    expect(result.current(space)).toBe("Not enough free space: 5.0 GiB needed, 1.0 MiB available");
    await i18n.changeLanguage("ru");
    rerender();
    expect(result.current(space)).toBe("Не хватает места: нужно 5,0 ГиБ, свободно 1,0 МиБ");
  });

  it("translate a nested reason", async () => {
    await i18n.changeLanguage("ru");
    const { result } = renderHook(() => useNoticeText(), { wrapper });
    const hidden = notice("noStartup", "OPL hides this image: SYSTEM.CNF has no BOOT2 line", {
      key: "notices.noStartup",
      params: { reason: { key: "reasons.noBoot2", params: {} } },
    });
    expect(result.current(hidden)).toBe(
      "OPL скрывает этот образ: в SYSTEM.CNF нет строки BOOT2 (диск PS1, видео-DVD или повреждённый образ)",
    );
  });

  it("show the English message of a notice without a phrase", async () => {
    await i18n.changeLanguage("ru");
    const { result } = renderHook(() => useNoticeText(), { wrapper });
    expect(result.current(notice("noSpace", "not enough free space: 5 bytes needed, 1 available"))).toBe(
      "not enough free space: 5 bytes needed, 1 available",
    );
  });
});
