import { createContext, use } from "react";

import type { ConnectionStatus, Transport } from "@/api/transport";

export interface ApiContextValue {
  transport: Transport;
  status: ConnectionStatus;
}

export const ApiContext = createContext<ApiContextValue | null>(null);

export function useApi(): ApiContextValue {
  const value = use(ApiContext);
  if (!value) throw new Error("useApi must be used inside <ApiProvider>");
  return value;
}
