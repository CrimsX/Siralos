import { deepFreeze } from "../domain/deep-freeze.js";

/**
 * WorkspaceScope and ActiveWorkingSet (harness context optimization,
 * ADR 0023).
 *
 * A task's derived execution scope is distinct from the current step's
 * working set:
 *
 *   Task scope: 12 relevant files
 *   Current implementation step: 3 active files
 *
 * `WorkspaceScope` is a DERIVED execution scope, never a security
 * authority: actual capability/path policy remains authoritative, and
 * these models carry no policy surface. Verified files carry evidence
 * (a revision handle plus a `kind:ref` evidence pointer) and were
 * actually inspected; candidate files are merely potentially relevant
 * and their contents are never automatically placed into model context.
 * Promotion is explicit and observable: a candidate becomes verified
 * only with recorded evidence (`candidate -> structural/summary
 * inspection -> relevance evidence -> verified`).
 *
 * Source-context budgets control CONTEXT, not repository access: they
 * bound exact source bytes and active files, and eviction demotes exact
 * source views to summaries while retaining revision identity and
 * evidence references — authoritative evidence is never deleted.
 */

export type SourceFileConfidence = "verified" | "candidate";

/** Which representation of the file currently occupies source context. */
export type SourceView = "exact" | "structural" | "summary" | "none";

export interface SourceFileRef {
  /** Workspace-relative path. */
  readonly path: string;
  readonly confidence: SourceFileConfidence;
  readonly view: SourceView;
  /**
   * Workspace revision handle (`rev_` + 32 hex chars). REQUIRED for
   * verified files: a verified claim must point at the exact inspected
   * revision.
   */
  readonly revision?: string;
  /** Bounded evidence reference in `kind:ref` form; REQUIRED for verified files. */
  readonly evidence?: string;
  /** Bounded note on why the file entered the verified set. */
  readonly reason?: string;
}

export interface ScopePromotionRecord {
  readonly path: string;
  /** The evidence that justified promotion (REQUIRED; a guess never promotes). */
  readonly evidence: string;
  /** Exact inspected revision handle (REQUIRED for verified promotion). */
  readonly revision: string;
  /** Bounded reason the file is relevant. */
  readonly reason: string;
}

export interface WorkspaceContextBudget {
  /** Maximum exact source files in model context at once. */
  readonly maxActiveExactFiles: number;
  /** Maximum total exact source bytes in model context. */
  readonly maxExactBytes: number;
  /** Maximum structural/summary representations retained. */
  readonly maxStructuralSummaries: number;
  /** Maximum candidate files tracked. */
  readonly maxCandidateFiles: number;
  /** Maximum retained historical (evicted) source views. */
  readonly maxRetainedHistoricalViews: number;
}

export interface WorkspaceScope {
  readonly verifiedFiles: readonly SourceFileRef[];
  readonly candidateFiles: readonly SourceFileRef[];
  /** Workspace-relative roots where new files may be created (derived, not authority). */
  readonly allowedCreateRoots: readonly string[];
  /** Workspace-relative paths excluded from default source-context discovery. */
  readonly excludedPaths: readonly string[];
  readonly budget: WorkspaceContextBudget;
  /** Observable promotion history: candidate -> verified with evidence. */
  readonly promotions: readonly ScopePromotionRecord[];
}

export interface EvictionRecord {
  readonly path: string;
  /** The view that was dropped from exact source context. */
  readonly droppedView: SourceView;
  /** The view retained after eviction (revision/evidence always retained). */
  readonly retainedView: SourceView;
  readonly reason: string;
}

/** Host-owned hard bounds for workspace scope models (never raised by input). */
export const WORKSPACE_SCOPE_LIMITS = Object.freeze({
  maxVerifiedFiles: 64,
  maxCandidateFiles: 64,
  maxCreateRoots: 8,
  maxExcludedPaths: 32,
  maxPromotions: 128,
  maxPathBytes: 1024,
  maxEvidenceBytes: 256,
  maxReasonBytes: 512,
});

