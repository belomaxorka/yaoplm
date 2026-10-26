import { afterEach, describe, expect, it, vi } from "vitest";

import { call, HttpTransport, MockTransport, RpcError, takeToken } from "@/api/transport";

afterEach(() => {
  vi.unstubAllGlobals();
  window.history.replaceState({}, "", "/");
});

describe("HttpTransport", () => {
  it("posts JSON with the bearer token", async () => {
    const fetchMock = vi.fn(() => Promise.resolve(Response.json({ type: "Pong" })));
    vi.stubGlobal("fetch", fetchMock);

    const transport = new HttpTransport({ baseUrl: "http://127.0.0.1:7878", token: "s3cret" });
    await expect(transport.call({ type: "Ping" })).resolves.toEqual({ type: "Pong" });

    expect(fetchMock).toHaveBeenCalledWith(new URL("http://127.0.0.1:7878/api/rpc"), {
      method: "POST",
      headers: { "content-type": "application/json", authorization: "Bearer s3cret" },
      body: '{"type":"Ping"}',
    });
  });

  // A browser's "Failed to fetch" names nothing (release pass 1.16): the
  // error says where the core is, and the link check hears about it.
  it("names the core's address when a request never gets there, and tells the link check", async () => {
    vi.stubGlobal("fetch", () => Promise.reject(new TypeError("Failed to fetch")));
    const transport = new HttpTransport({ baseUrl: "http://nas.local:7878" });
    const heard = vi.fn();
    transport.onLinkError(heard);

    const error = await transport.call({ type: "Ping" }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(RpcError);
    expect(error).toMatchObject({
      code: "Internal",
      message: "No connection to YAOPLM at nas.local:7878",
      phrase: { key: "connection.unreachable", params: { address: "nas.local:7878" } },
    });
    expect(heard).toHaveBeenCalledTimes(1);
  });

  it("turns API errors into RpcError", async () => {
    vi.stubGlobal("fetch", () =>
      Promise.resolve(Response.json({ code: "Unauthorized", message: "missing token" }, { status: 401 })),
    );
    const transport = new HttpTransport({ baseUrl: "http://127.0.0.1:7878" });

    const error = await transport.call({ type: "Ping" }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(RpcError);
    expect(error).toMatchObject({ code: "Unauthorized", message: "missing token" });
  });
});

describe("call", () => {
  it("rejects an unexpected response type", async () => {
    const transport = new MockTransport(() => ({ type: "Pong" }));
    await expect(call(transport, { type: "Version" }, "Version")).rejects.toThrow(RpcError);
  });
});

describe("takeToken", () => {
  it("stores the URL token and removes it from the address bar", () => {
    window.history.replaceState({}, "", "/settings?token=abc&x=1");
    expect(takeToken()).toBe("abc");
    expect(window.location.search).toBe("?x=1");
    expect(takeToken()).toBe("abc");
  });
});
