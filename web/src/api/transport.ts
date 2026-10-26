import { Channel, invoke, isTauri } from "@tauri-apps/api/core";

import type { ApiError } from "@/api/generated/ApiError";
import type { JobEvent } from "@/api/generated/JobEvent";
import type { Phrase } from "@/api/generated/Phrase";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import { readStorage, writeStorage } from "@/lib/storage";

export type TransportKind = "tauri" | "http" | "mock";
export type ConnectionStatus = "connecting" | "open" | "closed";

export interface EventHandlers {
  onEvent: (event: JobEvent) => void;
  onStatus?: (status: ConnectionStatus) => void;
}

/** How the UI reaches the Rust core: Tauri IPC, HTTP + WebSocket, or a mock. */
export interface Transport {
  readonly kind: TransportKind;
  /** Where the core runs, for messages about the link: `host:port` over HTTP. */
  readonly address?: string;
  call(request: Request): Promise<Response>;
  /** Streams job events until the returned function is called. */
  subscribe(handlers: EventHandlers): () => void;
  /**
   * Tells when a request never reached the core (the network, not an
   * answer), until the returned function is called. Only a link that can
   * drop has it.
   */
  onLinkError?(listener: () => void): () => void;
}

export class RpcError extends Error {
  readonly code: ApiError["code"];
  /** The message for the UI's language; see `errorText`. */
  readonly phrase: Phrase | undefined;

  constructor(error: ApiError) {
    super(error.message);
    this.name = "RpcError";
    this.code = error.code;
    this.phrase = error.phrase;
  }
}

function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as ApiError).code === "string" &&
    typeof (value as ApiError).message === "string"
  );
}

function toRpcError(error: unknown): RpcError {
  if (isApiError(error)) return new RpcError(error);
  return new RpcError({ code: "Internal", message: String(error) });
}

export class TauriTransport implements Transport {
  readonly kind = "tauri";

  async call(request: Request): Promise<Response> {
    try {
      return await invoke<Response>("rpc", { request });
    } catch (error) {
      throw toRpcError(error);
    }
  }

  subscribe({ onEvent, onStatus }: EventHandlers): () => void {
    let active = true;
    const channel = new Channel<JobEvent>((event) => {
      if (active) onEvent(event);
    });
    onStatus?.("connecting");
    invoke("subscribe", { onEvent: channel }).then(
      () => {
        if (active) onStatus?.("open");
      },
      () => {
        if (active) onStatus?.("closed");
      },
    );
    return () => {
      active = false;
    };
  }
}

const MAX_RECONNECT_DELAY_MS = 15_000;

export class HttpTransport implements Transport {
  readonly kind = "http";
  readonly address: string;
  readonly #baseUrl: string;
  readonly #token: string | null;
  readonly #linkListeners = new Set<() => void>();

  constructor(options: { baseUrl?: string; token?: string | null } = {}) {
    this.#baseUrl = options.baseUrl ?? window.location.origin;
    this.#token = options.token ?? null;
    this.address = new URL(this.#baseUrl).host;
  }

  async call(request: Request): Promise<Response> {
    const headers: Record<string, string> = { "content-type": "application/json" };
    if (this.#token) headers.authorization = `Bearer ${this.#token}`;

    let response: globalThis.Response;
    try {
      response = await fetch(new URL("/api/rpc", this.#baseUrl), {
        method: "POST",
        headers,
        body: JSON.stringify(request),
      });
    } catch {
      // The request never reached the core: a browser's "Failed to fetch"
      // names nothing, so the error says where the core is and the link
      // check decides whether it is gone (release pass 1.16).
      for (const listener of this.#linkListeners) listener();
      throw new RpcError({
        code: "Internal",
        message: `No connection to YAOPLM at ${this.address}`,
        phrase: { key: "connection.unreachable", params: { address: this.address } },
      });
    }
    const body: unknown = await response.json().catch(() => null);
    if (!response.ok) {
      throw toRpcError(body ?? { code: "Internal", message: `HTTP ${response.status}` });
    }
    return body as Response;
  }

  onLinkError(listener: () => void): () => void {
    this.#linkListeners.add(listener);
    return () => {
      this.#linkListeners.delete(listener);
    };
  }

  subscribe({ onEvent, onStatus }: EventHandlers): () => void {
    const url = new URL("/api/events", this.#baseUrl);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    // Browsers cannot send headers with a WebSocket handshake.
    if (this.#token) url.searchParams.set("token", this.#token);

    let socket: WebSocket | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let attempt = 0;
    let stopped = false;

    const connect = () => {
      onStatus?.("connecting");
      socket = new WebSocket(url);
      socket.onopen = () => {
        attempt = 0;
        onStatus?.("open");
      };
      socket.onmessage = (message) => {
        if (typeof message.data === "string") onEvent(JSON.parse(message.data) as JobEvent);
      };
      socket.onclose = () => {
        if (stopped) return;
        onStatus?.("closed");
        timer = setTimeout(connect, Math.min(1000 * 2 ** attempt++, MAX_RECONNECT_DELAY_MS));
      };
    };

    connect();
    return () => {
      stopped = true;
      clearTimeout(timer);
      socket?.close();
    };
  }
}

/** Answers requests in memory. Used by tests and component previews. */
export class MockTransport implements Transport {
  readonly kind = "mock";
  readonly #handle: (request: Request) => Response | Promise<Response>;
  readonly #listeners = new Set<(event: JobEvent) => void>();

  constructor(handle: (request: Request) => Response | Promise<Response> = mockResponse) {
    this.#handle = handle;
  }

  async call(request: Request): Promise<Response> {
    return this.#handle(request);
  }

  subscribe({ onEvent, onStatus }: EventHandlers): () => void {
    this.#listeners.add(onEvent);
    onStatus?.("open");
    return () => {
      this.#listeners.delete(onEvent);
    };
  }

  emit(event: JobEvent): void {
    for (const listener of this.#listeners) listener(event);
  }
}

function mockResponse(request: Request): Response {
  switch (request.type) {
    case "Ping":
      return { type: "Pong" };
    case "Version":
      return { type: "Version", name: "YAOPLM", version: "0.0.0-mock" };
    case "ListDevices":
      return { type: "Devices", devices: [] };
    case "ListJobs":
      return { type: "Jobs", jobs: [] };
    case "ScanLibrary":
      return { type: "Job", job: 1 };
    default:
      throw new RpcError({
        code: "NotFound",
        message: `${request.type} is not available in the mock`,
      });
  }
}

const TOKEN_KEY = "yaoplm.token";

/**
 * Server mode: a `?token=` in the address is saved once and removed from the
 * address bar, so it does not end up in bookmarks or screenshots.
 */
export function takeToken(): string | null {
  const url = new URL(window.location.href);
  const fromUrl = url.searchParams.get("token");
  if (fromUrl) {
    writeStorage(TOKEN_KEY, fromUrl);
    url.searchParams.delete("token");
    window.history.replaceState(window.history.state, "", url);
    return fromUrl;
  }
  return readStorage(TOKEN_KEY);
}

export function createTransport(): Transport {
  return isTauri() ? new TauriTransport() : new HttpTransport({ token: takeToken() });
}

/** Calls `request` and checks that the answer has the expected type. */
export async function call<T extends Response["type"]>(
  transport: Transport,
  request: Request,
  expected: T,
): Promise<Extract<Response, { type: T }>> {
  const response = await transport.call(request);
  if (response.type !== expected) {
    throw new RpcError({
      code: "Internal",
      message: `expected a ${expected} response, got ${response.type}`,
    });
  }
  return response as Extract<Response, { type: T }>;
}
