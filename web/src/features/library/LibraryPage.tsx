import { Disc3 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { EmptyState } from "@/components/EmptyState";

export function LibraryPage() {
  const { t } = useTranslation();
  return <EmptyState icon={Disc3} title={t("library.emptyTitle")} description={t("library.emptyDescription")} />;
}
