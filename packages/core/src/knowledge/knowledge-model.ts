import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import { computeArtifactDigest } from "../identity/artifact-digest.js";
import type { EvidenceKind } from "../tasks/task-model.js";
import type { ResearchSourceRef } from "../research/research-model.js";

/**
 * Structured project knowledge facts (Stage 3 milestone 4).
 *
 * Knowledge records factual claims about the project. It is factual
 * context, never instruction authority: a knowledge record cannot enable a
 * tool, grant a permission, override sandbox policy, approve a mutation,
 * override a TaskContract, or override hard security rules. Knowledge is
 * untrusted input to the model, framed as facts with provenance,
 * confidence, freshness, and expiry.
 *
 * Authority classes are distinct (ADR 0017):
 *
 *   instructions tell Solaris how work should be performed
 *   knowledge   records factual claims about the project
 *   history     records what happened or was observed
 */

export type KnowledgeScope = "project";

export type KnowledgeFactType = "fact" | "decision" | "convention";

export type KnowledgeConfidence = "low" | "medium" | "high";

export type KnowledgeVolatility = "volatile" | "normal" | "stable" | "evergreen";

export type KnowledgeActivation = "pinned" | "retrieved";

export type KnowledgeFactId = string;

/**
 * Provenance reference. Facts point at already-owned artifacts instead of
 * duplicating raw adapter output: task evidence records, exact workspace
 * file states (path + SHA-256), or host-verified research evidence.
 * Research observations never become facts automatically — a fact may only
 * cite research evidence that the host verified (`hasResearchEvidence`).
 */
export type KnowledgeProvenanceRef =
  | { readonly type: "evidence"; readonly evidenceId: string; readonly kind: EvidenceKind }
  | { readonly type: "workspace_file"; readonly path: string; readonly sha256: string }
  | {
      readonly type: "research_evidence";
      readonly evidenceId: string;
      readonly source: ResearchSourceRef;
      readonly fetchedAtMs: number;
    };

export interface ProjectKnowledgeFact {
  /** Deterministic identity of this exact revision instance. */
  readonly id: KnowledgeFactId;
  readonly scope: KnowledgeScope;
  /** Subject key of the evolving fact; null for one-off facts. */
  readonly subjectKey: string | null;
  readonly type: KnowledgeFactType;
  readonly content: string;
  /**
   * Canonical content digest of this revision (ADR 0028). Content only —
   * provenance, confidence, freshness, and volatility are NEVER collapsed
   * into this hash equality; they remain separate authoritative fields.
   */
  readonly contentDigest: string;
  /** Immutable revision number (1-based). Subject-less facts are revision 1. */
  readonly revision: number;
  readonly provenance: readonly KnowledgeProvenanceRef[];
  readonly confidence: KnowledgeConfidence;
  readonly volatility: KnowledgeVolatility;
  readonly createdAtMs: number;
  readonly updatedAtMs: number;
  readonly lastVerifiedAtMs: number | null;
  /** Facts past expiry are excluded from automatic retrieval but remain history. */
  readonly expiresAtMs: number | null;
  readonly activation: KnowledgeActivation;
}

export interface KnowledgeCandidate {
  /** Subject key of an evolving fact; omit for a one-off fact. */
  readonly subjectKey?: string;
  readonly type?: KnowledgeFactType;
  readonly content: string;
  readonly provenance?: readonly KnowledgeProvenanceRef[];
  readonly proposedConfidence?: KnowledgeConfidence;
  readonly proposedVolatility?: KnowledgeVolatility;
  readonly lastVerifiedAtMs?: number;
  readonly expiresAtMs?: number;
  readonly pinned?: boolean;
}

export interface KnowledgeRetrievalQuery {
  /** Task text used as the retrieval basis (keyword overlap). */
  readonly text?: string;
  /** Exact subject-key retrieval. */
  readonly subjectKey?: string;
  /** Workspace-relative paths the current task affects. */
  readonly paths?: readonly string[];
  readonly factTypes?: readonly KnowledgeFactType[];
  readonly limit?: number;
  readonly maxBytes?: number;
}

