import { deepFreeze } from "../../domain/deep-freeze.js";
import type { TaskId } from "../../tasks/task-model.js";
import type { WorkspaceRevisionHandle } from "../../workspace/workspace-revision.js";

/**
 * Review context and impact intelligence (Stage 3 milestone 9, ADR 0025).
 *
 * Given one or more proposed or actual changed surfaces, Siralos derives
 * a bounded, revision-aware, evidence-backed review/validation context:
 * primary changes, directly related surfaces (with verified vs candidate
 * confidence), regression areas, validation recommendations, and honest
 * completeness/diagnostics.
 *
 * The manifest is DERIVED state, not task authority: it grants no
 * capability, performs no mutation, launches no Godot process, and never
 * proves runtime impact — absence of a static relationship is not proof
 * of runtime non-impact, and stale relationship evidence is never
 * presented as current.
 */

export type ImpactSurfaceKind =
  "script" | "scene" | "resource" | "autoload" | "signal-endpoint" | "test" | "project-config";

/** Verified impact is evidence-backed; candidate impact is plausible but unproven. */
export type ImpactConfidence = "verified" | "candidate";

export type ImpactRelationKind =
  | "script_attachment"
  | "scene_inheritance"
  | "scene_instancing"
  | "resource_dependency"
  | "script_dependency"
  | "signal_connection"
  | "autoload_global"
  | "test_covers";

export type ImpactCompleteness = "complete" | "bounded" | "partial";

export type ValidationPriority = "required_now" | "recommended" | "runtime_evidence_unavailable";

export type ValidationKind =
  | "gdscript_check_only"
  | "fresh_lsp_diagnostics"
  | "specific_test_script"
  | "scene_resource_parse"
  | "project_config_checks"
  | "broader_repo_validation"
  | "runtime_validation";

/** One changed surface with its exact revision and confidence. */
export interface ImpactSurface {
  readonly path: string;
  readonly kind: ImpactSurfaceKind;
  /** Exact workspace revision of the inspected file state, when known. */
  readonly revision: WorkspaceRevisionHandle | null;
  readonly confidence: ImpactConfidence;
  /** Bounded evidence reference in `kind:ref` form. */
  readonly evidence: string;
  readonly note?: string;
}

/** One derived relationship between a changed surface and a related surface. */
export interface ImpactRelation {
  readonly kind: ImpactRelationKind;
  /** The changed surface (source of the impact). */
  readonly sourcePath: string;
  /** The related surface (potentially impacted). */
  readonly targetPath: string;
  readonly sourceRevision: WorkspaceRevisionHandle | null;
  readonly targetRevision: WorkspaceRevisionHandle | null;
  readonly confidence: ImpactConfidence;
  readonly evidence: string;
  readonly note?: string;
}

/** One evidence-backed regression area (never generic boilerplate). */
export interface ImpactRegressionArea {
  readonly id: string;
  readonly title: string;
  /** Why this area is relevant, tied to the observed relations. */
  readonly reason: string;
  /** Bounded related surface paths backing the area. */
  readonly surfaces: readonly string[];
}

/** One structured validation recommendation derived from observed impact. */
export interface ImpactValidationRecommendation {
  readonly kind: ValidationKind;
  readonly priority: ValidationPriority;
  readonly rationale: string;
  /** Surfaces the recommendation applies to (bounded). */
  readonly surfaces: readonly string[];
}

/** Honest limitation/uncertainty disclosure. */
export interface ImpactDiagnostic {
  /** Stable code, e.g. IMPACT.TRAVERSAL_BOUND. */
  readonly code: string;
  readonly message: string;
}

/**
 * Immutable derived review/validation context for one task. Revision- and
 * evidence-bound; never task authority.
 */
export interface ReviewContextManifest {
  readonly taskId: TaskId;
  readonly taskContractRevision: number;
  readonly primaryChanges: readonly ImpactSurface[];
  readonly relatedSurfaces: readonly ImpactRelation[];
  readonly regressionAreas: readonly ImpactRegressionArea[];
  readonly validation: readonly ImpactValidationRecommendation[];
  /** Bounded evidence references (`kind:ref`) backing the manifest. */
  readonly evidence: readonly string[];
  readonly completeness: ImpactCompleteness;
  readonly diagnostics: readonly ImpactDiagnostic[];
}

