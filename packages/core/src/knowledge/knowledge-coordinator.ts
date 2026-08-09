import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import { isValidResearchSourceRef } from "../research/research-model.js";
import {
  KNOWLEDGE_LIMITS,
  KNOWLEDGE_RETRIEVAL_SCORING,
  KNOWLEDGE_STATE_VERSION,
  computeKnowledgeFactId,
  freshnessScore,
  isValidSubjectKey,
  normalizeFactContent,
  rejectPolicyShapedContent,
  tokenizeFactText,
  type KnowledgeActivation,
  type KnowledgeCandidate,
  type KnowledgeProvenanceRef,
  type KnowledgeRetrievalQuery,
  type KnowledgeRetrievalResult,
  type KnowledgeRetrievalSelection,
  type KnowledgeRetrievalTrace,
  type KnowledgeScope,
  type ProjectKnowledgeFact,
} from "./knowledge-model.js";

/**
 * KnowledgeCoordinator (Stage 3 milestone 4) — the single application-owned
 * writer of current project knowledge.
 *
 * Many components may propose knowledge; one coordinator owns durable
 * current-knowledge mutations. Providers and the CLI never write fact
 * structures directly. The coordinator is in-memory and serializable by
 * design (ADR 0017 §Persistence): structures are plain data, revisions are
 * immutable, and the schema version is documented for future persistence.
 *
 * Knowledge can never grant capability: this coordinator only stores and
 * retrieves facts. It never touches ToolProjector, CapabilityPolicy,
 * SandboxBackend, or approval rules, and its retrieval output is framed as
 * factual context by the projection layer.
 *
 * Research observations never become facts automatically: there is no
 * automatic proposal path from research evidence into knowledge. A fact may
 * cite `research_evidence` provenance only through an explicit `propose`
 * call whose evidence the host verifies (`hasResearchEvidence`).
 */

export type KnowledgeProposalResult =
  | { readonly status: "accepted"; readonly fact: ProjectKnowledgeFact }
  | { readonly status: "unchanged" }
  | { readonly status: "rejected"; readonly reason: string };

export interface KnowledgeCoordinatorOptions {
  readonly now?: () => number;
  /** Known secret values; candidates containing one are rejected. */
  readonly secrets?: readonly string[];
  /** Validates that referenced task evidence exists; rejects when false. */
  readonly hasEvidence?: (evidenceId: string) => boolean;
  /** Validates that a referenced workspace file state exists; rejects when false. */
  readonly hasFile?: (path: string, sha256: string) => boolean;
  /**
   * Validates that referenced research evidence exists (host-verified).
   * `research_evidence` provenance is accepted iff this is provided AND
   * returns true for the id; otherwise it is rejected with a precise
   * reason. Research never enters knowledge automatically.
   */
  readonly hasResearchEvidence?: (evidenceId: string) => boolean;
  readonly limits?: Partial<Record<keyof typeof KNOWLEDGE_LIMITS, number>>;
}

export interface KnowledgeCoordinator {
  /** The single mutation entry point for proposing facts. */
  propose(candidate: KnowledgeCandidate): KnowledgeProposalResult;
  /** Retire a subject: the current pointer becomes absent, revisions remain. */
  retire(subjectKey: string): void;
  /** Move a fact into the bounded pinned set (stable/contextual context). */
  pin(subjectKey: string): { readonly ok: true } | { readonly ok: false; readonly reason: string };
  unpin(subjectKey: string): void;
  /** Current active revision of a subject, or null. */
  fact(subjectKey: string): ProjectKnowledgeFact | null;
  /** Immutable revision history of a subject, oldest first. */
  history(subjectKey: string): readonly ProjectKnowledgeFact[];
  /** All current active facts (subject-keyed current revisions + one-off facts). */
  activeFacts(): readonly ProjectKnowledgeFact[];
  /** Retired subjects whose revisions are retained. */
  retiredSubjects(): readonly string[];
  /** Bounded pinned projection for stable/contextual context. */
  pinnedFacts(): readonly ProjectKnowledgeFact[];
  /** Deterministic bounded retrieval; expired and pinned facts excluded. */
  retrieve(query: KnowledgeRetrievalQuery): KnowledgeRetrievalResult;
  /** Trace of the most recent retrieval (debugging, tests, `/knowledge why`). */
  lastRetrievalTrace(): KnowledgeRetrievalTrace | null;
  /** Deterministic digest over the current knowledge state. */
  revision(): string;
  readonly size: number;
}