/**
 * Default source-context exclusions: noisy generated/vendor paths are
 * suppressed from default discovery unless a task explicitly requires
 * them. Context exclusion is NOT security denial — these paths may still
 * be read when the task needs them.
 */
export const DEFAULT_SOURCE_EXCLUSIONS: readonly string[] = deepFreeze([
  "node_modules/",
  "dist/",
  "build/",
  "coverage/",
  ".git/",
  ".godot/",
  "generated/",
  "out/",
]);

/** Conservative deterministic budget defaults (host-owned). */
export const DEFAULT_WORKSPACE_CONTEXT_BUDGET: WorkspaceContextBudget = deepFreeze({
  maxActiveExactFiles: 4,
  maxExactBytes: 32 * 1024,
  maxStructuralSummaries: 12,
  maxCandidateFiles: 16,
  maxRetainedHistoricalViews: 4,
});

/** Inclusion reasons for active working-set files (Part E §13). */
export type FileInclusionReason =
  | "direct task target"
  | "dependency"
  | "test counterpart"
  | "architecture owner"
  | "validation target"
  | "candidate under investigation";

export interface ActiveFile {
  readonly path: string;
  readonly reason: FileInclusionReason;
  readonly view: SourceView;
  readonly revision?: string;
}

/** The current plan step's bounded working set (a subset of task scope). */
export interface ActiveWorkingSet {
  /** Plan step id this set belongs to. */
  readonly stepId: string;
  readonly files: readonly ActiveFile[];
}

export const ACTIVE_WORKING_SET_LIMITS = Object.freeze({
  maxFiles: 8,
  maxStepIdBytes: 128,
});

const textEncoder = new TextEncoder();

function boundedText(
  value: string | undefined,
  maxBytes: number,
  field: string,
): string | undefined {
  if (value === undefined) {
    return undefined;
  }
  const text = value.trim();
  if (text.length === 0) {
    throw new Error(`${field} must not be empty when provided.`);
  }
  if (textEncoder.encode(text).length > maxBytes) {
    throw new Error(`${field} exceeds ${maxBytes} UTF-8 bytes.`);
  }
  return text;
}

function requireBoundedText(value: string, maxBytes: number, field: string): string {
  const text = value.trim();
  if (text.length === 0) {
    throw new Error(`${field} must not be empty.`);
  }
  if (textEncoder.encode(text).length > maxBytes) {
    throw new Error(`${field} exceeds ${maxBytes} UTF-8 bytes.`);
  }
  return text;
}

function validatePath(path: string): string {
  const text = requireBoundedText(path, WORKSPACE_SCOPE_LIMITS.maxPathBytes, "A source path");
  if (
    text.includes("\\") ||
    text.startsWith("/") ||
    /^[A-Za-z]:/.test(text) ||
    text.includes("\0")
  ) {
    throw new Error(
      `Source paths must be workspace-relative (no drive, absolute, or backslash): ${text}`,
    );
  }
  if (text.split("/").includes("..")) {
    throw new Error(`Source paths must not traverse parents: ${text}`);
  }
  return text;
}

const REVISION_HANDLE_PATTERN = /^rev_[0-9a-f]{32}$/;

function validateFileRef(input: SourceFileRef): SourceFileRef {
  const path = validatePath(input.path);
  const view = input.view;
  if (view !== "exact" && view !== "structural" && view !== "summary" && view !== "none") {
    throw new Error(`Invalid source view for ${path}: ${String(view)}`);
  }
  if (input.confidence !== "verified" && input.confidence !== "candidate") {
    throw new Error(`Invalid confidence for ${path}: ${String(input.confidence)}`);
  }
  const revision = boundedText(input.revision, 128, `Revision for ${path}`);
  const evidence = boundedText(
    input.evidence,
    WORKSPACE_SCOPE_LIMITS.maxEvidenceBytes,
    `Evidence for ${path}`,
  );
  const reason = boundedText(
    input.reason,
    WORKSPACE_SCOPE_LIMITS.maxReasonBytes,
    `Reason for ${path}`,
  );
  if (input.confidence === "verified") {
    if (revision === undefined || !REVISION_HANDLE_PATTERN.test(revision)) {
      throw new Error(`Verified file ${path} requires an exact revision handle (rev_ + 32 hex).`);
    }
    if (evidence === undefined) {
      throw new Error(`Verified file ${path} requires evidence; a guess is never verified.`);
    }
  }
  return {
    path,
    confidence: input.confidence,
    view,
    ...(revision === undefined ? {} : { revision }),
    ...(evidence === undefined ? {} : { evidence }),
    ...(reason === undefined ? {} : { reason }),
  };
}

