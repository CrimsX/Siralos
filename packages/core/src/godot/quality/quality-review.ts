import { canonicalizeJson, sha256Hex } from "../digest.js";
import { QUALITY_LIMITS, type ChangeDiffMetrics, type QualityEvidence } from "./quality-model.js";

/**
 * Independent change reviewer contracts (§25–§36).
 *
 * The reviewer is one dedicated model-based reasoning signal over the
 * final change. It uses a FRESH provider context (never the primary
 * implementer's conversational history), is strictly read-only, and
 * returns bounded structured findings. Deterministic gates remain
 * authoritative for measurable conditions: the reviewer can never replace
 * parser checks, LSP diagnostics, hash verification, source-integrity
 * checks, sandbox enforcement, or test exit codes, and a reviewer finding
 * is untrusted data until validated and normalized.
 */

export type ChangeReviewFindingSeverity = "critical" | "high" | "medium" | "low";

export type ChangeReviewFindingCategory =
  | "correctness"
  | "regression"
  | "godot-api"
  | "architecture"
  | "security"
  | "maintainability"
  | "testing"
  | "documentation"
  | "style";

export type ChangeReviewConfidence = "high" | "medium" | "low";

export interface ChangeReviewFinding {
  readonly id: string;
  readonly severity: ChangeReviewFindingSeverity;
  readonly category: ChangeReviewFindingCategory;
  readonly title: string;
  /** Workspace-relative path; null when not applicable. */
  readonly path: string | null;
  readonly line: number | null;
  readonly evidence: string;
  readonly impact: string;
  readonly recommendation: string;
  readonly confidence: ChangeReviewConfidence;
}

/** One changed file carried into the review context (bounded). */
export interface ChangeReviewFile {
  readonly path: string;
  readonly unifiedDiff: string;
}

/**
 * Read-only review request (§28). Never includes the primary provider's
 * hidden reasoning, credentials, approval internals, mirror host paths,
 * or unrelated conversation history.
 */
export interface ChangeReviewRequest {
  readonly developmentId: string;
  /** The user's original development request. */
  readonly request: string;
  readonly engineVersion: string | null;
  readonly changedPaths: readonly string[];
  /** Complete bounded per-file diffs (§52/§53). */
  readonly files: readonly ChangeReviewFile[];
  readonly metrics: ChangeDiffMetrics;
  /** Bounded validation evidence summary (parser, LSP, scope, tests). */
  readonly evidenceSummary: readonly QualityEvidence[];
  /** Applicable repository guidance when the workflow discovered any. */
  readonly repositoryGuidance: string | null;
  /** Finding ids of previous review rounds, for traceability only (§35). */
  readonly previousFindingIds: readonly string[];
  readonly reviewRound: number;
}

export type ChangeReviewResultStatus = "completed" | "cancelled" | "failed" | "too_large";

export interface ChangeReviewResult {
  readonly status: ChangeReviewResultStatus;
  readonly findings: readonly ChangeReviewFinding[];
  readonly message: string | null;
}

/**
 * Dedicated independent reviewer port. Implementations must use a fresh
 * provider context, must be read-only, and must respect the abort signal.
 */
export interface ChangeReviewer {
  review(request: ChangeReviewRequest, signal?: AbortSignal): Promise<ChangeReviewResult>;
}

export function createCleanReviewResult(): ChangeReviewResult {
  return { status: "completed", findings: [], message: null };
}

export function estimateReviewContextBytes(request: ChangeReviewRequest): number {
  const encoder = new TextEncoder();
  let total = 0;
  for (const file of request.files) {
    total += encoder.encode(file.unifiedDiff).length;
  }
  return total;
}

/**
 * Deterministic chunking by complete file (§53). When the full diff
 * exceeds the review-context bound, the files are split into chunks that
 * each fit; every changed file is covered by exactly one chunk and the
 * shared request metadata is repeated per chunk so no chunk loses context.
 */
