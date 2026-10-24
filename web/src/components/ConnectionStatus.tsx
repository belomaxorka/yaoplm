import { useQueryClient } from "@tanstack/react-query";
import { Loader2, Unplug } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";

/** A dropped event stream is often just that; the core is asked after this. */
const LINK_GRACE_MS = 2000;
/** How often an unreachable core is asked again. */
const RECHECK_MS = 5000;

/**
 * The link between this page and the YAOPLM core, the part that reads the
 * library and writes the games; not the PS2, the network or Samba (UI-4,
 * docs/plans/m5-ui-simple-mode.md, task 8).
 *
 * The desktop app runs the core in its own process: the link cannot drop,
 * so nothing is shown. In a browser the core is yaoplm-server on another
 * machine. The event stream closing alone (a proxy timing out an idle
 * socket) is not a loss: requests may still get through, and the stream
 * reconnects by itself. Only when the core does not answer a request either
 * does a banner name it, and everything behind the banner, dialogs opened
 * meanwhile included, takes no input until the stream is back.
 */
export function ConnectionGuard() {
  const { t } = useTranslation();
  const { transport, status } = useApi();
  const queryClient = useQueryClient();
  const banner = useRef<HTMLDivElement>(null);
  // Before the first connection "connecting" is the page loading, not a loss.
  const [seenOpen, setSeenOpen] = useState(false);
  if (status === "open" && !seenOpen) setSeenOpen(true);
  const streamDown = transport.kind === "http" && (status === "closed" || (status === "connecting" && seenOpen));
  const [unreachable, setUnreachable] = useState(false);

  useEffect(() => {
    if (!streamDown) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const check = async () => {
      let reachable = false;
      try {
        await transport.call({ type: "Ping" });
        reachable = true;
      } catch {
        // The core or the network is gone: the banner says so.
      }
      if (stopped) return;
      setUnreachable(!reachable);
      timer = setTimeout(() => void check(), RECHECK_MS);
    };
    timer = setTimeout(() => void check(), LINK_GRACE_MS);
    return () => {
      stopped = true;
      clearTimeout(timer);
      setUnreachable(false);
    };
  }, [streamDown, transport]);

  const lost = streamDown && unreachable;

  useEffect(() => {
    if (!lost) return;
    const blocked = new Set<Element>();
    const block = (element: Element) => {
      if (element === banner.current || element.hasAttribute("inert")) return;
      element.setAttribute("inert", "");
      blocked.add(element);
    };
    for (const element of document.body.children) block(element);
    // A dialog a finished job opens meanwhile gets a portal of its own.
    const observer = new MutationObserver((changes) => {
      for (const change of changes) for (const node of change.addedNodes) if (node instanceof Element) block(node);
    });
    observer.observe(document.body, { childList: true });
    return () => {
      observer.disconnect();
      for (const element of blocked) element.removeAttribute("inert");
      // A real outage: the server may have restarted, or the device changed.
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
