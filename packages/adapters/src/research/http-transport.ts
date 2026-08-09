import https from "node:https";
import { URL } from "node:url";
import type { ResearchTransportPort, TransportOutcome } from "@solaris/core";
import { boundedErrorMessage, classifyContentType } from "./normalization.js";

/**
 * HTTP transports (Stage 3 milestone 5).
 *
 * `createNodeHttpsTransport` is the real provider-neutral HTTP boundary for
 * research: node:https GET with https-only URLs, bounded redirect following,
 * streaming download caps, timeout, abort, and content-type classification.
 * It NEVER throws — every path returns a typed `TransportOutcome`.
 *
 * `createFakeTransport` is the deterministic, network-free stand-in used by
 * ALL source tests: fixed routes, redirects, delays, and the same
 * maxBytes/redirects/timeout/signal semantics.
 */

const USER_AGENT = "solaris-research/0.1";
const REDIRECT_STATUSES = new Set([301, 302, 303, 307, 308]);

function parseHttpsUrl(
  raw: string,
): { readonly ok: true; readonly url: URL } | { readonly ok: false; readonly reason: string } {
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    return { ok: false, reason: "The URL is malformed." };
  }
  if (parsed.protocol === "http:") {
    return { ok: false, reason: "https only" };
  }
  if (parsed.protocol !== "https:") {
    return { ok: false, reason: "only https URLs are supported" };
  }
  return { ok: true, url: parsed };
}

type SingleExchangeOutcome =
  TransportOutcome | { readonly status: "redirect"; readonly location: string };

function performRequest(
  url: URL,
  options: { readonly maxBytes: number; readonly timeoutMs: number; readonly signal: AbortSignal },
): Promise<SingleExchangeOutcome> {
  return new Promise<SingleExchangeOutcome>((resolve) => {
    let settled = false;
    const timerRef: { current: ReturnType<typeof setTimeout> | null } = { current: null };
    const chunks: Buffer[] = [];
    let received = 0;
    let request: import("node:http").ClientRequest | undefined;
    const finish = (outcome: SingleExchangeOutcome): void => {
      if (settled) {
        return;
      }
      settled = true;
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
      }
      options.signal.removeEventListener("abort", onAbort);
      if (request !== undefined && !request.destroyed) {
        request.destroy();
      }
      resolve(outcome);
    };
    const onAbort = (): void => finish({ status: "cancelled" });
    if (options.signal.aborted) {
      finish({ status: "cancelled" });
      return;
    }
    options.signal.addEventListener("abort", onAbort, { once: true });
    timerRef.current = setTimeout(() => finish({ status: "timeout" }), options.timeoutMs);
    try {
      request = https.get(url, { headers: { "User-Agent": USER_AGENT } }, (response) => {
        const statusCode = response.statusCode ?? 0;
        if (REDIRECT_STATUSES.has(statusCode)) {
          const location = response.headers["location"];
          if (typeof location !== "string" || location.length === 0) {
            finish({ status: "failed", reason: "redirect response without a Location header" });
            return;
          }
          response.resume();
          finish({ status: "redirect", location });
          return;
        }
        response.on("data", (chunk: Buffer) => {
          received += chunk.length;
          if (received > options.maxBytes) {
            finish({
              status: "oversized",
              reason: `response exceeded the download limit of ${options.maxBytes} bytes`,
            });
            return;
          }
          chunks.push(chunk);
        });
        response.on("end", () => {
          const rawContentType =
            typeof response.headers["content-type"] === "string"
              ? response.headers["content-type"]
              : null;
          const classified = classifyContentType(rawContentType);
          if (classified === null) {
            finish({
              status: "unsupported-content",
              contentType: rawContentType,
              reason: `unsupported content type ${rawContentType ?? "(none)"}`,
            });
            return;
          }
          finish({
            status: "ok",
            statusCode,
            contentType: classified,
            bytes: Buffer.concat(chunks),
          });
        });
        response.on("error", (error: Error) =>
          finish({ status: "failed", reason: boundedErrorMessage(error.message) }),
        );
      });
    } catch (error: unknown) {
      finish({ status: "failed", reason: boundedErrorMessage(error) });
      return;
    }
    request.on("error", (error: Error) =>
      finish({ status: "failed", reason: boundedErrorMessage(error.message) }),
    );
  });
}