interface FactEntry {
  readonly revisions: ProjectKnowledgeFact[];
  readonly current: number | null;
}

const DEFAULT_SCOPE: KnowledgeScope = "project";

export function createKnowledgeCoordinator(
  options: KnowledgeCoordinatorOptions = {},
): KnowledgeCoordinator {
  const now = options.now ?? Date.now;
  const secrets = options.secrets ?? [];
  const hasEvidence = options.hasEvidence;
  const hasFile = options.hasFile;
  const hasResearchEvidence = options.hasResearchEvidence;
  const limits = { ...KNOWLEDGE_LIMITS, ...options.limits };
  // subjectKey -> entry (immutable revisions + current pointer).
  const bySubject = new Map<string, FactEntry>();
  // one-off facts (no subject key) are individually stored facts.
  const oneOff = new Map<string, ProjectKnowledgeFact>();
  const retiredSubjects = new Set<string>();
  const pinned = new Set<string>();
  let lastTrace: KnowledgeRetrievalTrace | null = null;

  function confidenceFor(candidate: KnowledgeCandidate): ProjectKnowledgeFact["confidence"] {
    return candidate.proposedConfidence ?? (evidenceCount(candidate) > 0 ? "medium" : "low");
  }

  function evidenceCount(candidate: KnowledgeCandidate): number {
    return candidate.provenance?.length ?? 0;
  }

  function validateCandidate(
    candidate: KnowledgeCandidate,
  ): { readonly ok: true } | { readonly ok: false; readonly reason: string } {
    if (typeof candidate.content !== "string" || candidate.content.trim().length === 0) {
      return { ok: false, reason: "A knowledge fact requires non-empty content." };
    }
    const contentBytes = new TextEncoder().encode(candidate.content).length;
    if (contentBytes > limits.maxContentBytes) {
      return {
        ok: false,
        reason: `Knowledge content exceeds the limit of ${limits.maxContentBytes} bytes.`,
      };
    }
    if (candidate.subjectKey !== undefined) {
      if (typeof candidate.subjectKey !== "string" || !isValidSubjectKey(candidate.subjectKey)) {
        return {
          ok: false,
          reason: `The subject key "${String(candidate.subjectKey)}" is malformed; use a lowercase dotted key such as project.godot.version.`,
        };
      }
    }
    if (
      candidate.type !== undefined &&
      candidate.type !== "fact" &&
      candidate.type !== "decision" &&
      candidate.type !== "convention"
    ) {
      return { ok: false, reason: "Unknown knowledge fact type." };
    }
    for (const ref of candidate.provenance ?? []) {
      const check = validateProvenanceRef(ref);
      if (check !== null) {
        return { ok: false, reason: check };
      }
    }
    for (const secret of secrets) {
      if (secret.length > 0 && candidate.content.includes(secret)) {
        return {
          ok: false,
          reason: "The candidate contains a known secret and cannot be stored as knowledge.",
        };
      }
    }
    const policy = rejectPolicyShapedContent(candidate.content);
    if (policy !== null) {
      return { ok: false, reason: policy };
    }
    return { ok: true };
  }

  function validateProvenanceRef(ref: KnowledgeProvenanceRef): string | null {
    if (ref.type === "evidence") {
      if (ref.evidenceId.length === 0) {
        return "An evidence provenance reference requires an evidence id.";
      }
      if (hasEvidence !== undefined && !hasEvidence(ref.evidenceId)) {
        return `The referenced evidence "${ref.evidenceId}" does not exist; a fact cannot cite missing evidence.`;
      }
      return null;
    }
    if (ref.type === "workspace_file") {
      if (ref.path.length === 0 || !/^[a-z0-9._/ -]+$/i.test(ref.path) || ref.path.includes("\\")) {
        return `The workspace-file provenance path "${ref.path}" is malformed.`;
      }
      if (ref.sha256.length !== 64 || !/^[0-9a-f]{64}$/.test(ref.sha256)) {
        return "A workspace-file provenance reference requires the exact 64-hex-digit SHA-256.";
      }
      if (hasFile !== undefined && !hasFile(ref.path, ref.sha256)) {
        return `The referenced file state "${ref.path}" does not match the current workspace; reread the file before citing it.`;
      }
      return null;
    }
    if (ref.type === "research_evidence") {
      if (ref.evidenceId.length === 0) {
        return "A research-evidence provenance reference requires an evidence id.";
      }
      if (!isValidResearchSourceRef(ref.source)) {
        return "A research-evidence provenance reference requires a valid research source.";
      }
      if (typeof ref.fetchedAtMs !== "number" || !Number.isFinite(ref.fetchedAtMs)) {
        return "A research-evidence provenance reference requires a valid fetch timestamp.";
      }
      if (hasResearchEvidence === undefined) {
        return "Research-evidence provenance requires host verification; no research-evidence verifier is configured.";
      }
      if (!hasResearchEvidence(ref.evidenceId)) {
        return `The referenced research evidence "${ref.evidenceId}" does not exist; a fact cannot cite missing evidence.`;
      }
      return null;
    }
    return "Unknown provenance reference type.";
  }

  function buildFact(
    subjectKey: string | null,
    revision: number,
    candidate: KnowledgeCandidate,
    createdAtMs: number,
    updatedAtMs: number,
    activation: KnowledgeActivation,
  ): ProjectKnowledgeFact {
    return Object.freeze({
      id: computeKnowledgeFactId({
        scope: DEFAULT_SCOPE,
        subjectKey,
        content: candidate.content,
        revision,
      }),
      scope: DEFAULT_SCOPE,
      subjectKey,
      type: candidate.type ?? "fact",
      content: candidate.content,
      revision,
      provenance: Object.freeze([...(candidate.provenance ?? [])]),
      confidence: confidenceFor(candidate),
      volatility: candidate.proposedVolatility ?? "normal",
      createdAtMs,
      updatedAtMs,
      lastVerifiedAtMs: candidate.lastVerifiedAtMs ?? null,
      expiresAtMs: candidate.expiresAtMs ?? null,
      activation,
    });
  }

  function proposeSubject(
    candidate: KnowledgeCandidate,
    subjectKey: string,
  ): KnowledgeProposalResult {
    const entry = bySubject.get(subjectKey);
    if (entry !== undefined && entry.current !== null) {
      const current = entry.revisions[entry.current - 1];
      if (
        current !== undefined &&
        normalizeFactContent(current.content) === normalizeFactContent(candidate.content)
      ) {
        return { status: "unchanged" };
      }
    }
    const nextRevision = entry === undefined ? 1 : entry.revisions.length + 1;
    if (entry !== undefined && entry.revisions.length >= limits.maxRevisionsPerSubject) {
      return {
        status: "rejected",
        reason: `The subject "${subjectKey}" has reached the revision-history limit of ${limits.maxRevisionsPerSubject}; retire and re-create it instead of churning revisions.`,
      };
    }
    if (entry === undefined && bySubject.size >= limits.maxFacts) {
      return {
        status: "rejected",
        reason: `The knowledge store reached the limit of ${limits.maxFacts} active facts.`,
      };
    }
    if (entry === undefined && oneOff.size + bySubject.size >= limits.maxFacts) {
      return {
        status: "rejected",
        reason: `The knowledge store reached the limit of ${limits.maxFacts} active facts.`,
      };
    }
    const timestamp = now();
    const activation: KnowledgeActivation =
      candidate.pinned === true && pinned.size < limits.maxPinnedFacts ? "pinned" : "retrieved";
    if (candidate.pinned === true && activation !== "pinned") {
      return {
        status: "rejected",
        reason: `The pinned-knowledge budget (${limits.maxPinnedFacts} facts) is exhausted; unpin another fact first.`,
      };
    }
    const fact = buildFact(subjectKey, nextRevision, candidate, timestamp, timestamp, activation);
    bySubject.set(subjectKey, {
      revisions: [...(entry?.revisions ?? []), fact],
      current: nextRevision,
    });
    retiredSubjects.delete(subjectKey);
    if (activation === "pinned") {
      pinned.add(subjectKey);
    }
    return { status: "accepted", fact };
  }

  function proposeOneOff(candidate: KnowledgeCandidate): KnowledgeProposalResult {
    if (oneOff.size + bySubject.size >= limits.maxFacts) {
      return {
        status: "rejected",
        reason: `The knowledge store reached the limit of ${limits.maxFacts} active facts.`,
      };
    }
    const timestamp = now();
    const fact = buildFact(null, 1, candidate, timestamp, timestamp, "retrieved");
    oneOff.set(fact.id, fact);
    return { status: "accepted", fact };
  }

  function activeFacts(): readonly ProjectKnowledgeFact[] {
    const facts: ProjectKnowledgeFact[] = [];
    for (const entry of bySubject.values()) {
      if (entry.current === null) {
        continue;
      }
      const fact = entry.revisions[entry.current - 1];
      if (fact !== undefined) {
        facts.push(fact);
      }
    }
    for (const fact of oneOff.values()) {
      facts.push(fact);
    }
    return facts.sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  }

  function isExpired(fact: ProjectKnowledgeFact, atMs: number): boolean {
    return fact.expiresAtMs !== null && fact.expiresAtMs <= atMs;
  }

  function retrieve(query: KnowledgeRetrievalQuery): KnowledgeRetrievalResult {
    const timestamp = now();
    const limit = Math.min(query.limit ?? limits.maxRetrievalFacts, limits.maxRetrievalFacts);
    const maxBytes = Math.min(query.maxBytes ?? limits.maxRetrievalBytes, limits.maxRetrievalBytes);
    const queryTokens = tokenizeFactText(query.text ?? "");
    const querySubject = query.subjectKey ?? null;
    const queryPaths = (query.paths ?? []).map((path) =>
      path.replace(/\\/g, "/").replace(/^\.\//, ""),
    );
    const considered: Array<{
      readonly fact: ProjectKnowledgeFact;
      readonly score: number;
      readonly reasons: readonly string[];
    }> = [];
    for (const fact of activeFacts()) {
      if (fact.activation === "pinned") {
        continue; // pinned facts are always present via the pinned projection
      }
      if (isExpired(fact, timestamp)) {
        continue;
      }
      if (query.factTypes !== undefined && !query.factTypes.includes(fact.type)) {
        continue;
      }
      const { score, reasons } = scoreFact(fact, {
        queryTokens,
        querySubject,
        queryPaths,
        nowMs: timestamp,
      });
      if (score > 0) {
        considered.push({ fact, score, reasons });
      }
    }
    considered.sort((a, b) => {
      if (b.score !== a.score) {
        return b.score - a.score;
      }
      const aKey = a.fact.subjectKey ?? a.fact.id;
      const bKey = b.fact.subjectKey ?? b.fact.id;
      if (aKey !== bKey) {
        return aKey < bKey ? -1 : 1;
      }
      return b.fact.revision - a.fact.revision;
    });
    const selected: ProjectKnowledgeFact[] = [];
    const selections: KnowledgeRetrievalSelection[] = [];
    let usedBytes = 0;
    let omitted = 0;
    for (const candidate of considered) {
      const bytes = factProjectionBytes(candidate.fact);
      if (selected.length >= limit || usedBytes + bytes > maxBytes) {
        omitted += 1;
        continue;
      }
      selected.push(candidate.fact);
      usedBytes += bytes;
      selections.push({
        factId: candidate.fact.id,
        subjectKey: candidate.fact.subjectKey,
        revision: candidate.fact.revision,
        confidence: candidate.fact.confidence,
        volatility: candidate.fact.volatility,
        score: Math.round(candidate.score * 100) / 100,
        matchReasons: candidate.reasons,
        expiresAtMs: candidate.fact.expiresAtMs,
      });
    }
    const trace: KnowledgeRetrievalTrace = {
      atMs: timestamp,
      scope: DEFAULT_SCOPE,
      query: {
        text: query.text ?? null,
        subjectKey: querySubject,
        paths: [...queryPaths],
        factTypes: [...(query.factTypes ?? [])],
      },
      selected: selections,
      consideredCount: considered.length,
      omittedCount: omitted,
      budget: { limit, maxBytes, usedBytes },
    };
    lastTrace = trace;
    return { facts: selected, trace };
  }

  function scoreFact(
    fact: ProjectKnowledgeFact,
    context: {
      readonly queryTokens: readonly string[];
      readonly querySubject: string | null;
      readonly queryPaths: readonly string[];
      readonly nowMs: number;
    },
  ): { readonly score: number; readonly reasons: readonly string[] } {
    const reasons: string[] = [];
    let relevance = 0;
    if (context.querySubject !== null && fact.subjectKey !== null) {
      if (fact.subjectKey === context.querySubject) {
        relevance += KNOWLEDGE_RETRIEVAL_SCORING.subjectExact;
        reasons.push("exact subject-key match");
      } else if (
        fact.subjectKey.startsWith(`${context.querySubject}.`) ||
        context.querySubject.startsWith(`${fact.subjectKey}.`)
      ) {
        relevance += KNOWLEDGE_RETRIEVAL_SCORING.subjectPrefix;
        reasons.push("subject-key prefix match");
      }
    }
    const factTokens = tokenizeFactText(`${fact.subjectKey ?? ""} ${fact.content}`);
    const overlap = context.queryTokens.filter((token) => factTokens.includes(token)).length;
    if (overlap > 0) {
      relevance += Math.min(
        overlap * KNOWLEDGE_RETRIEVAL_SCORING.keywordPerOverlap,
        KNOWLEDGE_RETRIEVAL_SCORING.keywordCap,
      );
      reasons.push(`keyword overlap (${overlap})`);
    }
    for (const ref of fact.provenance) {
      if (ref.type === "workspace_file") {
        const refPath = ref.path;
        const relevant = context.queryPaths.some(
          (path) =>
            refPath === path || refPath.startsWith(`${path}/`) || path.startsWith(`${refPath}/`),
        );
        if (relevant) {
          relevance += KNOWLEDGE_RETRIEVAL_SCORING.pathRelevance;
          reasons.push(`provenance path relevance (${refPath})`);
          break;
        }
      }
    }
    // A fact only matches when it has a real relevance signal (subject,
    // keyword, or path). Confidence and freshness rank matches; they
    // never make an unrelated fact match.
    if (relevance <= 0) {
      return { score: 0, reasons: [] };
    }
    const confidence =
      fact.confidence === "high"
        ? KNOWLEDGE_RETRIEVAL_SCORING.confidenceHigh
        : fact.confidence === "medium"
          ? KNOWLEDGE_RETRIEVAL_SCORING.confidenceMedium
          : KNOWLEDGE_RETRIEVAL_SCORING.confidenceLow;
    if (confidence > 0) {
      relevance += confidence;
      reasons.push(`confidence ${fact.confidence}`);
    }
    const freshness = freshnessScore(fact.volatility, fact.updatedAtMs, context.nowMs);
    if (freshness.score > 0) {
      relevance += freshness.score;
      reasons.push(`freshness ${freshness.days}d`);
    }
    return { score: relevance, reasons };
  }

  return {
    propose(candidate: KnowledgeCandidate): KnowledgeProposalResult {
      const validated = validateCandidate(candidate);
      if (!validated.ok) {
        return { status: "rejected", reason: validated.reason };
      }
      if (candidate.subjectKey !== undefined) {
        return proposeSubject(candidate, candidate.subjectKey);
      }
      return proposeOneOff(candidate);
    },

    retire(subjectKey: string): void {
      const entry = bySubject.get(subjectKey);
      if (entry === undefined) {
        return;
      }
      bySubject.set(subjectKey, { ...entry, current: null });
      retiredSubjects.add(subjectKey);
      pinned.delete(subjectKey);
    },

    pin(
      subjectKey: string,
    ): { readonly ok: true } | { readonly ok: false; readonly reason: string } {
      const fact = this.fact(subjectKey);
      if (fact === null) {
        return { ok: false, reason: `No active fact exists for subject "${subjectKey}".` };
      }
      if (fact.activation === "pinned") {
        return { ok: true };
      }
      if (pinned.size >= limits.maxPinnedFacts) {
        return {
          ok: false,
          reason: `The pinned-knowledge budget (${limits.maxPinnedFacts} facts) is exhausted; unpin another fact first.`,
        };
      }
      if (
        pinnedProjectionBytes() + new TextEncoder().encode(fact.content).length >
        limits.maxPinnedBytes
      ) {
        return {
          ok: false,
          reason: `Pinning "${subjectKey}" would exceed the pinned-knowledge byte budget (${limits.maxPinnedBytes} bytes).`,
        };
      }
      pinned.add(subjectKey);
      replaceActivation(subjectKey, "pinned");
      return { ok: true };
    },

    unpin(subjectKey: string): void {
      if (!pinned.delete(subjectKey)) {
        return;
      }
      replaceActivation(subjectKey, "retrieved");
    },

    fact(subjectKey: string): ProjectKnowledgeFact | null {
      const entry = bySubject.get(subjectKey);
      if (entry === undefined || entry.current === null) {
        return null;
      }
      return entry.revisions[entry.current - 1] ?? null;
    },

    history(subjectKey: string): readonly ProjectKnowledgeFact[] {
      const entry = bySubject.get(subjectKey);
      if (entry === undefined) {
        return [];
      }
      return [...entry.revisions];
    },

    activeFacts,

    retiredSubjects(): readonly string[] {
      return [...retiredSubjects].sort();
    },

    pinnedFacts(): readonly ProjectKnowledgeFact[] {
      const facts: ProjectKnowledgeFact[] = [];
      for (const subjectKey of pinned) {
        const fact = this.fact(subjectKey);
        if (fact !== null) {
          facts.push(fact);
        }
      }
      return facts.sort((a, b) => {
        const aKey = a.subjectKey ?? "";
        const bKey = b.subjectKey ?? "";
        return aKey < bKey ? -1 : aKey > bKey ? 1 : 0;
      });
    },

    retrieve,

    lastRetrievalTrace(): KnowledgeRetrievalTrace | null {
      return lastTrace === null ? null : structuredClone(lastTrace);
    },

    revision(): string {
      return sha256Hex(
        canonicalizeJson({
          version: KNOWLEDGE_STATE_VERSION,
          facts: activeFacts().map((fact) => ({
            id: fact.id,
            subjectKey: fact.subjectKey,
            revision: fact.revision,
            content: fact.content,
            activation: fact.activation,
          })),
        }),
      );
    },

    get size(): number {
      return bySubject.size + oneOff.size;
    },
  };

  function replaceActivation(subjectKey: string, activation: KnowledgeActivation): void {
    const entry = bySubject.get(subjectKey);
    if (entry === undefined || entry.current === null) {
      return;
    }
    const current = entry.revisions[entry.current - 1];
    if (current === undefined) {
      return;
    }
    const updated: ProjectKnowledgeFact = Object.freeze({ ...current, activation });
    const revisions = [...entry.revisions];
    revisions[entry.current - 1] = updated;
    bySubject.set(subjectKey, { ...entry, revisions });
  }

  function pinnedProjectionBytes(): number {
    let bytes = 0;
    for (const subjectKey of pinned) {
      const entry = bySubject.get(subjectKey);
      if (entry === undefined || entry.current === null) {
        continue;
      }
      const fact = entry.revisions[entry.current - 1];
      if (fact !== undefined) {
        bytes += new TextEncoder().encode(fact.content).length;
      }
    }
    return bytes;
  }
}

function factProjectionBytes(fact: ProjectKnowledgeFact): number {
  return new TextEncoder().encode(`${fact.subjectKey ?? fact.id} ${fact.content}`).length;
}
