import type {
  ResearchBounds,
  ResearchOutcome,
  ResearchRequest,
  ResearchSourceKind,
} from "./research-model.js";

/**
 * Research ports (Stage 3 milestone 5).
 *
 * Core defines the ports; adapters implement them. A research source port
 * fetches one bounded document for one request. The transport port is the
 * provider-neutral HTTP boundary: no HTTP library exists in core, and the
 * transport implementation (adapter) owns redirect handling bounded by
 * `maxRedirects`. Core never performs network or process execution.
 */

export interface ResearchSourcePort {
  readonly kind: ResearchSourceKind;
  readonly id: string;
  /** Human/model-facing label; requests may match a source by label. */
  readonly label: string;
  /**
   * Fetch one document. The source receives the caller-composed signal
   * (caller abort AND service timeout are both reflected), the bounded
   * `bounds`, and must return a typed outcome — never throw.
   */
  fetch(
    request: ResearchRequest,
    bounds: ResearchBounds,
    signal: AbortSignal,
  ): Promise<ResearchOutcome>;
}

/**
 * Transport outcome. Bytes are `Uint8Array` — core imports no Node
 * modules, so the transport contract is expressed in platform types.
 */
export type TransportOutcome =
  | {
      readonly status: "ok";
      readonly statusCode: number;
      readonly contentType: string;
      readonly bytes: Uint8Array;
    }
  | { readonly status: "refused"; readonly reason: string }
  | { readonly status: "timeout" }
  | { readonly status: "cancelled" }
  | {
      readonly status: "unsupported-content";
      readonly contentType: string | null;
      readonly reason: string;
    }
  | { readonly status: "oversized"; readonly reason: string }
  | { readonly status: "failed"; readonly reason: string };

export interface ResearchTransportPort {
  /**
   * Perform one bounded GET. Redirect handling is the transport's job,
   * bounded by `maxRedirects`. `maxBytes` caps the downloaded body;
   * `timeoutMs` caps the whole exchange; `signal` cancels it.
   */
  get(
    url: string,
    options: {
      readonly maxBytes: number;
      readonly maxRedirects: number;
      readonly timeoutMs: number;
      readonly signal: AbortSignal;
    },
  ): Promise<TransportOutcome>;
}
