/**
 * Typed domain failure outcomes (Stage 3R R6).
 *
 * The Host observes failures as typed, stable-coded outcomes so future
 * bounded recovery (R11) can branch on semantics instead of string
 * matching. R6 implements no recovery machinery: a failure may stop or
 * reject an activation, and capability denial never triggers automatic
 * permission escalation.
 */

/** Resource classes for typed resource-exceeded failures (R6). */
export type ResourceKind = "FUEL" | "MEMORY" | "INPUT_BYTES" | "OUTPUT_BYTES" | "HOST_CALLS";

/**
 * Typed Host-observed domain failure. Codes are stable and
 * machine-branchable; recovery never matches display strings.
 */
export type DomainFailure =
  | { readonly code: "NOT_INSTALLED" }
  | { readonly code: "DISABLED" }
  | { readonly code: "ALREADY_INSTALLED" }
  | { readonly code: "ALREADY_ENABLED" }
  | { readonly code: "ALREADY_DISABLED" }
  | { readonly code: "ACTIVE" }
  | { readonly code: "NOT_ACTIVE" }
  | {
      readonly code: "UNSUPPORTED_ABI";
      readonly expected: string;
      readonly found: string;
    }
  | { readonly code: "IDENTITY_MISMATCH"; readonly detail: string }
  | {
      readonly code: "CAPABILITY_DENIED";
      readonly missing: readonly string[];
    }
  | { readonly code: "RESOURCE_EXCEEDED"; readonly kind: ResourceKind }
  | { readonly code: "INVALID_INPUT"; readonly reason: string }
  | { readonly code: "INVALID_OUTPUT"; readonly reason: string }
  | { readonly code: "GUEST_FAULT"; readonly detail: string }
  | { readonly code: "CANCELLED" }
  | { readonly code: "UNAVAILABLE"; readonly reason: string };

/** Stable failure code of a typed failure. */
export function failureCode(failure: DomainFailure): string {
  return failure.code;
}

/**
 * Result of a fallible parse/decision: either the typed value or a
 * typed failure (never an exception for input-driven failures).
 */
export type ParseResult<T> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly failure: DomainFailure };

/** Construct an invalid-input parse result. */
export function invalidInput(reason: string): {
  readonly ok: false;
  readonly failure: DomainFailure;
} {
  return { ok: false, failure: { code: "INVALID_INPUT", reason } };
}
