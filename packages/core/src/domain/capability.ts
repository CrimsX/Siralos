/**
 * Capability request/grant semantics (Stage 3R R6).
 *
 * A domain declares the capabilities it wants before activation. The
 * Host policy decides the effective grant: the grant can only be equal
 * to or narrower than Host authority, and a domain can never grant
 * itself capability. Enablement never implies authority: the authority
 * set is separate Host state and activation still applies policy.
 */

import { invalidInput, type DomainFailure, type ParseResult } from "./failure.js";

/** Maximum number of capability ids in one request or authority set. */
export const MAX_CAPABILITIES = 32;

/** Maximum length of one capability identifier in bytes. */
export const MAX_CAPABILITY_ID_BYTES = 64;

/** A validated canonical capability identifier (`workspace-read`). */
export type CapabilityId = string;

function validCapabilityId(value: string): boolean {
  if (value.length === 0 || value.length > MAX_CAPABILITY_ID_BYTES) {
    return false;
  }
  let previousSeparator = false;
  for (let index = 0; index < value.length; index += 1) {
    const byte = value.charCodeAt(index);
    const separator = byte === 0x2d; // '-'
    const lower = byte >= 0x61 && byte <= 0x7a; // a-z
    const digit = byte >= 0x30 && byte <= 0x39; // 0-9
    if (!(lower || digit || separator) || (separator && (index === 0 || previousSeparator))) {
      return false;
    }
    previousSeparator = separator;
  }
  return !previousSeparator;
}

/** Parse one canonical capability identifier. */
export function parseCapabilityId(value: unknown): ParseResult<CapabilityId> {
  if (typeof value !== "string" || !validCapabilityId(value)) {
    return invalidInput("invalid capability id");
  }
  return { ok: true, value };
}

/** Ordered, deduplicated set of requested capabilities. */
export interface CapabilityRequest {
  /** Canonical (sorted, unique) capability ids. */
  readonly ids: readonly CapabilityId[];
}

/** Parse and canonicalize a request (validation, dedup, sort). */
export function parseCapabilityRequest(values: unknown): ParseResult<CapabilityRequest> {
  if (!Array.isArray(values)) {
    return invalidInput("capability request must be an array");
  }
  if (values.length > MAX_CAPABILITIES) {
    return invalidInput("too many requested capabilities");
  }
  const ids: CapabilityId[] = [];
  for (const value of values) {
    const parsed = parseCapabilityId(value);
    if (!parsed.ok) {
      return parsed;
    }
    if (!ids.includes(parsed.value)) {
      ids.push(parsed.value);
    }
  }
  ids.sort();
  return { ok: true, value: { ids } };
}

/**
 * The effective grant: the capability set the Host actually gives an
 * active domain. Always equal to or narrower than Host authority.
 */
export interface CapabilityGrant {
  /** Canonical (sorted, unique) granted capability ids. */
  readonly ids: readonly CapabilityId[];
}

/**
 * The capability set the Host may grant. This is separate Host state:
 * enablement, installation, and activation never widen it.
 */
export interface HostAuthority {
  /** Canonical (sorted, unique) authority capability ids. */
  readonly ids: readonly CapabilityId[];
}

/** Parse and canonicalize a Host authority set. */
export function parseHostAuthority(values: unknown): ParseResult<HostAuthority> {
  if (!Array.isArray(values)) {
    return invalidInput("authority must be an array");
  }
  if (values.length > MAX_CAPABILITIES) {
    return invalidInput("too many authority capabilities");
  }
  const ids: CapabilityId[] = [];
  for (const value of values) {
    const parsed = parseCapabilityId(value);
    if (!parsed.ok) {
      return parsed;
    }
    if (!ids.includes(parsed.value)) {
      ids.push(parsed.value);
    }
  }
  ids.sort();
  return { ok: true, value: { ids } };
}

/**
 * The Host policy decision for one request: granted with the effective
 * grant, or denied with the requested capabilities outside Host
 * authority in canonical order.
 */
export type GrantDecision =
  | { readonly granted: true; readonly grant: CapabilityGrant }
  | { readonly granted: false; readonly missing: readonly CapabilityId[] };

/**
 * The deterministic Host policy: fail closed unless every requested
 * capability is within Host authority. The effective grant equals the
 * request and can never be wider than the authority. A denial is typed
 * and never triggers automatic permission escalation.
 */
export function decideGrant(request: CapabilityRequest, authority: HostAuthority): GrantDecision {
  const missing = request.ids.filter((id) => !authority.ids.includes(id));
  if (missing.length === 0) {
    return { granted: true, grant: { ids: [...request.ids] } };
  }
  return { granted: false, missing };
}

/** A parse result narrowed to a failure (helper for record builders). */
export function failureOf(result: ParseResult<unknown>): DomainFailure | null {
  return result.ok ? null : result.failure;
}