/** Host-owned hard bounds for review-context manifests (never raised by input). */
export const REVIEW_CONTEXT_LIMITS = Object.freeze({
  maxPrimaryChanges: 16,
  maxRelatedSurfaces: 64,
  maxRegressionAreas: 8,
  maxValidation: 12,
  maxEvidence: 32,
  maxDiagnostics: 16,
  /** Default traversal depth from the primary surfaces (primary -> direct -> selected second-order). */
  maxDepth: 2,
  /** Default visited-surface bound (cycle-safe, breadth-first). */
  maxSurfacesVisited: 64,
  /** Default visited-relation bound. */
  maxRelationsVisited: 128,
  /** Candidate test surfaces per primary change. */
  maxCandidateTests: 8,
  maxPathBytes: 1024,
  maxEvidenceRefBytes: 256,
  maxNoteBytes: 512,
  maxReasonBytes: 512,
  maxRationaleBytes: 512,
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

function validatePath(path: string, field: string): string {
  const text = requireBoundedText(path, REVIEW_CONTEXT_LIMITS.maxPathBytes, field);
  if (
    text.includes("\\") ||
    text.startsWith("/") ||
    /^[A-Za-z]:/.test(text) ||
    text.includes("\0")
  ) {
    throw new Error(`${field} must be a workspace-relative path: ${text}`);
  }
  if (text.split("/").includes("..")) {
    throw new Error(`${field} must not traverse parents: ${text}`);
  }
  return text;
}

function copyBoundedStrings(
  values: readonly string[],
  max: number,
  maxBytes: number,
  field: string,
): string[] {
  if (values.length > max) {
    throw new Error(`${field} accepts at most ${max} entries.`);
  }
  return values.map((value) => requireBoundedText(value, maxBytes, `${field} entry`));
}

const SURFACE_KINDS: readonly ImpactSurfaceKind[] = [
  "script",
  "scene",
  "resource",
  "autoload",
  "signal-endpoint",
  "test",
  "project-config",
];
const RELATION_KINDS: readonly ImpactRelationKind[] = [
  "script_attachment",
  "scene_inheritance",
  "scene_instancing",
  "resource_dependency",
  "script_dependency",
  "signal_connection",
  "autoload_global",
  "test_covers",
];
const VALIDATION_KINDS: readonly ValidationKind[] = [
  "gdscript_check_only",
  "fresh_lsp_diagnostics",
  "specific_test_script",
  "scene_resource_parse",
  "project_config_checks",
  "broader_repo_validation",
  "runtime_validation",
];
const VALIDATION_PRIORITIES: readonly ValidationPriority[] = [
  "required_now",
  "recommended",
  "runtime_evidence_unavailable",
];

export interface ValidateReviewContextInput {
  readonly taskId: TaskId;
  readonly taskContractRevision: number;
  readonly primaryChanges?: readonly ImpactSurface[];
  readonly relatedSurfaces?: readonly ImpactRelation[];
  readonly regressionAreas?: readonly ImpactRegressionArea[];
  readonly validation?: readonly ImpactValidationRecommendation[];
  readonly evidence?: readonly string[];
  readonly completeness?: ImpactCompleteness;
  readonly diagnostics?: readonly ImpactDiagnostic[];
}

/**
 * Validate and detach a review-context manifest at a runtime boundary
 * (mirrors the other host-owned models): only the normalized shape
 * produced by the impact analyzer can become authoritative.
 */
export function validateReviewContextManifest(
  input: ValidateReviewContextInput,
): ReviewContextManifest {
  const taskId = requireBoundedText(input.taskId, REVIEW_CONTEXT_LIMITS.maxPathBytes, "A task id");
  if (!Number.isSafeInteger(input.taskContractRevision) || input.taskContractRevision < 1) {
    throw new Error("A review context requires a positive safe-integer task contract revision.");
  }
  const primaryChanges = (input.primaryChanges ?? []).map((surface, index) => {
    const path = validatePath(surface.path, `Primary surface ${index}`);
    if (!SURFACE_KINDS.includes(surface.kind)) {
      throw new Error(`Invalid impact surface kind: ${String(surface.kind)}`);
    }
    if (surface.confidence !== "verified" && surface.confidence !== "candidate") {
      throw new Error(`Invalid impact confidence: ${String(surface.confidence)}`);
    }
    return {
      path,
      kind: surface.kind,
      revision: surface.revision ?? null,
      confidence: surface.confidence,
      evidence: requireBoundedText(
        surface.evidence,
        REVIEW_CONTEXT_LIMITS.maxEvidenceRefBytes,
        `Evidence for ${path}`,
      ),
      ...(surface.note === undefined
        ? {}
        : {
            note: boundedText(
              surface.note,
              REVIEW_CONTEXT_LIMITS.maxNoteBytes,
              `Note for ${path}`,
            )!,
          }),
    };
  });
  if (primaryChanges.length > REVIEW_CONTEXT_LIMITS.maxPrimaryChanges) {
    throw new Error(
      `A review context accepts at most ${REVIEW_CONTEXT_LIMITS.maxPrimaryChanges} primary changes.`,
    );
  }
  const relatedSurfaces = (input.relatedSurfaces ?? []).map((relation, index) => {
    if (!RELATION_KINDS.includes(relation.kind)) {
      throw new Error(`Invalid impact relation kind: ${String(relation.kind)}`);
    }
    if (relation.confidence !== "verified" && relation.confidence !== "candidate") {
      throw new Error(`Invalid relation confidence: ${String(relation.confidence)}`);
    }
    return {
      kind: relation.kind,
      sourcePath: validatePath(relation.sourcePath, `Relation ${index} source`),
      targetPath: validatePath(relation.targetPath, `Relation ${index} target`),
      sourceRevision: relation.sourceRevision ?? null,
      targetRevision: relation.targetRevision ?? null,
      confidence: relation.confidence,
      evidence: requireBoundedText(
        relation.evidence,
        REVIEW_CONTEXT_LIMITS.maxEvidenceRefBytes,
        `Relation ${index} evidence`,
      ),
      ...(relation.note === undefined
        ? {}
        : {
            note: boundedText(
              relation.note,
              REVIEW_CONTEXT_LIMITS.maxNoteBytes,
              `Relation ${index} note`,
            )!,
          }),
    };
  });
  if (relatedSurfaces.length > REVIEW_CONTEXT_LIMITS.maxRelatedSurfaces) {
    throw new Error(
      `A review context accepts at most ${REVIEW_CONTEXT_LIMITS.maxRelatedSurfaces} related surfaces.`,
    );
  }
  const regressionAreas = (input.regressionAreas ?? []).map((area) => ({
    id: requireBoundedText(
      area.id,
      REVIEW_CONTEXT_LIMITS.maxEvidenceRefBytes,
      "A regression area id",
    ),
    title: requireBoundedText(
      area.title,
      REVIEW_CONTEXT_LIMITS.maxReasonBytes,
      "A regression area title",
    ),
    reason: requireBoundedText(
      area.reason,
      REVIEW_CONTEXT_LIMITS.maxReasonBytes,
      "A regression area reason",
    ),
    surfaces: copyBoundedStrings(
      area.surfaces,
      REVIEW_CONTEXT_LIMITS.maxRelatedSurfaces,
      REVIEW_CONTEXT_LIMITS.maxPathBytes,
      "A regression area surface",
    ),
  }));
  if (regressionAreas.length > REVIEW_CONTEXT_LIMITS.maxRegressionAreas) {
    throw new Error(
      `A review context accepts at most ${REVIEW_CONTEXT_LIMITS.maxRegressionAreas} regression areas.`,
    );
  }
  const validation = (input.validation ?? []).map((recommendation) => {
    if (!VALIDATION_KINDS.includes(recommendation.kind)) {
      throw new Error(`Invalid validation kind: ${String(recommendation.kind)}`);
    }
    if (!VALIDATION_PRIORITIES.includes(recommendation.priority)) {
      throw new Error(`Invalid validation priority: ${String(recommendation.priority)}`);
    }
    return {
      kind: recommendation.kind,
      priority: recommendation.priority,
      rationale: requireBoundedText(
        recommendation.rationale,
        REVIEW_CONTEXT_LIMITS.maxRationaleBytes,
        "A validation rationale",
      ),
      surfaces: copyBoundedStrings(
        recommendation.surfaces,
        REVIEW_CONTEXT_LIMITS.maxRelatedSurfaces,
        REVIEW_CONTEXT_LIMITS.maxPathBytes,
        "A validation surface",
      ),
    };
  });
  if (validation.length > REVIEW_CONTEXT_LIMITS.maxValidation) {
    throw new Error(
      `A review context accepts at most ${REVIEW_CONTEXT_LIMITS.maxValidation} validation recommendations.`,
    );
  }
  const evidence = copyBoundedStrings(
    input.evidence ?? [],
    REVIEW_CONTEXT_LIMITS.maxEvidence,
    REVIEW_CONTEXT_LIMITS.maxEvidenceRefBytes,
    "Evidence",
  );
  if (
    input.completeness !== "complete" &&
    input.completeness !== "bounded" &&
    input.completeness !== "partial"
  ) {
    throw new Error(`Invalid completeness: ${String(input.completeness)}`);
  }
  const diagnostics = (input.diagnostics ?? []).map((diagnostic) => ({
    code: requireBoundedText(
      diagnostic.code,
      REVIEW_CONTEXT_LIMITS.maxEvidenceRefBytes,
      "A diagnostic code",
    ),
    message: requireBoundedText(
      diagnostic.message,
      REVIEW_CONTEXT_LIMITS.maxReasonBytes,
      "A diagnostic message",
    ),
  }));
  if (diagnostics.length > REVIEW_CONTEXT_LIMITS.maxDiagnostics) {
    throw new Error(
      `A review context accepts at most ${REVIEW_CONTEXT_LIMITS.maxDiagnostics} diagnostics.`,
    );
  }
  return deepFreeze({
    taskId,
    taskContractRevision: input.taskContractRevision,
    primaryChanges,
    relatedSurfaces,
    regressionAreas,
    validation,
    evidence,
    completeness: input.completeness,
    diagnostics,
  });
}
