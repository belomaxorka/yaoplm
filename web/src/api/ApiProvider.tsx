import { type ReactNode, useEffect, useMemo, useState } from "react";

import { ApiContext } from "@/api/context";
import type { ConnectionStatus, Transport } from "@/api/transport";

export function ApiProvider({ transport, children }: { transport: Transport; children: ReactNode }) {
  const [status, setStatus] = useState<ConnectionStatus>("connecting");

  useEffect(
    () =>
      transport.subscribe({
        // Nothing consumes job events yet; the stream drives the status badge.
        onEvent: () => undefined,
        onStatus: setStatus,
      }),
    [transport],
  );

  const value = useMemo(() => ({ transport, status }), [transport, status]);
  return <ApiContext value={value}>{children}</ApiContext>;
}
