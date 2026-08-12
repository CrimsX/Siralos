import { GODOT_LIMITS } from "@siralos/core";
import { LSPFrameParser, frameMessage } from "./frame-parser.js";

/**
 * Bounded JSON-RPC 2.0 client for the LSP channel.
 *
 * Implements requests, responses, notifications, and server-initiated
 * requests over an abstract byte sink. Pending requests are bounded and
 * never reuse an id while pending; requests time out and are cancellable
 * (cancelled requests also send `$/cancelRequest`); late and duplicate
 * responses for unknown ids are ignored safely; malformed responses are
 * reported to the protocol-error handler without crashing Siralos;
 * notifications never block request handling; and server-initiated
 * requests are dispatched to a single allowlisted handler that returns
 * MethodNotFound for anything unsupported.
 */

export interface JSONRPCErrorObject {
  readonly code: number;
  readonly message: string;
  readonly data?: unknown;
}

export interface JSONRPCResponse {
  readonly id: number | string;
  readonly result?: unknown;
  readonly error?: JSONRPCErrorObject;
}

export const JSONRPC_CODES = {
  parseError: -32700,
  invalidRequest: -32600,
  methodNotFound: -32601,
  invalidParams: -32602,
  internalError: -32603,
  requestCancelled: -32800,
  contentModified: -32801,
} as const;

export interface JSONRPCClientOptions {
  readonly requestTimeoutMs?: number;
  readonly maxPending?: number;
}

export type JSONRPCServerRequestHandler = (method: string, params: unknown) => Promise<unknown>;

/**
 * The client dispatches incoming messages through this interface so the
 * session host can subscribe to diagnostics notifications and protocol
 * failures without coupling transport details into session models.
 */
export interface JSONRPCMessageDispatch {
  onNotification(method: string, params: unknown): void;
  onProtocolError(message: string): void;
  onUnknownResponse(id: number | string): void;
}

export class JSONRPCClient {
  private readonly sink: (bytes: Uint8Array) => void;
  private readonly requestTimeoutMs: number;
  private readonly maxPending: number;
  private readonly parser: LSPFrameParser;
  private readonly dispatch: JSONRPCMessageDispatch | null;
  private nextId = 1;
  private closed = false;
  private serverRequestHandler: JSONRPCServerRequestHandler | null = null;
  private readonly pending = new Map<
    number,
    {
      readonly method: string;
      resolve(response: JSONRPCResponse): void;
      reject(error: Error): void;
      timer: ReturnType<typeof setTimeout>;
    }
  >();

  constructor(
    sink: (bytes: Uint8Array) => void,
    options: JSONRPCClientOptions & { readonly dispatch?: JSONRPCMessageDispatch } = {},
  ) {
    this.sink = sink;
    this.requestTimeoutMs = options.requestTimeoutMs ?? GODOT_LIMITS.lspRequestTimeoutMs;
    this.maxPending = options.maxPending ?? GODOT_LIMITS.lspMaxPendingRequests;
    this.dispatch = options.dispatch ?? null;
    this.parser = new LSPFrameParser();
  }

  /** Feed raw bytes received from the peer. */
  feed(chunk: Uint8Array): void {
    if (this.closed) {
      return;
    }
    for (const outcome of this.parser.feed(chunk)) {
      if (!outcome.ok) {
        this.dispatch?.onProtocolError(outcome.error.message);
        continue;
      }
      this.handlePayload(outcome.frame.payload);
    }
  }

