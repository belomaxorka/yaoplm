import { useTranslation } from "react-i18next";

import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { changeLanguage, isLanguage, languages } from "@/i18n";

export function LanguageSwitch({ showNames = false }: { showNames?: boolean }) {
  const { t, i18n } = useTranslation();

  return (
    <ToggleGroup
      type="single"
      variant="outline"
      size="sm"
      spacing={0}
      aria-label={t("settings.language")}
      value={i18n.resolvedLanguage}
      onValueChange={(value) => {
        if (isLanguage(value)) changeLanguage(value);
      }}
    >
      {languages.map((language) => (
        <ToggleGroupItem key={language} value={language} aria-label={t(`languages.${language}`)} lang={language}>
          {showNames ? t(`languages.${language}`) : language.toUpperCase()}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
