import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router";

import { ApiProvider } from "@/api/ApiProvider";
import type { Transport } from "@/api/transport";
import { AppShell } from "@/components/AppShell";
import { DevicesPage } from "@/features/devices/DevicesPage";
import { JobsPage } from "@/features/jobs/JobsPage";
import { LibraryPage } from "@/features/library/LibraryPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { ThemeProvider } from "@/theme/ThemeProvider";

export function App({ transport }: { transport: Transport }) {
  const [queryClient] = useState(
    () => new QueryClient({ defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } } }),
  );

  return (
    <QueryClientProvider client={queryClient}>
      <ApiProvider transport={transport}>
        <ThemeProvider>
          <BrowserRouter>
            <Routes>
              <Route element={<AppShell />}>
                <Route index element={<LibraryPage />} />
                <Route path="devices" element={<DevicesPage />} />
                <Route path="jobs" element={<JobsPage />} />
                <Route path="settings" element={<SettingsPage />} />
                <Route path="*" element={<Navigate to="/" replace />} />
              </Route>
            </Routes>
          </BrowserRouter>
        </ThemeProvider>
      </ApiProvider>
    </QueryClientProvider>
  );
}