/**
 * Real node:https transport. HTTPS only (`http://` is refused before any
 * socket opens); redirects followed up to `maxRedirects` (relative Location
 * headers resolved against the request URL; redirects to non-https are
 * refused); the body is capped while streaming (`maxBytes` → `oversized`);
 * `timeoutMs` and the abort `signal` destroy the request (`timeout` /
 * `cancelled`); network errors → `failed` with a bounded reason. Never
 * throws.
 */
export function createNodeHttpsTransport(): ResearchTransportPort {
  return {
    async get(url, options): Promise<TransportOutcome> {
      let current = parseHttpsUrl(url);
      if (!current.ok) {
        return { status: "refused", reason: current.reason };
      }
      if (options.signal.aborted) {
        return { status: "cancelled" };
      }
      let redirects = 0;
      for (;;) {
        const outcome = await performRequest(current.url, {
          maxBytes: options.maxBytes,
          timeoutMs: options.timeoutMs,
          signal: options.signal,
        });
        if (outcome.status !== "redirect") {
          return outcome;
        }
        redirects += 1;
        if (redirects > options.maxRedirects) {
          return { status: "failed", reason: "too many redirects" };
        }
        let resolved: URL;
        try {
          resolved = new URL(outcome.location, current.url);
        } catch {
          return { status: "failed", reason: "redirect Location is malformed" };
        }
        const next = parseHttpsUrl(resolved.href);
        if (!next.ok) {
          return { status: "refused", reason: next.reason };
        }
        current = next;
      }
    },
  };
}

export interface FakeTransportRoute {
  readonly statusCode?: number;
  readonly contentType?: string;
  readonly body?: string;
  readonly redirectsTo?: string;
  /** Simulated latency in milliseconds; the signal is checked after the delay. */
  readonly delayMs?: number;
}

export type FakeTransportRoutes = Readonly<Record<string, FakeTransportRoute>>;

/**
 * Deterministic, network-free transport for tests and the behavior harness.
 * Unknown URLs → `failed` "no route". Redirects follow `redirectsTo` (resolved
 * against the current URL) up to `maxRedirects`; bodies over `maxBytes` →
 * `oversized`; a `delayMs` route whose latency exceeds `timeoutMs` →
 * `timeout`; the abort signal is honored at entry and after any delay →
 * `cancelled`. Content types are classified exactly like the real transport.
 */
export function createFakeTransport(routes: FakeTransportRoutes): ResearchTransportPort {
  return {
    async get(url, options): Promise<TransportOutcome> {
      const current = parseHttpsUrl(url);
      if (!current.ok) {
        return { status: "refused", reason: current.reason };
      }
      if (options.signal.aborted) {
        return { status: "cancelled" };
      }
      let currentUrl = current.url;
      let redirects = 0;
      for (;;) {
        const route = routes[currentUrl.href];
        if (route === undefined) {
          return { status: "failed", reason: "no route" };
        }
        if (route.delayMs !== undefined && route.delayMs > 0) {
          await new Promise<void>((resolve) => setTimeout(resolve, route.delayMs));
          if (options.signal.aborted) {
            return { status: "cancelled" };
          }
          if (route.delayMs > options.timeoutMs) {
            return { status: "timeout" };
          }
        }
        if (route.redirectsTo !== undefined) {
          redirects += 1;
          if (redirects > options.maxRedirects) {
            return { status: "failed", reason: "too many redirects" };
          }
          let resolved: URL;
          try {
            resolved = new URL(route.redirectsTo, currentUrl);
          } catch {
            return { status: "failed", reason: "redirect Location is malformed" };
          }
          const next = parseHttpsUrl(resolved.href);
          if (!next.ok) {
            return { status: "refused", reason: next.reason };
          }
          currentUrl = next.url;
          continue;
        }
        const statusCode = route.statusCode ?? 200;
        const rawContentType = route.contentType ?? "text/markdown";
        const bytes = new TextEncoder().encode(route.body ?? "");
        if (bytes.length > options.maxBytes) {
          return {
            status: "oversized",
            reason: `response exceeded the download limit of ${options.maxBytes} bytes`,
          };
        }
        const classified = classifyContentType(rawContentType);
        if (classified === null) {
          return {
            status: "unsupported-content",
            contentType: rawContentType,
            reason: `unsupported content type ${rawContentType}`,
          };
        }
        return { status: "ok", statusCode, contentType: classified, bytes };
      }
    },
  };
}