export function chunkChangeReviewRequests(
  request: ChangeReviewRequest,
  maxBytes: number = QUALITY_LIMITS.maxReviewContextDiffBytes,
): readonly ChangeReviewRequest[] {
  const chunks: ChangeReviewRequest[] = [];
  const encoder = new TextEncoder();
  let current: ChangeReviewFile[] = [];
  let currentBytes = 0;
  for (const file of request.files) {
    const bytes = encoder.encode(file.unifiedDiff).length;
    if (current.length > 0 && currentBytes + bytes > maxBytes) {
      chunks.push(buildChunk(request, current));
      current = [];
      currentBytes = 0;
    }
    current.push(file);
    currentBytes += bytes;
  }
  if (current.length > 0) {
    chunks.push(buildChunk(request, current));
  }
  if (chunks.length === 0) {
    return [request];
  }
  return chunks;
}

function buildChunk(
  request: ChangeReviewRequest,
  files: readonly ChangeReviewFile[],
): ChangeReviewRequest {
  return {
    ...request,
    files,
    changedPaths: files.map((file) => file.path),
  };
}

const SEVERITIES: readonly ChangeReviewFindingSeverity[] = ["critical", "high", "medium", "low"];
const CATEGORIES: readonly ChangeReviewFindingCategory[] = [
  "correctness",
  "regression",
  "godot-api",
  "architecture",
  "security",
  "maintainability",
  "testing",
  "documentation",
  "style",
];
const CONFIDENCES: readonly ChangeReviewConfidence[] = ["high", "medium", "low"];

/**
 * Deterministic local finding identity (§36) over safe normalized fields.
 * Never a security identifier.
 */
export function deterministicFindingId(input: {
  readonly category: ChangeReviewFindingCategory;
  readonly path: string | null;
  readonly line: number | null;
  readonly title: string;
}): string {
  return sha256Hex(
    canonicalizeJson({
      category: input.category,
      path: input.path ?? "",
      line: input.line ?? -1,
      title: normalizeFindingTitle(input.title),
    }),
  ).slice(0, 24);
}

export function normalizeFindingTitle(title: string): string {
  return title
    .replace(/\s+/g, " ")
    .trim()
    .toLowerCase()
    .slice(0, QUALITY_LIMITS.maxFindingTitleChars);
}

/**
 * Runtime structure validation of reviewer output (§51). Reviewer output
 * is UNTRUSTED data: fields are type-checked, bounded, trimmed, paths are
 * normalized to safe workspace-relative form (absolute paths, drive
 * letters, backslashes, `..` segments, and null bytes are rejected),
 * findings are bounded to the immutable maximum, and deterministic ids are
 * assigned. Malformed output is rejected, never partially trusted.
 */
export function normalizeReviewFindings(
  value: unknown,
  options: { readonly maxFindings?: number } = {},
):
  | { readonly ok: true; readonly findings: readonly ChangeReviewFinding[] }
  | {
      readonly ok: false;
      readonly message: string;
    } {
  const maxFindings = options.maxFindings ?? QUALITY_LIMITS.maxReviewFindings;
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return { ok: false, message: "The review output must be a JSON object." };
  }
  const record = value as Record<string, unknown>;
  const rawFindings = record["findings"];
  if (!Array.isArray(rawFindings)) {
    return { ok: false, message: 'The review output must contain a "findings" array.' };
  }
  if (rawFindings.length > maxFindings) {
    return {
      ok: false,
      message: `The review returned ${rawFindings.length} findings; the maximum is ${maxFindings}.`,
    };
  }
  const findings: ChangeReviewFinding[] = [];
  for (const entry of rawFindings) {
    const parsed = parseFinding(entry);
    if (!parsed.ok) {
      return parsed;
    }
    findings.push(parsed.finding);
  }
  return { ok: true, findings: deduplicateReviewFindings(findings) };
}

