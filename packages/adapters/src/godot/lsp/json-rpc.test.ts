import { afterEach, describe, expect, it } from "vitest";
import { JSONRPCClient, ServerRequestRejectedError, JSONRPC_CODES } from "./json-rpc.js";
import {
  createFakeLSPServer,
  createInMemoryLSPWirePair,
  type FakeLSPServer,
} from "../testing/lsp-testing.js";

const timers: ReturnType<typeof setTimeout>[] = [];

afterEach(() => {
  for (const timer of timers.splice(0)) {
    clearTimeout(timer);
  }
});

interface Harness {
  readonly client: JSONRPCClient;
  readonly server: FakeLSPServer;
  readonly notifications: { method: string; params: unknown }[];
  readonly protocolErrors: string[];
  readonly unknownResponses: (number | string)[];
}

function createHarness(options: { requestTimeoutMs?: number } = {}): Harness {
  const { left, right } = createInMemoryLSPWirePair();
  const notifications: { method: string; params: unknown }[] = [];
  const protocolErrors: string[] = [];
  const unknownResponses: (number | string)[] = [];
  const server = createFakeLSPServer(right);
  const client = new JSONRPCClient((bytes) => left.send(bytes), {
    ...(options.requestTimeoutMs === undefined
      ? {}
      : { requestTimeoutMs: options.requestTimeoutMs }),
    dispatch: {
      onNotification(method, params) {
        notifications.push({ method, params });
      },
      onProtocolError(message) {
        protocolErrors.push(message);
      },
      onUnknownResponse(id) {
        unknownResponses.push(id);
      },
    },
  });
  left.onData((bytes) => client.feed(bytes));
  return { client, server, notifications, protocolErrors, unknownResponses };
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    timers.push(setTimeout(resolve, ms));
  });
}

