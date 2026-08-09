import { LSPFrameParser, frameMessage } from "../lsp/frame-parser.js";
import { JSONRPC_CODES } from "../lsp/json-rpc.js";

/**
 * Deterministic in-memory LSP wire pair and fake LSP server for tests.
 *
 * Normal transport, JSON-RPC, initialization, document, diagnostics,
 * hover, completion, definition, timeout, and cancellation tests run
 * against this fake server — no real Godot executable is required. The
 * fake server can be scripted to misbehave (unknown response ids,
 * duplicates, malformed JSON, server-initiated requests, slow responses,
 * crashes) so adversarial client behavior is verified deterministically.
 */

export interface LSPWireEndpoint {
  send(bytes: Uint8Array): void;
  onData(callback: (bytes: Uint8Array) => void): void;
  close(): void;
}

export function createInMemoryLSPWirePair(): {
  readonly left: LSPWireEndpoint;
  readonly right: LSPWireEndpoint;
} {
  let leftCallback: ((bytes: Uint8Array) => void) | null = null;
  let rightCallback: ((bytes: Uint8Array) => void) | null = null;
  let leftClosed = false;
  let rightClosed = false;
  const left: LSPWireEndpoint = {
    send(bytes) {
      if (!leftClosed && rightCallback !== null) {
        rightCallback(bytes);
      }
    },
    onData(callback) {
      leftCallback = callback;
    },
    close() {
      leftClosed = true;
    },
  };
  const right: LSPWireEndpoint = {
    send(bytes) {
      if (!rightClosed && leftCallback !== null) {
        leftCallback(bytes);
      }
    },
    onData(callback) {
      rightCallback = callback;
    },
    close() {
      rightClosed = true;
    },
  };
  return { left, right };
}

export interface FakeLSPServerCapabilities {
  readonly textDocumentSync?: number;
  readonly hoverProvider?: boolean;
  readonly completionProvider?: boolean;
  readonly definitionProvider?: boolean;
}

export interface FakeLSPServerOptions {
  readonly capabilities?: FakeLSPServerCapabilities;
  /** Per workspace-relative path diagnostics pushed after didOpen. */
  readonly diagnostics?: Readonly<Record<string, unknown>>;
  /** Per path hover contents. */
  readonly hover?: Readonly<Record<string, unknown>>;
  /** Per path completion lists. */
  readonly completion?: Readonly<Record<string, unknown>>;
  /** Per path definition locations. */
  readonly definition?: Readonly<Record<string, unknown>>;
  readonly onNotification?: (method: string, params: unknown) => void;
}

export interface FakeLSPServer {
  readonly sentRequests: () => { readonly method: string; readonly id: unknown }[];
  readonly openDocuments: () => readonly string[];
  /** Respond to the next request of `method` after `delayMs`. */
  delayResponse(method: string, delayMs: number): void;
  /** Send a response with an id the client never requested. */
  sendUnknownResponse(): void;
  /** Send a second response for the most recent request id. */
  sendDuplicateResponse(): void;
  /** Send raw non-JSON bytes. */
  sendMalformedJson(): void;
  /** Send a server-initiated request. */
  sendServerRequest(method: string, params: unknown): void;
  /** Close the wire as if the server process died. */
  crash(): void;
  close(): void;
}

