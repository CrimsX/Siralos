import type { ResearchDocument, ResearchSourceRef } from "./research-model.js";

export interface ResearchEvidence {
  readonly evidenceId: string;
  readonly requestId: string;
  readonly taskId: string;
  readonly taskContractRevision: number;
  readonly source: ResearchSourceRef;
  readonly fetchedAtMs: number;
  readonly resolvedRevision: string | null;
  readonly version: string | null;
  readonly fallback: boolean;
  /** First-section excerpt, bounded by `maxResearchEvidenceExcerptBytes`. */
  readonly excerpt: string;
  readonly truncated: boolean;
  readonly byteLength: number;
}

export type ResearchFetchResult =
  | { readonly status: "refused"; readonly reason: string }
  | {
      readonly status: "document";
      readonly document: ResearchDocument;
      readonly evidence: ResearchEvidence;
    }
  | {
      readonly status:
        | "unsupported-content"
        | "oversized"
        | "timeout"
        | "cancelled"
        | "stale"
        | "unavailable"
        | "failed";
      readonly reason: string;
    };

/** Exact task identity captured around one asynchronous research request. */
export interface ResearchTaskBinding {
  readonly taskId: string;
  readonly taskContractRevision: number;
}
