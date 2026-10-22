import { ListChecks } from "lucide-react";
import { useTranslation } from "react-i18next";

import { EmptyState } from "@/components/EmptyState";

export function JobsPage() {
  const { t } = useTranslation();
  return <EmptyState icon={ListChecks} title={t("jobs.emptyTitle")} description={t("jobs.emptyDescription")} />;
}