export function createFakeLSPServer(
  endpoint: LSPWireEndpoint,
  options: FakeLSPServerOptions = {},
): FakeLSPServer {
  const parser = new LSPFrameParser();
  const sentRequests: { method: string; id: unknown }[] = [];
  const openDocuments: string[] = [];
  const delayed = new Map<string, number>();
  let closed = false;
  let lastRequestId: unknown = null;
  let nextRequestId = 1;

  endpoint.onData((bytes) => {
    if (closed) {
      return;
    }
    for (const outcome of parser.feed(bytes)) {
      if (!outcome.ok) {
        continue;
      }
      handlePayload(outcome.frame.payload);
    }
  });

  function handlePayload(payload: Uint8Array): void {
    let message: unknown;
    try {
      message = JSON.parse(Buffer.from(payload).toString("utf8"));
    } catch {
      return;
    }
    if (typeof message !== "object" || message === null) {
      return;
    }
    const record = message as Record<string, unknown>;
    const method = typeof record["method"] === "string" ? record["method"] : null;
    if (method === null) {
      return;
    }
    const id = record["id"];
    const params = record["params"];
    if (id !== undefined) {
      sentRequests.push({ method, id });
      lastRequestId = id;
      const delay = delayed.get(method) ?? 0;
      const respond = (): void => {
        if (closed) {
          return;
        }
        reply(method, id, params);
      };
      if (delay > 0) {
        setTimeout(respond, delay);
      } else {
        respond();
      }
      return;
    }
    if (method === "textDocument/didOpen") {
      const uri = extractUri(params);
      if (uri !== null && !openDocuments.includes(uri)) {
        openDocuments.push(uri);
      }
      const diagnostics = options.diagnostics ?? {};
      for (const [path, payloadValue] of Object.entries(diagnostics)) {
        if (uri !== null && uri.endsWith(encodeURI(path.replace(/\\/g, "/")))) {
          send({
            jsonrpc: "2.0",
            method: "textDocument/publishDiagnostics",
            params: { uri, diagnostics: payloadValue },
          });
        }
      }
      return;
    }
    if (method === "textDocument/didClose") {
      const uri = extractUri(params);
      if (uri !== null) {
        const index = openDocuments.indexOf(uri);
        if (index !== -1) {
          openDocuments.splice(index, 1);
        }
      }
      return;
    }
    options.onNotification?.(method, params);
  }

  function reply(method: string, id: unknown, params: unknown): void {
    switch (method) {
      case "initialize":
        send({
          jsonrpc: "2.0",
          id,
          result: {
            capabilities: options.capabilities ?? {
              textDocumentSync: 1,
              hoverProvider: true,
              completionProvider: true,
              definitionProvider: true,
            },
            serverInfo: { name: "fake-godot-lsp", version: "test" },
          },
        });
        return;
      case "shutdown":
        send({ jsonrpc: "2.0", id, result: null });
        return;
      case "textDocument/hover":
        send({
          jsonrpc: "2.0",
          id,
          result: hoverFor(params, options.hover ?? {}),
        });
        return;
      case "textDocument/completion":
        send({
          jsonrpc: "2.0",
          id,
          result: completionFor(params, options.completion ?? {}),
        });
        return;
      case "textDocument/definition":
        send({
          jsonrpc: "2.0",
          id,
          result: definitionFor(params, options.definition ?? {}),
        });
        return;
      default:
        send({
          jsonrpc: "2.0",
          id,
          error: { code: JSONRPC_CODES.methodNotFound, message: `Method not found: ${method}` },
        });
    }
  }

  function send(message: unknown): void {
    endpoint.send(frameMessage(JSON.stringify(message)));
  }

  return {
    sentRequests: () => [...sentRequests],
    openDocuments: () => [...openDocuments],
    delayResponse(method, delayMs) {
      delayed.set(method, delayMs);
    },
    sendUnknownResponse() {
      send({ jsonrpc: "2.0", id: 999_999, result: null });
    },
    sendDuplicateResponse() {
      send({ jsonrpc: "2.0", id: lastRequestId, result: null });
    },
    sendMalformedJson() {
      endpoint.send(new TextEncoder().encode("Content-Length: 5\r\n\r\n{not"));
    },
    sendServerRequest(method, params) {
      send({ jsonrpc: "2.0", id: `server-${nextRequestId++}`, method, params });
    },
    crash() {
      closed = true;
      endpoint.close();
    },
    close() {
      closed = true;
      endpoint.close();
    },
  };
}

function extractUri(params: unknown): string | null {
  if (typeof params !== "object" || params === null) {
    return null;
  }
  const uri = (params as Record<string, unknown>)["textDocument"];
  if (typeof uri !== "object" || uri === null) {
    return null;
  }
  const value = (uri as Record<string, unknown>)["uri"];
  return typeof value === "string" ? value : null;
}

function hoverFor(params: unknown, hover: Readonly<Record<string, unknown>>): unknown {
  const uri = extractUri(params);
  if (uri === null) {
    return null;
  }
  for (const [path, value] of Object.entries(hover)) {
    if (uri.endsWith(encodeURI(path.replace(/\\/g, "/")))) {
      return value;
    }
  }
  return null;
}

function completionFor(params: unknown, completion: Readonly<Record<string, unknown>>): unknown {
  const uri = extractUri(params);
  if (uri === null) {
    return { isIncomplete: false, items: [] };
  }
  for (const [path, value] of Object.entries(completion)) {
    if (uri.endsWith(encodeURI(path.replace(/\\/g, "/")))) {
      return value;
    }
  }
  return { isIncomplete: false, items: [] };
}

function definitionFor(params: unknown, definition: Readonly<Record<string, unknown>>): unknown {
  const uri = extractUri(params);
  if (uri === null) {
    return [];
  }
  for (const [path, value] of Object.entries(definition)) {
    if (uri.endsWith(encodeURI(path.replace(/\\/g, "/")))) {
      return value;
    }
  }
  return [];
}
