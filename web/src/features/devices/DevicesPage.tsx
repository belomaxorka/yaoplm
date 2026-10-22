import { HardDrive } from "lucide-react";
import { useTranslation } from "react-i18next";

import { EmptyState } from "@/components/EmptyState";

export function DevicesPage() {
  const { t } = useTranslation();
  return <EmptyState icon={HardDrive} title={t("devices.emptyTitle")} description={t("devices.emptyDescription")} />;
}
