import type { WorkspaceRevisionHandle } from "../../workspace/workspace-revision.js";
import type {
  GodotParseStatus,
  GodotResourceModel,
  GodotSceneModel,
  GodotTextDiagnostic,
  GodotTextDocument,
} from "./models.js";
import type { GodotRelationshipEntry, GodotRelationshipKind } from "./relationship-index.js";
import type { GodotSceneNodeTree } from "./scene-tree.js";
import type { ReviewContextManifest } from "../impact/review-context.js";

/**
 * Godot scene/resource intelligence port (Stage 3 milestone 8).
 *
 * The single application-owned subsystem for current parsed Godot
 * semantic state. Everything this port returns is derived, read-only,
 * revision-bound, and static: no Godot process, no `@tool` execution, no
 * plugin activation, no project loading, no mutation, no checkpoint.
 *
 * The CLI, ContextProjector, planning, and review consume semantic models
 * through this port (or through the projected read-only tools) — they
 * never parse `.tscn`/`.tres` themselves.
 */

export type GodotIntelligenceStatus =
  "ok" | "not_found" | "unreadable" | "unsupported" | "denied" | "failed";

export interface GodotInspectionOutcome<T> {
  readonly status: GodotIntelligenceStatus;
  /** Human-readable failure detail; null on success. */
  readonly message: string | null;
  readonly path: string;
  /** Exact workspace revision of the inspected file state. */
  readonly revision: WorkspaceRevisionHandle | null;
  /** Parsed document (null on failure). */
  readonly document: GodotTextDocument<T> | null;
}

export interface GodotSceneInspectionResult extends GodotInspectionOutcome<GodotSceneModel> {
  /** Derived deterministic node tree when the document has usable structure. */
  readonly tree: GodotSceneNodeTree | null;
}

export type GodotResourceInspectionResult = GodotInspectionOutcome<GodotResourceModel>;

/** One bounded dependency edge derived from a parsed document. */
export interface GodotDependencyEdge {
  readonly kind: GodotRelationshipKind;
  /** Workspace-relative source path. */
  readonly sourcePath: string;
  /** Workspace-relative target path (resolved from res://). */
  readonly targetPath: string;
  /** Target `uid://` identity when both path and UID are known. */
  readonly targetUid?: string;
  /** Traversal depth from the query root (0 = immediate). */
  readonly depth: number;
}

export interface GodotDependencyResult {
  readonly status: GodotIntelligenceStatus;
  readonly message: string | null;
  readonly rootPath: string;
  readonly revision: WorkspaceRevisionHandle | null;
  /** Bounded traversal edges (cycle-safe, depth/file bounded). */
  readonly edges: readonly GodotDependencyEdge[];
  /** Who references the root document (from the relationship index). */
  readonly referrers: readonly {
    readonly sourcePath: string;
    readonly kind: GodotRelationshipKind;
    readonly sourceRevision: WorkspaceRevisionHandle | null;
    readonly stale: boolean;
  }[];
  readonly filesVisited: number;
  readonly truncatedDepth: boolean;
  readonly truncatedFiles: boolean;
  readonly cycleDetected: boolean;
  /** The detected cycle path (root-first), when a cycle was found. */
  readonly cyclePath?: readonly string[];
}

/** Structured autoload relationship (never executed). */
export interface GodotAutoload {
  readonly name: string;
  /** Workspace-relative target path resolved from the declaration. */
  readonly path: string;
  /** Serialized singleton state; `*_` prefixes serialize as disabled. */
  readonly enabled: boolean;
  readonly targetKind: "script" | "scene" | "resource" | "unknown";
  /** Original bounded target text (`*res://...` when singletons were declared). */
  readonly target: string;
}

/** Bounded input-action structural information (no InputEvent semantics). */
export interface GodotInputAction {
  readonly name: string;
  readonly deadzone: number | null;
  readonly eventCount: number;
  readonly eventTypes: readonly string[];
}

export interface GodotMainSceneReference {
  /** Workspace-relative main scene path. */
  readonly path: string;
  /** Revision of the main scene at resolution time. */
  readonly revision: WorkspaceRevisionHandle | null;
  readonly exists: boolean;
}

export interface GodotProjectRelationshipResult {
  readonly status: "ok" | "no_project";
  readonly message: string | null;
  readonly mainScene: GodotMainSceneReference | null;
  readonly autoloads: readonly GodotAutoload[];
  readonly inputActions: readonly GodotInputAction[];
  /** Parse diagnostics from the project scan (bounded). */
  readonly diagnostics: readonly GodotTextDiagnostic[];
}

export interface GodotSceneIntelligenceSupport {
  /** Static parsing is always available; this is an offline capability. */
  readonly state: "ready";
}

/**
 * Bounded scene/resource inspection observation for context projection
 * (project evidence/data authority class — never instructions).
 */
export interface GodotSceneEvidenceView {
  readonly path: string;
  /** Exact workspace revision of the inspected file state. */
  readonly revision: WorkspaceRevisionHandle | null;
  readonly kind: "scene" | "resource";
  readonly status: GodotParseStatus;
  /** Bounded single-line structural summary, e.g. "8 nodes, 1 script". */
  readonly summary: string;
  readonly evidenceId: string | null;
}

export interface GodotSceneIntelligence {
  /** Parse one `.tscn` at its current workspace revision (static, no Godot process). */
  inspectScene(request: { readonly path: string }): Promise<GodotSceneInspectionResult>;
  /** Parse one `.tres` at its current workspace revision (static, no Godot process). */
  inspectResource(request: { readonly path: string }): Promise<GodotResourceInspectionResult>;
  /**
   * Bounded immediate dependencies of one scene/resource, plus a bounded
   * cycle-safe traversal and (from the index) who references it.
   */
  dependencies(request: { readonly path: string }): Promise<GodotDependencyResult>;
  /** Structured project relationships: main scene, autoloads, input actions. */
  projectRelationships(): Promise<GodotProjectRelationshipResult>;
  /**
   * Bounded, revision-aware impact analysis (Stage 3 milestone 9): derive
   * a ReviewContextManifest for the changed surfaces from the existing
   * relationship index. Static, read-only, no Godot process, no mutation.
   */
  reviewContext(request: GodotImpactRequest): Promise<GodotImpactResult>;
  support(): GodotSceneIntelligenceSupport;
}

/** One impact-analysis request (Stage 3 milestone 9). */
export interface GodotImpactRequest {
  readonly taskId: string;
  readonly taskContractRevision: number;
  /** Workspace-relative changed surfaces (bounded; overflow is truncated honestly). */
  readonly changedPaths: readonly string[];
}

export interface GodotImpactResult {
  readonly status: "ok" | "failed";
  readonly message: string | null;
  /** Derived review/validation context; null on failure. */
  readonly manifest: ReviewContextManifest | null;
}

export interface GodotRelationshipIndexPort {
  readonly dependenciesOf: (sourcePath: string) => readonly GodotRelationshipEntry[];
  readonly referrersOf: (targetPath: string) => readonly GodotRelationshipEntry[];
}
