import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ApiContext, type EventListener } from "@/api/context";
import type { ConnectionStatus, Transport } from "@/api/transport";

export function ApiProvider({ transport, children }: { transport: Transport; children: ReactNode }) {
  const [status, setStatus] = useState<ConnectionStatus>("connecting");
  const listeners = useRef(new Set<EventListener>());

  useEffect(
    () =>
      transport.subscribe({
        onEvent: (event) => {
          for (const listener of listeners.current) listener(event);
        },
        onStatus: setStatus,
      }),
    [transport],
  );

  const addEventListener = useCallback((listener: EventListener) => {
    listeners.current.add(listener);
    return () => {
      listeners.current.delete(listener);
    };
  }, []);

  const value = useMemo(() => ({ transport, status, addEventListener }), [transport, status, addEventListener]);
  return <ApiContext value={value}>{children}</ApiContext>;
}