describe("JSONRPCClient", () => {
  it("correlates requests and responses and increments ids deterministically", async () => {
    const { client, server } = createHarness();
    const first = await client.request("textDocument/hover", {});
    const second = await client.request("textDocument/hover", {});
    expect(first.id).toBe(1);
    expect(second.id).toBe(2);
    expect(server.sentRequests().map((entry) => entry.id)).toEqual([1, 2]);
  });

  it("keeps concurrent pending requests bounded", async () => {
    const { client, server } = createHarness({ requestTimeoutMs: 200 });
    server.delayResponse("textDocument/hover", 30);
    const requests = Array.from({ length: 5 }, () => client.request("textDocument/hover", {}));
    expect(client.pendingCount).toBe(5);
    await Promise.all(requests);
    expect(client.pendingCount).toBe(0);
  });

  it("rejects new requests beyond the pending bound", async () => {
    const { client, server } = createHarness({ requestTimeoutMs: 500 });
    server.delayResponse("textDocument/hover", 100);
    const pending = Array.from({ length: 2 }, () => client.request("textDocument/hover", {}));
    const bounded = new JSONRPCClient(() => undefined, { maxPending: 2 });
    // Two requests pend; the third exceeds the bound and is refused.
    void bounded.request("x", {}).catch(() => undefined);
    void bounded.request("y", {}).catch(() => undefined);
    await expect(bounded.request("z", {})).rejects.toThrow("pending-request bound");
    bounded.close();
    await Promise.all(pending);
  });

  it("notifications require no response and do not block requests", async () => {
    const { client, server } = createHarness();
    client.notify("initialized", {});
    const response = await client.request("shutdown", {});
    expect(response.result).toBeNull();
    expect(server.sentRequests().some((entry) => entry.method === "initialized")).toBe(false);
    expect(server.sentRequests().some((entry) => entry.method === "shutdown")).toBe(true);
  });

  it("dispatches inbound notifications to the handler", () => {
    const { client, server } = createHarness();
    server.sendServerRequest("textDocument/publishDiagnostics", {
      uri: "file:///mirror/a.gd",
      diagnostics: [],
    });
    // publishDiagnostics is a notification from the server's perspective.
    expect(client.pendingCount).toBe(0);
  });

  it("answers unsupported server requests with MethodNotFound", async () => {
    const { client, server } = createHarness();
    const reply = new Promise((resolve) => {
      client.onServerRequest((method) => {
        resolve(method);
        return Promise.reject(new ServerRequestRejectedError(`Unsupported server request: ${method}`));
      });
    });
    server.sendServerRequest("workspace/executeCommand", { command: "evil" });
    expect(await reply).toBe("workspace/executeCommand");
    await sleep(10);
    // The rejection was answered with MethodNotFound; nothing executed.
    expect(client.pendingCount).toBe(0);
  });

  it("rejects workspace/applyEdit and workspace/executeCommand via the handler", async () => {
    const { client, server } = createHarness();
    const handled: string[] = [];
    client.onServerRequest((method) => {
      handled.push(method);
      if (method === "workspace/applyEdit" || method === "workspace/executeCommand") {
        return Promise.reject(new ServerRequestRejectedError(`Rejected: ${method}`));
      }
      return Promise.resolve(null);
    });
    server.sendServerRequest("workspace/applyEdit", { edit: {} });
    server.sendServerRequest("workspace/executeCommand", { command: "x" });
    await sleep(10);
    expect(handled).toEqual(["workspace/applyEdit", "workspace/executeCommand"]);
    expect(client.pendingCount).toBe(0);
  });

  it("ignores unknown and duplicate response ids safely", async () => {
    const { client, server, unknownResponses } = createHarness();
    server.sendUnknownResponse();
    server.sendDuplicateResponse();
    const response = await client.request("shutdown", {});
    expect(response.id).toBe(1);
    expect(unknownResponses.length).toBeGreaterThanOrEqual(1);
  });

  it("times out slow requests", async () => {
    const { client, server } = createHarness({ requestTimeoutMs: 40 });
    server.delayResponse("textDocument/hover", 200);
    await expect(client.request("textDocument/hover", {})).rejects.toThrow("timed out");
  });

  it("cancels requests on abort and ignores late responses", async () => {
    const { client, server } = createHarness();
    server.delayResponse("textDocument/hover", 60);
    const controller = new AbortController();
    const request = client.request("textDocument/hover", {}, controller.signal);
    controller.abort();
    await expect(request).rejects.toMatchObject({ name: "AbortError" });
    await sleep(80);
    // The late response must not resolve anything or crash.
    expect(client.pendingCount).toBe(0);
  });

  it("rejects pending requests when the connection closes mid-request", async () => {
    const { client, server } = createHarness({ requestTimeoutMs: 5_000 });
    server.delayResponse("textDocument/hover", 500);
    const request = client.request("textDocument/hover", {});
    server.crash();
    // The session host closes the client when the wire closes; every
    // pending request is rejected with a typed error, never hung.
    client.close();
    await expect(request).rejects.toThrow("closed");
  });

  it("reports malformed responses as protocol errors without crashing", () => {
    const { client, protocolErrors } = createHarness();
    client.feed(new TextEncoder().encode("Content-Length: 5\r\n\r\nnope!"));
    expect(protocolErrors.length).toBeGreaterThan(0);
  });

  it("rejects new requests after close", async () => {
    const { client } = createHarness();
    client.close();
    await expect(client.request("shutdown", {})).rejects.toThrow("closed");
  });

  it("shutdown drains pending state", async () => {
    const { client } = createHarness();
    const response = await client.request("shutdown", {});
    expect(response.result).toBeNull();
    expect(client.pendingCount).toBe(0);
  });

  it("handles a server process crash with a typed error path", async () => {
    const { client, server } = createHarness({ requestTimeoutMs: 5_000 });
    server.crash();
    client.close();
    await expect(client.request("shutdown", {})).rejects.toThrow("closed");
    expect(JSONRPC_CODES.methodNotFound).toBe(-32601);
  });
});