function validateBudget(input: WorkspaceContextBudget): WorkspaceContextBudget {
  const fields: readonly (keyof WorkspaceContextBudget)[] = [
    "maxActiveExactFiles",
    "maxExactBytes",
    "maxStructuralSummaries",
    "maxCandidateFiles",
    "maxRetainedHistoricalViews",
  ];
  for (const field of fields) {
    const value = input[field];
    if (!Number.isSafeInteger(value) || value < 1) {
      throw new Error(`Budget ${field} must be a positive safe integer.`);
    }
  }
  return { ...input };
}

export interface CreateWorkspaceScopeInput {
  readonly verifiedFiles?: readonly SourceFileRef[];
  readonly candidateFiles?: readonly SourceFileRef[];
  readonly allowedCreateRoots?: readonly string[];
  readonly excludedPaths?: readonly string[];
  readonly budget?: WorkspaceContextBudget;
  readonly promotions?: readonly ScopePromotionRecord[];
}

/** Create the derived task scope. Deterministic, validated, immutable. */
export function createWorkspaceScope(input: CreateWorkspaceScopeInput = {}): WorkspaceScope {
  const verified = input.verifiedFiles ?? [];
  const candidates = input.candidateFiles ?? [];
  if (verified.length > WORKSPACE_SCOPE_LIMITS.maxVerifiedFiles) {
    throw new Error(
      `A workspace scope accepts at most ${WORKSPACE_SCOPE_LIMITS.maxVerifiedFiles} verified files.`,
    );
  }
  if (candidates.length > WORKSPACE_SCOPE_LIMITS.maxCandidateFiles) {
    throw new Error(
      `A workspace scope accepts at most ${WORKSPACE_SCOPE_LIMITS.maxCandidateFiles} candidate files.`,
    );
  }
  const verifiedPaths = new Set<string>();
  const candidatePaths = new Set<string>();
  const verifiedFiles = verified.map((file) => {
    const ref = validateFileRef(file);
    if (verifiedPaths.has(ref.path)) {
      throw new Error(`Duplicate verified file: ${ref.path}`);
    }
    verifiedPaths.add(ref.path);
    return ref;
  });
  const candidateFiles = candidates.map((file) => {
    const ref = validateFileRef(file);
    if (verifiedPaths.has(ref.path) || candidatePaths.has(ref.path)) {
      throw new Error(`Duplicate file across scope sets: ${ref.path}`);
    }
    candidatePaths.add(ref.path);
    return ref;
  });
  const createRoots = (input.allowedCreateRoots ?? []).map((root) => validatePath(root));
  if (createRoots.length > WORKSPACE_SCOPE_LIMITS.maxCreateRoots) {
    throw new Error(
      `A workspace scope accepts at most ${WORKSPACE_SCOPE_LIMITS.maxCreateRoots} create roots.`,
    );
  }
  const excluded = (input.excludedPaths ?? []).map((path) => validatePath(path));
  if (excluded.length > WORKSPACE_SCOPE_LIMITS.maxExcludedPaths) {
    throw new Error(
      `A workspace scope accepts at most ${WORKSPACE_SCOPE_LIMITS.maxExcludedPaths} excluded paths.`,
    );
  }
  const promotions = (input.promotions ?? []).map((record) => {
    const path = validatePath(record.path);
    const evidence = requireBoundedText(
      record.evidence,
      WORKSPACE_SCOPE_LIMITS.maxEvidenceBytes,
      "Promotion evidence",
    );
    const revision = requireBoundedText(record.revision, 128, "Promotion revision");
    if (!REVISION_HANDLE_PATTERN.test(revision)) {
      throw new Error(`Promotion for ${path} requires an exact revision handle.`);
    }
    const reason = requireBoundedText(
      record.reason,
      WORKSPACE_SCOPE_LIMITS.maxReasonBytes,
      "Promotion reason",
    );
    return { path, evidence, revision, reason };
  });
  if (promotions.length > WORKSPACE_SCOPE_LIMITS.maxPromotions) {
    throw new Error(
      `A workspace scope accepts at most ${WORKSPACE_SCOPE_LIMITS.maxPromotions} promotion records.`,
    );
  }
  const budget = validateBudget(input.budget ?? DEFAULT_WORKSPACE_CONTEXT_BUDGET);
  return deepFreeze({
    verifiedFiles,
    candidateFiles,
    allowedCreateRoots: createRoots,
    excludedPaths: excluded,
    budget,
    promotions,
  });
}

