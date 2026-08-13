import type { EvidenceKind, EvidenceSource } from "./task-model.js";

const SHA256_PATTERN = /^[0-9a-f]{64}$/;

/**
 * Independently derive whether a bounded evidence source represents a
 * successful observation. Acceptance policy must not trust a claimed
 * verification outcome without checking the underlying structured result.
 */
export function evidenceSourceSupportsSuccessfulOutcome(
  kind: EvidenceKind,
  source: EvidenceSource,
): boolean {
  switch (source.type) {
    case "parser":
      return (
        kind === "parser_result" &&
        source.checkedFiles > 0 &&
        source.errors === 0 &&
        source.validFiles === source.checkedFiles
      );
    case "lsp":
      return kind === "lsp_result" && source.errors === 0;
    case "validation":
      return (
        kind === "validation_result" &&
        source.workspaceIntegrityVerified &&
        source.unexpectedChanges === 0
      );
    case "native_verification":
      return kind === "validation_result" && source.status === "verified";
    case "consistency":
      return kind === "validation_result" && source.consistent;
    case "impact":
      return kind === "validation_result" && source.completeness !== "partial";
    case "review":
      return kind === "review_result" && source.status === "clean" && source.blockingFindings === 0;
    case "user_approval":
      return kind === "user_approval" && source.decision === "approved";
    case "workspace_read":
      return kind === "workspace_read" && source.paths.length > 0;
    case "api_lookup":
      return kind === "api_lookup" && source.symbol.length > 0;
    case "lsp_query":
      return kind === "lsp_query" && source.query.length > 0;
    case "change_preview":
      return kind === "change_preview" && source.changeSetId.length > 0;
    case "mutation":
      return kind === "mutation_receipt" && source.changeSetId.length > 0;
    case "checkpoint":
      return kind === "checkpoint" && source.checkpointId.length > 0;
    case "reference_read":
      return kind === "reference_read" && SHA256_PATTERN.test(source.sha256);
    case "reference_search":
      return kind === "reference_search" && source.matchCount >= 0;
    case "research":
      return kind === "research" && source.requestId.length > 0;
  }
}
