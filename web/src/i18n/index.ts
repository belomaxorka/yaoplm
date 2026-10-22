import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import { readStorage, writeStorage } from "@/lib/storage";
import en from "@/locales/en/translation.json";
import ru from "@/locales/ru/translation.json";

export const languages = ["en", "ru"] as const;
export type Language = (typeof languages)[number];

const STORAGE_KEY = "yaoplm.language";

export function isLanguage(value: unknown): value is Language {
  return languages.includes(value as Language);
}

export function detectLanguage(): Language {
  const saved = readStorage(STORAGE_KEY);
  if (isLanguage(saved)) return saved;
  return navigator.language.toLowerCase().startsWith("ru") ? "ru" : "en";
}

export function changeLanguage(language: Language): void {
  writeStorage(STORAGE_KEY, language);
  void i18n.changeLanguage(language);
}

i18n.on("languageChanged", (language) => {
  document.documentElement.lang = language;
});

void i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, ru: { translation: ru } },
  lng: detectLanguage(),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  initAsync: false,
});

export default i18n;