/**
 * Whether a workspace-relative path is excluded from default
 * source-context discovery. Matches path prefixes so `node_modules/foo`
 * is excluded by `node_modules/`. Deterministic; never a security denial.
 */
export function isExcludedSourcePath(
  path: string,
  exclusions: readonly string[] = DEFAULT_SOURCE_EXCLUSIONS,
): boolean {
  const normalized = path.replace(/^\.\//, "");
  return exclusions.some((exclusion) => {
    const prefix = exclusion.replace(/^\.\//, "");
    return (
      normalized === prefix || normalized.startsWith(prefix.endsWith("/") ? prefix : `${prefix}/`)
    );
  });
}

/**
 * Add a candidate file to the scope. Candidate contents are never placed
 * into model context automatically (the view stays `none` until a
 * structural/summary read happens).
 */
export function addCandidateFile(
  scope: WorkspaceScope,
  path: string,
  note?: string,
): WorkspaceScope {
  const ref = validateFileRef({
    path,
    confidence: "candidate",
    view: "none",
    ...(note === undefined ? {} : { reason: note }),
  });
  if (scope.verifiedFiles.some((file) => file.path === ref.path)) {
    return scope;
  }
  if (scope.candidateFiles.some((file) => file.path === ref.path)) {
    return scope;
  }
  const candidateFiles = [...scope.candidateFiles, ref];
  if (candidateFiles.length > scope.budget.maxCandidateFiles) {
    // Budgets control context, not discovery authority: drop the oldest
    // candidate detail, retaining identity in the remaining list.
    candidateFiles.shift();
  }
  return deepFreeze({ ...scope, candidateFiles });
}

/**
 * Promote a candidate to verified. Promotion REQUIRES evidence and an
 * exact revision handle; a guess without evidence never promotes. The
 * promotion is recorded so scope expansion is observable.
 */
export function promoteCandidateFile(
  scope: WorkspaceScope,
  path: string,
  promotion: { readonly evidence: string; readonly revision: string; readonly reason: string },
): { readonly scope: WorkspaceScope; readonly record: ScopePromotionRecord } {
  const candidate = scope.candidateFiles.find((file) => file.path === path);
  if (candidate === undefined) {
    throw new Error(`Cannot promote unknown candidate: ${path}`);
  }
  const record: ScopePromotionRecord = {
    path,
    evidence: requireBoundedText(
      promotion.evidence,
      WORKSPACE_SCOPE_LIMITS.maxEvidenceBytes,
      "Promotion evidence",
    ),
    revision: requireBoundedText(promotion.revision, 128, "Promotion revision"),
    reason: requireBoundedText(
      promotion.reason,
      WORKSPACE_SCOPE_LIMITS.maxReasonBytes,
      "Promotion reason",
    ),
  };
  if (!REVISION_HANDLE_PATTERN.test(record.revision)) {
    throw new Error(`Promotion for ${path} requires an exact revision handle.`);
  }
  const verifiedFile: SourceFileRef = {
    path,
    confidence: "verified",
    view: "structural",
    revision: record.revision,
    evidence: record.evidence,
    reason: record.reason,
  };
  const verifiedFiles = [...scope.verifiedFiles, verifiedFile];
  if (verifiedFiles.length > WORKSPACE_SCOPE_LIMITS.maxVerifiedFiles) {
    throw new Error(
      `A workspace scope accepts at most ${WORKSPACE_SCOPE_LIMITS.maxVerifiedFiles} verified files.`,
    );
  }
  const candidateFiles = scope.candidateFiles.filter((file) => file.path !== path);
  const promotions = [...scope.promotions, record].slice(-WORKSPACE_SCOPE_LIMITS.maxPromotions);
  return { scope: deepFreeze({ ...scope, verifiedFiles, candidateFiles, promotions }), record };
}

/** Promote a file directly to verified (used when evidence exists at discovery time). */
export function addVerifiedFile(scope: WorkspaceScope, file: SourceFileRef): WorkspaceScope {
  const ref = validateFileRef({ ...file, confidence: "verified" });
  if (scope.verifiedFiles.some((entry) => entry.path === ref.path)) {
    return scope;
  }
  const verifiedFiles = [...scope.verifiedFiles, ref];
  if (verifiedFiles.length > WORKSPACE_SCOPE_LIMITS.maxVerifiedFiles) {
    throw new Error(
      `A workspace scope accepts at most ${WORKSPACE_SCOPE_LIMITS.maxVerifiedFiles} verified files.`,
    );
  }
  return deepFreeze({ ...scope, verifiedFiles });
}

/** Update the representation a file occupies in source context. */
export function setFileView(scope: WorkspaceScope, path: string, view: SourceView): WorkspaceScope {
  if (view !== "exact" && view !== "structural" && view !== "summary" && view !== "none") {
    throw new Error(`Invalid source view: ${String(view)}`);
  }
  const inVerified = scope.verifiedFiles.some((file) => file.path === path);
  const inCandidates = scope.candidateFiles.some((file) => file.path === path);
  if (!inVerified && !inCandidates) {
    throw new Error(`Cannot set view for unknown file: ${path}`);
  }
  const set = (files: readonly SourceFileRef[]): SourceFileRef[] =>
    files.map((file) => (file.path === path ? deepFreeze({ ...file, view }) : file));
  return deepFreeze({
    ...scope,
    verifiedFiles: inVerified ? set(scope.verifiedFiles) : scope.verifiedFiles,
    candidateFiles: inCandidates ? set(scope.candidateFiles) : scope.candidateFiles,
  });
}

/** Create the current step's bounded working set. */
export function createActiveWorkingSet(input: {
  readonly stepId: string;
  readonly files: readonly ActiveFile[];
}): ActiveWorkingSet {
  const stepId = requireBoundedText(
    input.stepId,
    ACTIVE_WORKING_SET_LIMITS.maxStepIdBytes,
    "A step id",
  );
  if (input.files.length > ACTIVE_WORKING_SET_LIMITS.maxFiles) {
    throw new Error(
      `An active working set accepts at most ${ACTIVE_WORKING_SET_LIMITS.maxFiles} files.`,
    );
  }
  const reasons: readonly FileInclusionReason[] = [
    "direct task target",
    "dependency",
    "test counterpart",
    "architecture owner",
    "validation target",
    "candidate under investigation",
  ];
  const files: ActiveFile[] = [];
  const seen = new Set<string>();
  for (const file of input.files) {
    const path = validatePath(file.path);
    if (seen.has(path)) {
      throw new Error(`Duplicate active file: ${path}`);
    }
    seen.add(path);
    if (!reasons.includes(file.reason)) {
      throw new Error(`Invalid inclusion reason for ${path}: ${file.reason}`);
    }
    if (file.view !== "exact" && file.view !== "structural" && file.view !== "summary") {
      throw new Error(`Invalid working-set view for ${path}: ${String(file.view)}`);
    }
    const revision = boundedText(file.revision, 128, `Revision for ${path}`);
    if (revision !== undefined && !REVISION_HANDLE_PATTERN.test(revision)) {
      throw new Error(`Active file ${path} has an invalid revision handle.`);
    }
    files.push({
      path,
      reason: file.reason,
      view: file.view,
      ...(revision === undefined ? {} : { revision }),
    });
  }
  return deepFreeze({ stepId, files });
}

/**
 * Deterministic budget enforcement: demote low-value exact source views
 * until the budget holds. Eviction order follows Part G §17 — stale
 * candidate details first, then exact source not required by the current
 * working set — and ALWAYS retains revision identity and evidence
 * references (authoritative evidence is never deleted). Returns the
 * updated scope plus the eviction records.
 *
 * `exactBytesOf` carries host-observed exact byte counts per path (the
 * composition root measures the real bytes placed into context; core
 * never reads files). Paths without a reported size still count toward
 * the file-count bound.
 */
export function evictLowValueContext(input: {
  readonly scope: WorkspaceScope;
  readonly workingSet: ActiveWorkingSet | null;
  readonly exactBytesOf?: Readonly<Record<string, number>>;
}): { readonly scope: WorkspaceScope; readonly evicted: readonly EvictionRecord[] } {
  const { scope, workingSet } = input;
  const reportedBytes = input.exactBytesOf ?? {};
  const workingPaths = new Set((workingSet?.files ?? []).map((file) => file.path));
  const exactFiles = scope.verifiedFiles.filter((file) => file.view === "exact");
  const candidateExact = scope.candidateFiles.filter((file) => file.view === "exact");
  let exactCount = exactFiles.length + candidateExact.length;
  let exactBytes = 0;
  const estimatedBytes = new Map<string, number>();
  for (const file of [...exactFiles, ...candidateExact]) {
    const size = reportedBytes[file.path];
    if (size !== undefined && (!Number.isSafeInteger(size) || size < 0)) {
      throw new Error(
        `Reported exact byte count for ${file.path} must be a non-negative safe integer.`,
      );
    }
    const count = size ?? 0;
    estimatedBytes.set(file.path, count);
    exactBytes += count;
  }
  let overCount = exactCount > scope.budget.maxActiveExactFiles;
  let overBytes = exactBytes > scope.budget.maxExactBytes;
  if (!overCount && !overBytes) {
    return { scope, evicted: [] };
  }
  // Eviction order: (1) candidate exact details, (2) verified exact files
  // not in the current working set, (3) verified exact files in the
  // working set (last resort). Deterministic within each tier by scope order.
  const tiers: readonly (readonly SourceFileRef[])[] = [
    [...candidateExact],
    [...exactFiles].filter((file) => !workingPaths.has(file.path)),
    [...exactFiles].filter((file) => workingPaths.has(file.path)),
  ];
  const evicted: EvictionRecord[] = [];
  let scopeAfter = scope;
  for (const tier of tiers) {
    for (const file of tier) {
      if (!overCount && !overBytes) {
        break;
      }
      const size = estimatedBytes.get(file.path) ?? 0;
      scopeAfter = setFileView(scopeAfter, file.path, "summary");
      exactCount -= 1;
      exactBytes = Math.max(0, exactBytes - size);
      evicted.push({
        path: file.path,
        droppedView: "exact",
        retainedView: "summary",
        reason: workingPaths.has(file.path)
          ? "over budget; exact source demoted to summary with revision/evidence retained"
          : "over budget; low-value exact source demoted to summary with revision/evidence retained",
      });
      overCount = exactCount > scopeAfter.budget.maxActiveExactFiles;
      overBytes = exactBytes > scopeAfter.budget.maxExactBytes;
    }
    if (!overCount && !overBytes) {
      break;
    }
  }
  return { scope: scopeAfter, evicted };
}