function parseFinding(value: unknown):
  | { readonly ok: true; readonly finding: ChangeReviewFinding }
  | {
      readonly ok: false;
      readonly message: string;
    } {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return { ok: false, message: "Every review finding must be an object." };
  }
  const record = value as Record<string, unknown>;
  const severity = readEnum(record, "severity", SEVERITIES);
  if (!severity.ok) {
    return severity;
  }
  const category = readEnum(record, "category", CATEGORIES);
  if (!category.ok) {
    return category;
  }
  const confidence = readEnum(record, "confidence", CONFIDENCES);
  if (!confidence.ok) {
    return confidence;
  }
  const title = readBoundedString(record, "title", QUALITY_LIMITS.maxFindingTitleChars, true);
  if (!title.ok) {
    return title;
  }
  const evidence = readBoundedString(
    record,
    "evidence",
    QUALITY_LIMITS.maxFindingEvidenceChars,
    false,
  );
  if (!evidence.ok) {
    return evidence;
  }
  const impact = readBoundedString(record, "impact", QUALITY_LIMITS.maxFindingImpactChars, false);
  if (!impact.ok) {
    return impact;
  }
  const recommendation = readBoundedString(
    record,
    "recommendation",
    QUALITY_LIMITS.maxFindingRecommendationChars,
    false,
  );
  if (!recommendation.ok) {
    return recommendation;
  }
  const path = readSafePath(record);
  if (!path.ok) {
    return path;
  }
  const line = readLine(record);
  if (!line.ok) {
    return line;
  }
  const finding: ChangeReviewFinding = {
    id: deterministicFindingId({
      category: category.value,
      path: path.value,
      line: line.value,
      title: title.value,
    }),
    severity: severity.value,
    category: category.value,
    title: title.value,
    path: path.value,
    line: line.value,
    evidence: evidence.value,
    impact: impact.value,
    recommendation: recommendation.value,
    confidence: confidence.value,
  };
  return { ok: true, finding };
}

function readEnum<T extends string>(
  record: Record<string, unknown>,
  key: string,
  allowed: readonly T[],
): { readonly ok: true; readonly value: T } | { readonly ok: false; readonly message: string } {
  const value = record[key];
  if (typeof value !== "string" || !allowed.includes(value as T)) {
    return {
      ok: false,
      message: `Finding field "${key}" must be one of: ${allowed.join(", ")}.`,
    };
  }
  return { ok: true, value: value as T };
}

function readBoundedString(
  record: Record<string, unknown>,
  key: string,
  maxChars: number,
  required: boolean,
):
  { readonly ok: true; readonly value: string } | { readonly ok: false; readonly message: string } {
  const value = record[key];
  if (value === undefined) {
    if (required) {
      return { ok: false, message: `Finding field "${key}" is required.` };
    }
    return { ok: true, value: "" };
  }
  if (typeof value !== "string") {
    return { ok: false, message: `Finding field "${key}" must be a string.` };
  }
  const trimmed = value.trim();
  if (trimmed.length > maxChars) {
    return {
      ok: false,
      message: `Finding field "${key}" exceeds ${maxChars} characters; the finding was rejected.`,
    };
  }
  return { ok: true, value: trimmed };
}

/**
 * Safe path normalization: workspace-relative, `/`-separated, no absolute
 * prefixes, no drive letters, no backslashes, no `..` traversal, no null
 * bytes. Absolute private paths are rejected (never silently sanitized
 * into a different path).
 */