  /** Send a request; resolves with the correlated response. */
  request(method: string, params: unknown, signal?: AbortSignal): Promise<JSONRPCResponse> {
    if (this.closed) {
      return Promise.reject(new Error("The LSP connection is closed."));
    }
    if (this.pending.size >= this.maxPending) {
      return Promise.reject(
        new Error(`The LSP pending-request bound of ${this.maxPending} was reached.`),
      );
    }
    const id = this.nextId;
    this.nextId += 1;
    if (signal?.aborted) {
      return Promise.reject(createAbortError());
    }
    return new Promise<JSONRPCResponse>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`The LSP request ${method} timed out.`));
      }, this.requestTimeoutMs);
      this.pending.set(id, { method, resolve, reject, timer });
      this.sendMessage({ jsonrpc: "2.0", id, method, params });
      const onAbort = (): void => {
        if (!this.pending.has(id)) {
          return;
        }
        this.pending.delete(id);
        clearTimeout(timer);
        // Notify the server per the LSP cancellation protocol.
        this.sendMessage({ jsonrpc: "2.0", method: "$/cancelRequest", params: { id } });
        reject(createAbortError());
      };
      if (signal !== undefined) {
        if (signal.aborted) {
          onAbort();
          return;
        }
        signal.addEventListener("abort", onAbort, { once: true });
      }
    });
  }

  /** Send a notification; no response is expected or tracked. */
  notify(method: string, params: unknown): void {
    if (this.closed) {
      return;
    }
    this.sendMessage({ jsonrpc: "2.0", method, params });
  }

  /**
   * Handle server-initiated requests. Only the handler's explicit
   * allowlist is honored; unsupported methods return MethodNotFound.
   */
  onServerRequest(handler: JSONRPCServerRequestHandler): void {
    this.serverRequestHandler = handler;
  }

  get pendingCount(): number {
    return this.pending.size;
  }

  /** Reject all pending requests; further sends are refused. */
  close(): void {
    if (this.closed) {
      return;
    }
    this.closed = true;
    for (const entry of this.pending.values()) {
      clearTimeout(entry.timer);
      entry.reject(new Error("The LSP connection closed before the response arrived."));
    }
    this.pending.clear();
  }

  private handlePayload(payload: Uint8Array): void {
    let message: unknown;
    try {
      message = JSON.parse(Buffer.from(payload).toString("utf8"));
    } catch {
      this.dispatch?.onProtocolError("The LSP peer sent invalid JSON.");
      return;
    }
    if (typeof message !== "object" || message === null || Array.isArray(message)) {
      this.dispatch?.onProtocolError("The LSP peer sent a non-object JSON-RPC message.");
      return;
    }
    const record = message as Record<string, unknown>;
    if (typeof record["method"] === "string" && record["id"] !== undefined) {
      void this.handleServerRequest(record);
      return;
    }
    if (typeof record["method"] === "string") {
      const params = record["params"];
      this.dispatch?.onNotification(record["method"], params);
      return;
    }
    if (record["id"] !== undefined) {
      this.handleResponse(record);
      return;
    }
    this.dispatch?.onProtocolError("The LSP peer sent an unidentifiable JSON-RPC message.");
  }

  private handleServerRequest(record: Record<string, unknown>): void {
    const id = record["id"];
    const method = record["method"] as string;
    const params = record["params"];
    if (this.serverRequestHandler === null) {
      this.replyError(id, JSONRPC_CODES.methodNotFound, "No server-request handler is registered.");
      return;
    }
    this.serverRequestHandler(method, params).then(
      (result) => {
        this.sendMessage({ jsonrpc: "2.0", id, result });
      },
      (error: unknown) => {
        const message = error instanceof Error ? error.message : String(error);
        const code =
          error instanceof ServerRequestRejectedError
            ? JSONRPC_CODES.methodNotFound
            : JSONRPC_CODES.internalError;
        this.replyError(id, code, message);
      },
    );
  }

  private handleResponse(record: Record<string, unknown>): void {
    const id = record["id"];
    if (typeof id !== "number" && typeof id !== "string") {
      this.dispatch?.onProtocolError("The LSP peer sent a response with an invalid id.");
      return;
    }
    const numericId = typeof id === "number" ? id : null;
    const pending = numericId === null ? undefined : this.pending.get(numericId);
    if (pending === undefined) {
      // Late or duplicate response: ignored safely, never resolved twice.
      this.dispatch?.onUnknownResponse(id);
      return;
    }
    clearTimeout(pending.timer);
    this.pending.delete(numericId as number);
    pending.resolve({
      id,
      ...(record["result"] !== undefined ? { result: record["result"] } : {}),
      ...(record["error"] !== undefined && isErrorObject(record["error"])
        ? { error: record["error"] }
        : {}),
    });
  }

  private replyError(id: unknown, code: number, message: string): void {
    this.sendMessage({ jsonrpc: "2.0", id, error: { code, message } });
  }

  private sendMessage(message: unknown): void {
    this.sink(frameMessage(JSON.stringify(message)));
  }
}

/** Thrown by server-request handlers to reject a request safely. */
export class ServerRequestRejectedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ServerRequestRejectedError";
  }
}

function isErrorObject(value: unknown): value is JSONRPCErrorObject {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>)["code"] === "number" &&
    typeof (value as Record<string, unknown>)["message"] === "string"
  );
}

function createAbortError(): Error {
  return new DOMException("The LSP request was aborted.", "AbortError");
}