export interface KnowledgeRetrievalSelection {
  readonly factId: KnowledgeFactId;
  readonly subjectKey: string | null;
  readonly revision: number;
  readonly confidence: KnowledgeConfidence;
  readonly volatility: KnowledgeVolatility;
  readonly score: number;
  /** Deterministic human-readable match reasons. */
  readonly matchReasons: readonly string[];
  readonly expiresAtMs: number | null;
}

export interface KnowledgeRetrievalTrace {
  readonly atMs: number;
  readonly scope: KnowledgeScope;
  readonly query: {
    readonly text: string | null;
    readonly subjectKey: string | null;
    readonly paths: readonly string[];
    readonly factTypes: readonly KnowledgeFactType[];
  };
  readonly selected: readonly KnowledgeRetrievalSelection[];
  /** Active facts considered before budget selection. */
  readonly consideredCount: number;
  /** Matching facts excluded by the budget (count + bytes). */
  readonly omittedCount: number;
  readonly budget: {
    readonly limit: number;
    readonly maxBytes: number;
    readonly usedBytes: number;
  };
}

export interface KnowledgeRetrievalResult {
  readonly facts: readonly ProjectKnowledgeFact[];
  readonly trace: KnowledgeRetrievalTrace;
}

export const KNOWLEDGE_STATE_VERSION = "knowledge-1";

export const KNOWLEDGE_LIMITS = {
  /** Active facts (current revisions) per project scope. */
  maxFacts: 256,
  /** Immutable revisions retained per subject key. */
  maxRevisionsPerSubject: 64,
  /** UTF-8 bytes of one fact's content. */
  maxContentBytes: 4 * 1024,
  /** UTF-8 bytes of one provenance description. */
  maxProvenanceBytes: 2 * 1024,
  /** Pinned facts that may enter stable/contextual context automatically. */
  maxPinnedFacts: 6,
  /** Total UTF-8 bytes of the pinned projection. */
  maxPinnedBytes: 1_200,
  /** Retrieved facts per query. */
  maxRetrievalFacts: 8,
  /** Total UTF-8 bytes of one retrieval result. */
  maxRetrievalBytes: 6_000,
  /** Longest subject key. */
  maxSubjectKeyLength: 128,
} as const;

export const SUBJECT_KEY_PATTERN = /^[a-z][a-z0-9._-]*$/;

export function isValidSubjectKey(subjectKey: string): boolean {
  return (
    subjectKey.length > 0 &&
    subjectKey.length <= KNOWLEDGE_LIMITS.maxSubjectKeyLength &&
    SUBJECT_KEY_PATTERN.test(subjectKey)
  );
}

export function computeKnowledgeFactId(input: {
  readonly scope: KnowledgeScope;
  readonly subjectKey: string | null;
  readonly content: string;
  readonly revision: number;
}): KnowledgeFactId {
  const digest = sha256Hex(
    canonicalizeJson({
      scope: input.scope,
      subjectKey: input.subjectKey,
      content: input.content,
      revision: input.revision,
    }),
  );
  return `kf_${digest.slice(0, 24)}`;
}

/** Structural normalization used for the no-churn comparison. */
export function normalizeFactContent(content: string): string {
  return content.replace(/\r\n/g, "\n").replace(/\s+/g, " ").trim();
}

/**
 * Canonical content digest of a knowledge fact revision (ADR 0028).
 * Content only; provenance/confidence/freshness/volatility stay separate.
 */
export function computeKnowledgeFactContentDigest(content: string): string {
  return computeArtifactDigest({
    artifactType: "KnowledgeFact",
    schemaVersion: 1,
    payload: { content },
  }).value;
}

/** Deterministic knowledge-state digest (task-snapshot identity). */
export function computeKnowledgeStateRevision(facts: readonly ProjectKnowledgeFact[]): string {
  return sha256Hex(
    canonicalizeJson(
      [...facts]
        .sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
        .map((fact) => ({
          id: fact.id,
          scope: fact.scope,
          subjectKey: fact.subjectKey,
          revision: fact.revision,
          content: fact.content,
          status: "active",
        })),
    ),
  );
}

// ---------------------------------------------------------------------------
// Retrieval scoring (documented constants, deterministic and explainable).
// ---------------------------------------------------------------------------

