import { useQueryClient } from "@tanstack/react-query";
import { Loader2, Unplug } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";

/**
 * The link between this page and the YAOPLM core, the part that reads the
 * library and writes the games; not the PS2, the network or Samba (UI-4,
 * docs/plans/m5-ui-simple-mode.md, task 8).
 *
 * The desktop app runs the core in its own process: the link cannot drop,
 * so nothing is shown. In a browser the core is yaoplm-server on another
 * machine. When the link drops, a banner names it, and everything behind
 * the banner, open dialogs included, takes no input until it is back: the
 * buttons would do nothing.
 */
export function ConnectionGuard() {
  const { t } = useTranslation();
  const { transport, status } = useApi();
  const queryClient = useQueryClient();
  const banner = useRef<HTMLDivElement>(null);
  // Before the first connection "connecting" is the page loading, not a loss.
  const [seenOpen, setSeenOpen] = useState(false);
  if (status === "open" && !seenOpen) setSeenOpen(true);
  const lost = transport.kind === "http" && (status === "closed" || (status === "connecting" && seenOpen));

  useEffect(() => {
    if (!lost) return;
    const blocked = [...document.body.children].filter(
      (element) => element !== banner.current && !element.hasAttribute("inert"),
    );
    for (const element of blocked) element.setAttribute("inert", "");
    return () => {
      for (const element of blocked) element.removeAttribute("inert");
      // The server may have restarted, or the device changed meanwhile: read again.
      void queryClient.invalidateQueries({ queryKey: ["library"] });
      void queryClient.invalidateQueries({ queryKey: ["device"] });
      void queryClient.invalidateQueries({ queryKey: ["devices"] });
    };
  }, [lost, queryClient]);

  if (!lost) return null;
  return createPortal(
    <div
      ref={banner}
      className="fixed inset-0 z-[60] flex items-start justify-center bg-background/70 p-4 pt-20 backdrop-blur-[2px]"
      data-testid="connection-lost"
    >
      <div
        role="alert"
        className="flex max-w-lg items-start gap-3 rounded-lg border border-destructive/40 bg-popover p-4 text-sm text-popover-foreground shadow-lg"
      >
        <Unplug className="mt-0.5 size-5 shrink-0 text-destructive" aria-hidden />
        <div className="flex flex-col gap-2">
          <p className="font-medium">{t("connection.lost", { address: transport.address ?? window.location.host })}</p>
          <p className="flex items-center gap-2 text-muted-foreground">
            <Loader2 className="size-4 animate-spin" aria-hidden />
            {t("connection.retrying")}
          </p>
        </div>
      </div>
    </div>,
    document.body,
  );
}
