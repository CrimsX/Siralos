import { truncateText } from "../projection/evidence-projector.js";
import type { ReferenceAlias, ReferenceId, ReferenceRevision } from "./reference-model.js";

/**
 * Reference evidence views (Stage 3 milestone 5).
 *
 * A `ReferenceObservation` is the internal record the registry/access
 * layer produces when a reference read/search/list happens; the
 * model-facing `ReferenceEvidenceView` is its bounded projection (never
 * absolute cache paths — `path` is reference-relative). The projection
 * service renders the most recent views into the volatile
 * `[Reference evidence]` context section.
 */

export interface ReferenceObservation {
  readonly referenceId: ReferenceId;
  readonly alias: ReferenceAlias;
  /** Registry-bound revision the observation was made against. */
  readonly revision: ReferenceRevision;
  /** Reference-relative path (forward slashes, no leading slash). */
  readonly path: string;
  readonly operation: "read" | "search" | "list";
  readonly mode: string | null;
  readonly sha256: string | null;
  readonly evidenceId: string | null;
}

/** Model-facing bounded view of one reference observation. */
export type ReferenceEvidenceView = ReferenceObservation;

/**
 * Deterministic anchor of a revision: the resolved commit for repository
 * references, the fingerprint for local directories.
 */
export function referenceIdentityAnchor(revision: ReferenceRevision): string {
  return revision.identity.kind === "repository"
    ? revision.identity.commit
    : revision.identity.fingerprint;
}

/** One-line bounded rendering: `@reference/<alias> @ <anchor> <path> (<operation>[, mode])`. */
export function formatReferenceEvidenceLine(view: ReferenceEvidenceView): string {
  const mode = view.mode === null ? "" : `, ${view.mode}`;
  return `@reference/${view.alias} @ ${referenceIdentityAnchor(view.revision)} ${view.path} (${view.operation}${mode})`;
}

/** Default bound for the two-line reference evidence view. */
export const DEFAULT_REFERENCE_VIEW_MAX_BYTES = 1_024;

/**
 * Pure, bounded model-facing rendering of one reference evidence record:
 *
 * ```
 * @reference/<alias> @ <commit-or-fingerprint> <path> (<operation>[, mode])
 * Evidence: <evidenceId | "-">
 * ```
 *
 * Bounded with `… [truncated]`; the EvidenceProjector pipeline still
 * sanitizes/redacts the final string.
 */
export function formatReferenceEvidenceView(
  view: ReferenceEvidenceView,
  opts: { readonly maxBytes?: number } = {},
): string {
  const text = `${formatReferenceEvidenceLine(view)}\nEvidence: ${view.evidenceId ?? "-"}`;
  const maxBytes = opts.maxBytes ?? DEFAULT_REFERENCE_VIEW_MAX_BYTES;
  return truncateText(text, maxBytes).text;
}