/** Keyword tokens that carry no retrieval signal. */
const STOPWORDS = new Set([
  "the",
  "a",
  "an",
  "is",
  "are",
  "was",
  "were",
  "to",
  "of",
  "for",
  "and",
  "or",
  "in",
  "on",
  "with",
  "at",
  "by",
  "it",
  "its",
  "this",
  "that",
  "project",
  "godot",
  "file",
  "files",
  "use",
  "uses",
  "using",
]);

export function tokenizeFactText(text: string): readonly string[] {
  const tokens = text
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter(Boolean);
  return [...new Set(tokens.filter((token) => !STOPWORDS.has(token) && token.length > 1))];
}

export const KNOWLEDGE_RETRIEVAL_SCORING = {
  subjectExact: 100,
  subjectPrefix: 40,
  keywordPerOverlap: 4,
  keywordCap: 40,
  pathRelevance: 20,
  confidenceHigh: 15,
  confidenceMedium: 8,
  confidenceLow: 2,
  /** Freshness weight per volatility class and its decay window in days. */
  freshness: {
    volatile: { weight: 12, windowDays: 30 },
    normal: { weight: 10, windowDays: 180 },
    stable: { weight: 6, windowDays: 730 },
    evergreen: { weight: 4, windowDays: 0 },
  } as const,
} as const;

export function freshnessScore(
  volatility: KnowledgeVolatility,
  updatedAtMs: number,
  nowMs: number,
): { readonly score: number; readonly days: number } {
  const rule = KNOWLEDGE_RETRIEVAL_SCORING.freshness[volatility];
  if (rule.windowDays === 0) {
    return { score: rule.weight, days: 0 };
  }
  const ageDays = Math.max(0, nowMs - updatedAtMs) / 86_400_000;
  return {
    score: rule.weight * Math.max(0, 1 - ageDays / rule.windowDays),
    days: Math.round(ageDays),
  };
}

// ---------------------------------------------------------------------------
// Conservative rejection of policy-shaped knowledge.
// ---------------------------------------------------------------------------

/**
 * Structural, conservative rejection of knowledge that masquerades as
 * policy. This is NOT natural-language safety classification: only clear
 * permission/capability/sandbox claims shaped like instructions are
 * rejected, and knowledge never grants capability regardless.
 */
const POLICY_CLAIM_PATTERNS: readonly RegExp[] = [
  /\balways\s+allow\b/,
  /\b(allow|enable|grant|permit|approve)\b[^.\n]{0,60}\b(shell|network|write|execute|exec|access|sandbox|approval|mutation|command|internet)\b/,
  /\b(access|shell|network|write|execute|commands?|scripts?)\b[^.\n]{0,30}\b(is|are)\s+(allowed|permitted|granted|enabled)\b/,
  /\b(disable|bypass|turn\s*off|turn\s*down)\b[^.\n]{0,60}\b(sandbox|approval|checkpoint|security|policy|restriction|limit)\b/,
  /\bignore\b[^.\n]{0,40}\b(policy|rules|restrictions|security|approval)\b/,
  /\bno\s+(approval|permission|checkpoint|review)\b[^.\n]{0,40}\b(needed|required|necessary)\b/,
  /\b(commands?|scripts?|shell|execution|mutations?)\b[^.\n]{0,40}\b(without|no)\s+(approval|permission|checkpoint|review)\b/,
  /\b(writes?|edits?|changes?|mutations?)\b[^.\n]{0,40}\b(without|no)\s+(approval|permission|checkpoint|review)\b/,
  /\b(is|are)\s+(allowed|permitted|granted)\b[^.\n]{0,40}\b(without|no)\s+(approval|permission|checkpoint|review)\b/,
  /^[^.\n]{0,40}\b(unrestricted|full)\s+(network|shell|write|access)\b/,
];

export function rejectPolicyShapedContent(content: string): string | null {
  const normalized = content.toLowerCase().replace(/\s+/g, " ").trim();
  for (const pattern of POLICY_CLAIM_PATTERNS) {
    if (pattern.test(normalized)) {
      return "Knowledge that claims permissions, capability grants, or sandbox/approval policy is not accepted; knowledge is factual context and can never grant capability.";
    }
  }
  return null;
}
