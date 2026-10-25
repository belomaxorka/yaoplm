// The providers a loader dialog needs, around a mock core (tests only).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { MemoryRouter } from "react-router";

import { ApiProvider } from "@/api/ApiProvider";
import type { MockTransport } from "@/api/transport";
import { TooltipProvider } from "@/components/ui/tooltip";
import { JobsProvider } from "@/jobs/JobsProvider";
import { SettingsProvider } from "@/settings/SettingsProvider";

export function Providers({ transport, children }: { transport: MockTransport; children: ReactNode }) {
  const [queryClient] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  return (
    <QueryClientProvider client={queryClient}>
      <ApiProvider transport={transport}>
        <SettingsProvider>
          <JobsProvider>
            <TooltipProvider>
              <MemoryRouter>{children}</MemoryRouter>
            </TooltipProvider>
          </JobsProvider>
        </SettingsProvider>
      </ApiProvider>
    </QueryClientProvider>
  );
}
