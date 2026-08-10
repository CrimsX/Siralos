import { canonicalizeJson, sha256Hex } from "../godot/digest.js";

/**
 * Research source model (Stage 3 milestone 5).
 *
 * Research is bounded, host-coordinated fetching of external reference
 * material (repository docs, Godot documentation) into typed documents
 * with provenance. The ResearchService is the application-owned
 * coordinator: it gates on the capability policy BEFORE any source port is
 * invoked, validates every request, bounds downloads/documents, and
 * produces bounded evidence excerpts. Provider output is untrusted: it is
 * normalized into `ResearchDocument`s under explicit bounds with
 * truncation disclosure, and research never becomes knowledge without an
 * explicit `propose` call.
 */

export type ResearchSourceKind = "repository" | "godot-docs" | "fake";

/** Reference to one configured research source. */
export interface ResearchSourceRef {
  readonly kind: ResearchSourceKind;
  /** Source id (bounded). */
  readonly id: string;
  /** Human label shown to the model (bounded). */
  readonly label: string;
}

export type ResearchContentType = "text/markdown" | "text/plain" | "application/json" | "text/html";

export interface ResearchSection {
  /** Section heading; null when the document has no headings. */
  readonly heading: string | null;
  readonly text: string;
  /** UTF-8 bytes of `text`. */
  readonly byteLength: number;
}

export interface ResearchLink {
  readonly url: string;
  readonly title: string | null;
}

/**
 * Provenance of one fetched document. `requestedRef`/`requestedVersion`
 * are what the caller asked for; `resolvedRevision`/`usedVersion` are what
 * was actually served; `fallback` records when the source served something
 * other than what was requested (with the reason).
 */
export interface ResearchProvenance {
  readonly source: ResearchSourceRef;
  readonly requestedRef: string | null;
  readonly resolvedRevision: string | null;
  readonly requestedVersion: string | null;
  readonly usedVersion: string | null;
  readonly fallback: boolean;
  readonly fallbackReason: string | null;
  readonly fetchedAtMs: number;
  /** Resource identifier within the source (path, page id, ...). */
  readonly resource: string;
}

export type ResearchDocumentId = string & { readonly __researchDocumentId: unique symbol };

export interface ResearchDocument {
  readonly id: ResearchDocumentId;
  readonly source: ResearchSourceRef;
  readonly title: string | null;
  readonly fetchedAtMs: number;
  readonly contentType: ResearchContentType;
  readonly sections: readonly ResearchSection[];
  readonly links: readonly ResearchLink[];
  readonly provenance: ResearchProvenance;
  readonly truncated: boolean;
  readonly truncationReason: string | null;
  readonly byteLength: number;
}

/**
 * Deterministic document id: `rd_` + 24 hex chars from the source id and
 * the request digest. Identical requests to the same source produce the
 * same document id (useful for deduplication and testing).
 */
export function computeResearchDocumentId(
  sourceId: string,
  requestDigest: string,
): ResearchDocumentId {
  const digest = sha256Hex(canonicalizeJson({ sourceId, requestDigest }));
  return `rd_${digest.slice(0, 24)}` as ResearchDocumentId;
}

export interface ResearchRequest {
  readonly source: ResearchSourceRef;
  /** Required: non-empty, ≤ 512 bytes. */
  readonly query: string;
  /** ≤ 256 bytes; null when unused. */
  readonly topic: string | null;
  /** Reference-relative resource path; must be relative with no "..", ≤ 1024 chars. */
  readonly path: string | null;
  /** Git ref pin (commit/tag/branch), ≤ 256 chars. */
  readonly ref: string | null;
  /** Version pin matching `^[0-9]+(\.[0-9]+){0,3}([.-][A-Za-z0-9.-]+)?$`, ≤ 64 chars. */
  readonly version: string | null;
  /** Optional hard download cap override (bounded by `maxDownloadBytes`). */
  readonly maxBytes: number | null;
}

export const RESEARCH_LIMITS = {
  /** Hard cap on bytes a transport may download for one request. */
  maxDownloadBytes: 2 * 1024 * 1024,
  /** Hard cap on the normalized document size. */
  maxDocumentBytes: 256 * 1024,
  maxSections: 64,
  maxLinks: 32,
  maxHeadingBytes: 512,
  maxSectionTextBytes: 32 * 1024,
  maxRedirects: 4,
  timeoutMs: 10_000,
  /** Absolute lifetime of one fetch (defense against adapter stalls). */
  hardLifetimeMs: 30_000,
  /** Excerpt bound for research evidence attached to tasks. */
  maxResearchEvidenceExcerptBytes: 4096,
  /** Retained research evidence views in the service's ring. */
  maxRetainedEvidenceViews: 8,
} as const;

export interface ResearchBounds {
  readonly maxDownloadBytes: number;
  readonly maxDocumentBytes: number;
  readonly maxSections: number;
  readonly maxLinks: number;
  readonly maxHeadingBytes: number;
  readonly maxSectionTextBytes: number;
  readonly maxRedirects: number;
  readonly timeoutMs: number;
  readonly hardLifetimeMs: number;
}