function readSafePath(record: Record<string, unknown>):
  | { readonly ok: true; readonly value: string | null }
  | {
      readonly ok: false;
      readonly message: string;
    } {
  const value = record["path"];
  if (value === undefined || value === null) {
    return { ok: true, value: null };
  }
  if (typeof value !== "string") {
    return { ok: false, message: 'Finding field "path" must be a string or null.' };
  }
  const trimmed = value.trim();
  if (trimmed.length === 0) {
    return { ok: true, value: null };
  }
  if (trimmed.length > QUALITY_LIMITS.maxFindingPathChars) {
    return { ok: false, message: "Finding path exceeds the length bound." };
  }
  if (trimmed.includes("\u0000") || trimmed.includes("\\") || trimmed.includes(":")) {
    return {
      ok: false,
      message:
        "Finding paths must be workspace-relative with forward slashes; absolute and drive-qualified paths are rejected.",
    };
  }
  if (trimmed.startsWith("/") || /^[A-Za-z]:/.test(trimmed)) {
    return {
      ok: false,
      message: "Finding paths must be workspace-relative; absolute paths are rejected.",
    };
  }
  for (const segment of trimmed.split("/")) {
    if (segment === ".." || segment === "." || segment.length === 0) {
      return {
        ok: false,
        message: "Finding paths must not contain traversal segments, empty segments, or dots.",
      };
    }
  }
  return { ok: true, value: trimmed };
}

function readLine(record: Record<string, unknown>):
  | { readonly ok: true; readonly value: number | null }
  | {
      readonly ok: false;
      readonly message: string;
    } {
  const value = record["line"];
  if (value === undefined || value === null) {
    return { ok: true, value: null };
  }
  if (typeof value !== "number" || !Number.isInteger(value) || value < 1 || value > 10_000_000) {
    return { ok: false, message: 'Finding field "line" must be a positive integer or null.' };
  }
  return { ok: true, value };
}

/**
 * Conservative deduplication (§36): identical deterministic ids collapse
 * to the first occurrence, preserving deterministic ordering.
 */
export function deduplicateReviewFindings(
  findings: readonly ChangeReviewFinding[],
): readonly ChangeReviewFinding[] {
  const seen = new Set<string>();
  const unique: ChangeReviewFinding[] = [];
  for (const finding of findings) {
    if (seen.has(finding.id)) {
      continue;
    }
    seen.add(finding.id);
    unique.push(finding);
  }
  return unique;
}

/**
 * Blocking policy (§31): Critical/High findings block clean completion
 * only when backed by sufficient concrete evidence. A reviewer cannot
 * change this policy; findings are normalized conservatively — a
 * Low-confidence Critical/High finding is advisory, never silently
 * blocking. Medium/Low findings are always advisory.
 */
export function classifyReviewFindingBlocking(finding: ChangeReviewFinding): boolean {
  if (finding.severity !== "critical" && finding.severity !== "high") {
    return false;
  }
  return finding.confidence === "high" || finding.confidence === "medium";
}

export function countBlockingFindings(findings: readonly ChangeReviewFinding[]): number {
  return findings.filter(classifyReviewFindingBlocking).length;
}

export function countReviewFindingsBySeverity(findings: readonly ChangeReviewFinding[]): {
  readonly critical: number;
  readonly high: number;
  readonly medium: number;
  readonly low: number;
} {
  return {
    critical: findings.filter((finding) => finding.severity === "critical").length,
    high: findings.filter((finding) => finding.severity === "high").length,
    medium: findings.filter((finding) => finding.severity === "medium").length,
    low: findings.filter((finding) => finding.severity === "low").length,
  };
}

/**
 * Read-only aggregation of chunked review results (§53): completed chunk
 * results are merged, deduplicated by deterministic identity, and bounded
 * to the immutable maximum; a cancelled/failed/too-large chunk is never
 * silently converted into a completed review. The aggregator never
 * re-reviews and never executes.
 */
export function aggregateReviewResults(results: readonly ChangeReviewResult[]): ChangeReviewResult {
  for (const result of results) {
    if (result.status !== "completed") {
      return result;
    }
  }
  const merged: ChangeReviewFinding[] = [];
  for (const result of results) {
    merged.push(...result.findings);
  }
  const deduplicated = deduplicateReviewFindings(merged);
  const bounded = deduplicated.slice(0, QUALITY_LIMITS.maxReviewFindings);
  return { status: "completed", findings: bounded, message: null };
}
