import { truncateText } from "../projection/evidence-projector.js";
import { RESEARCH_LIMITS, type ResearchDocument } from "./research-model.js";
import type { ResearchEvidence, ResearchTaskBinding } from "./research-service-model.js";

export interface ResearchEvidenceStore {
  record(
    document: ResearchDocument,
    requestId: string,
    task: ResearchTaskBinding,
  ): ResearchEvidence;
  snapshots(): readonly ResearchEvidence[];
}

/** Bounded FIFO retention for model-facing research excerpts. */
export function createResearchEvidenceStore(maxEvidenceBytes: number): ResearchEvidenceStore {
  const entries: ResearchEvidence[] = [];
  let sequence = 0;
  let totalBytes = 0;

  return {
    record(
      document: ResearchDocument,
      requestId: string,
      task: ResearchTaskBinding,
    ): ResearchEvidence {
      const excerpt = truncateText(
        document.sections[0]?.text ?? "",
        RESEARCH_LIMITS.maxResearchEvidenceExcerptBytes,
      );
      sequence += 1;
      const entry: ResearchEvidence = {
        evidenceId: `ev-research-${sequence}`,
        requestId,
        taskId: task.taskId,
        taskContractRevision: task.taskContractRevision,
        source: { ...document.source },
        fetchedAtMs: document.fetchedAtMs,
        resolvedRevision: document.provenance.resolvedRevision,
        version: document.provenance.usedVersion,
        fallback: document.provenance.fallback,
        excerpt: excerpt.text,
        truncated: excerpt.truncated,
        byteLength: new TextEncoder().encode(excerpt.text).length,
      };
      entries.push(entry);
      totalBytes += entry.byteLength;
      while (
        entries.length > RESEARCH_LIMITS.maxRetainedEvidenceViews ||
        totalBytes > maxEvidenceBytes
      ) {
        const dropped = entries.shift();
        if (dropped === undefined) {
          break;
        }
        totalBytes -= dropped.byteLength;
      }
      return { ...entry, source: { ...entry.source } };
    },

    snapshots(): readonly ResearchEvidence[] {
      return entries.map((entry) => ({ ...entry, source: { ...entry.source } }));
    },
  };
}