export function defaultResearchBounds(): ResearchBounds {
  return {
    maxDownloadBytes: RESEARCH_LIMITS.maxDownloadBytes,
    maxDocumentBytes: RESEARCH_LIMITS.maxDocumentBytes,
    maxSections: RESEARCH_LIMITS.maxSections,
    maxLinks: RESEARCH_LIMITS.maxLinks,
    maxHeadingBytes: RESEARCH_LIMITS.maxHeadingBytes,
    maxSectionTextBytes: RESEARCH_LIMITS.maxSectionTextBytes,
    maxRedirects: RESEARCH_LIMITS.maxRedirects,
    timeoutMs: RESEARCH_LIMITS.timeoutMs,
    hardLifetimeMs: RESEARCH_LIMITS.hardLifetimeMs,
  };
}

export type ResearchOutcome =
  | { readonly status: "document"; readonly document: ResearchDocument }
  | { readonly status: "refused"; readonly reason: string }
  | { readonly status: "unsupported-content"; readonly reason: string }
  | { readonly status: "oversized"; readonly reason: string }
  | { readonly status: "timeout" }
  | { readonly status: "cancelled" }
  | { readonly status: "unavailable"; readonly reason: string }
  | { readonly status: "failed"; readonly reason: string };

export function isResearchSourceKind(value: unknown): value is ResearchSourceKind {
  return value === "repository" || value === "godot-docs" || value === "fake";
}

/** Structural validation of a source reference (id/label bounds). */
export function isValidResearchSourceRef(value: unknown): value is ResearchSourceRef {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const record = value as Record<string, unknown>;
  if (!isResearchSourceKind(record["kind"])) {
    return false;
  }
  if (typeof record["id"] !== "string" || record["id"].length === 0 || record["id"].length > 128) {
    return false;
  }
  if (
    typeof record["label"] !== "string" ||
    record["label"].length === 0 ||
    record["label"].length > 128
  ) {
    return false;
  }
  return true;
}

const VERSION_PATTERN = /^[0-9]+(\.[0-9]+){0,3}([.-][A-Za-z][A-Za-z0-9.-]*)?$/;

/**
 * Validate an untrusted research request against the bounded model. The
 * service runs this before any source port is invoked.
 */
export function validateResearchRequest(
  input: unknown,
):
  | { readonly ok: true; readonly request: ResearchRequest }
  | { readonly ok: false; readonly reason: string } {
  if (typeof input !== "object" || input === null) {
    return { ok: false, reason: "A research request must be an object." };
  }
  const record = input as Record<string, unknown>;
  const source = record["source"];
  if (!isValidResearchSourceRef(source)) {
    return { ok: false, reason: "The research request requires a valid source (kind, id, label)." };
  }
  const query = record["query"];
  if (typeof query !== "string" || query.trim().length === 0) {
    return { ok: false, reason: "A research request requires a non-empty query." };
  }
  if (new TextEncoder().encode(query).length > 512) {
    return { ok: false, reason: "The research query exceeds the limit of 512 bytes." };
  }
  for (const key of ["topic", "path", "ref", "version"] as const) {
    const value = record[key];
    if (value !== undefined && value !== null && typeof value !== "string") {
      return { ok: false, reason: `The research ${key} must be a string or null.` };
    }
  }
  const topic =
    record["topic"] === null || record["topic"] === undefined ? null : (record["topic"] as string);
  if (topic !== null && new TextEncoder().encode(topic).length > 256) {
    return { ok: false, reason: "The research topic exceeds the limit of 256 bytes." };
  }
  const path =
    record["path"] === null || record["path"] === undefined ? null : (record["path"] as string);
  if (path !== null) {
    if (path.length > 1024) {
      return { ok: false, reason: "The research path exceeds the limit of 1024 characters." };
    }
    if (path.startsWith("/") || path.includes("\\") || path.includes("\0")) {
      return { ok: false, reason: "The research path must be relative with forward slashes." };
    }
    const segments = path.split("/");
    if (segments.some((segment) => segment === ".." || segment === ".")) {
      return { ok: false, reason: 'The research path must not contain ".." or "." segments.' };
    }
  }
  const ref =
    record["ref"] === null || record["ref"] === undefined ? null : (record["ref"] as string);
  if (ref !== null && ref.length > 256) {
    return { ok: false, reason: "The research ref exceeds the limit of 256 characters." };
  }
  const version =
    record["version"] === null || record["version"] === undefined
      ? null
      : (record["version"] as string);
  if (version !== null) {
    if (version.length > 64 || !VERSION_PATTERN.test(version)) {
      return {
        ok: false,
        reason: `The research version "${version}" is malformed; versions look like 4.3 or 4.3-stable.`,
      };
    }
  }
  const maxBytesValue = record["maxBytes"];
  let maxBytes: number | null = null;
  if (maxBytesValue !== undefined && maxBytesValue !== null) {
    if (
      typeof maxBytesValue !== "number" ||
      !Number.isFinite(maxBytesValue) ||
      maxBytesValue <= 0
    ) {
      return { ok: false, reason: "The research maxBytes must be a positive number or null." };
    }
    maxBytes = Math.floor(maxBytesValue);
  }
  return {
    ok: true,
    request: {
      source: { kind: source.kind, id: source.id, label: source.label },
      query,
      topic,
      path,
      ref,
      version,
      maxBytes,
    },
  };
}
