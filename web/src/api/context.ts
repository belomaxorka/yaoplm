import { createContext, use } from "react";

import type { JobEvent } from "@/api/generated/JobEvent";
import type { ConnectionStatus, Transport } from "@/api/transport";

export type EventListener = (event: JobEvent) => void;

export interface ApiContextValue {
  transport: Transport;
  status: ConnectionStatus;
  /** One event stream per app (Tauri replaces earlier subscriptions), fanned out to listeners. */
  addEventListener: (listener: EventListener) => () => void;
}

export const ApiContext = createContext<ApiContextValue | null>(null);

export function useApi(): ApiContextValue {
  const value = use(ApiContext);
  if (!value) throw new Error("useApi must be used inside <ApiProvider>");
  return value;
}
