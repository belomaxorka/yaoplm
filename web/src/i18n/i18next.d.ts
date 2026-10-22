import "i18next";

import type en from "@/locales/en/translation.json";

// Typed keys: t("nav.library") is checked against the English catalogue.
declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: { translation: typeof en };
  }
}
