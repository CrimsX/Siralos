/**
 * Structured blocked disposition (Stage 3 milestone 11, ADR 0027).
 *
 * When a unified development task cannot complete, the host produces a
 * typed blocker with a concrete explanation and the list of successful
 * prior changes that are preserved. Completion is never fabricated:
 * an unsupported requirement ends the task honestly as blocked.
 */

export type BlockedReasonKind =
  | "unsupported_serialization"
  | "runtime_verification_required"
  | "mutation_not_representable"
  | "sandbox_unavailable"
  | "repeated_stale_revision"
  | "approval_denied"
  | "validation_gate_unavailable"
  | "repair_budget_exhausted"
  | "infrastructure_unavailable";

export interface BlockedDisposition {
  readonly kind: BlockedReasonKind;
  /** Concrete, bounded explanation of the blocker. */
  readonly detail: string;
  /** Workspace-relative paths of successful prior changes (preserved). */
  readonly preservedChanges: readonly string[];
}

export function createBlockedDisposition(input: {
  readonly kind: BlockedReasonKind;
  readonly detail: string;
  readonly preservedChanges?: readonly string[];
}): BlockedDisposition {
  const detail = input.detail.trim();
  if (detail.length === 0) {
    throw new Error("A blocked disposition requires a concrete explanation.");
  }
  return {
    kind: input.kind,
    detail,
    preservedChanges: [...(input.preservedChanges ?? [])],
  };
}

/** Deterministic single-line reason text for TaskState blocking. */
export function blockedReasonText(disposition: BlockedDisposition): string {
  const preserved =
    disposition.preservedChanges.length === 0
      ? ""
      : ` (preserved changes: ${disposition.preservedChanges.join(", ")})`;
  return `blocked[${disposition.kind}]: ${disposition.detail}${preserved}`;
}
