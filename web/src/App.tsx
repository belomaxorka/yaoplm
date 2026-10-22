import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router";

import { ApiProvider } from "@/api/ApiProvider";
import type { Transport } from "@/api/transport";
import { AppShell } from "@/components/AppShell";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AddGamesPage } from "@/features/add/AddGamesPage";
import { DevicesPage } from "@/features/devices/DevicesPage";
import { GamePage } from "@/features/game/GamePage";
import { JobsPage } from "@/features/jobs/JobsPage";
import { LibraryPage } from "@/features/library/LibraryPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { JobsProvider } from "@/jobs/JobsProvider";
import { SettingsProvider } from "@/settings/SettingsProvider";
import { ThemeProvider } from "@/theme/ThemeProvider";

export function App({ transport }: { transport: Transport }) {
  const [queryClient] = useState(
    () => new QueryClient({ defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } } }),
  );

  return (
    <QueryClientProvider client={queryClient}>
      <ApiProvider transport={transport}>
        <SettingsProvider>
          <JobsProvider>
            <ThemeProvider>
              <TooltipProvider>
                <BrowserRouter>
                  <Routes>
                    <Route element={<AppShell />}>
                      <Route index element={<Navigate to="/library" replace />} />
                      <Route path="library" element={<LibraryPage />} />
                      <Route path="library/add" element={<AddGamesPage />} />
                      <Route path="library/game/:key" element={<GamePage />} />
                      <Route path="devices" element={<DevicesPage />} />
                      <Route path="jobs" element={<JobsPage />} />
                      <Route path="settings" element={<SettingsPage />} />
                      <Route path="*" element={<Navigate to="/library" replace />} />
                    </Route>
                  </Routes>
                </BrowserRouter>
              </TooltipProvider>
            </ThemeProvider>
          </JobsProvider>
        </SettingsProvider>
      </ApiProvider>
    </QueryClientProvider>
  );
}
