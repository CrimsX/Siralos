import type { WorkspaceRevisionHandle } from "../../workspace/workspace-revision.js";
import {
  REVIEW_CONTEXT_LIMITS,
  validateReviewContextManifest,
  type ImpactDiagnostic,
  type ImpactRegressionArea,
  type ImpactRelation,
  type ImpactRelationKind,
  type ImpactSurface,
  type ImpactSurfaceKind,
  type ImpactValidationRecommendation,
  type ReviewContextManifest,
  type ValidationKind,
  type ValidationPriority,
} from "./review-context.js";

/**
 * Deterministic impact analyzer (Stage 3 milestone 9, ADR 0025).
 *
 * A PURE derivation: given changed surfaces and an injected
 * `ImpactRelationshipSource` (implemented by the adapter over the
 * revision-aware relationship index), it produces a bounded
 * `ReviewContextManifest`. The analyzer owns no source relationships, no
 * revisions, no filesystem, no process — it only derives impact from
 * evidence the source provides.
 *
 * Guarantees:
 * - verified vs candidate impact is preserved; heuristics are never
 *   promoted silently;
 * - traversal is breadth-first, bounded (depth/surfaces/relations) and
 *   cycle-safe;
 * - stale relationships are never presented as current — they are
 *   excluded from related surfaces and surfaced as diagnostics;
 * - absence of a static relationship is never claimed as proof of
 *   runtime non-impact (runtime validation is recommended separately);
 * - identical inputs produce identical manifests (deterministic).
 */

/** One directed relationship edge provided by the relationship source. */
export interface ImpactEdge {
  readonly kind: ImpactRelationKind;
  readonly fromPath: string;
  readonly toPath: string;
  /** True when the recorded relationship is no longer current (stale). */
  readonly stale: boolean;
}

/** One serialized signal connection (static scene evidence). */
export interface ImpactSignalConnection {
  readonly signal: string;
  readonly sourceNode: string;
  readonly targetNode: string;
  readonly targetMethod: string;
}

/**
 * The relationship evidence source. Implemented by the adapter over the
 * Stage 3.8 relationship index, the workspace revision registry, the
 * static project scan, and bounded scene parsing — never over runtime
 * entities Siralos cannot prove.
 */
export interface ImpactRelationshipSource {
  /** Relationships the surface participates in as the source (from-path). */
  outgoing(path: string): readonly ImpactEdge[];
  /** Relationships pointing at the surface (to-path). */
  incoming(path: string): readonly ImpactEdge[];
  /** Serialized signal connections of one scene (bounded; empty for non-scenes). */
  signalConnections(path: string): Promise<readonly ImpactSignalConnection[]>;
  /** Autoload name when the path is a project autoload target; else null. */
  autoloadName(path: string): string | null;
  /** Workspace-relative main scene path, when known. */
  mainScene(): string | null;
  /** Exact current workspace revision of the path, when known. */
  currentRevision(path: string): WorkspaceRevisionHandle | null;
  /** Bounded candidate test files plausibly covering the surface (heuristic). */
  candidateTests(path: string): Promise<readonly string[]>;
}

export interface AnalyzeImpactInput {
  readonly taskId: string;
  readonly taskContractRevision: number;
  /** Workspace-relative changed surfaces (bounded; overflow is truncated with a diagnostic). */
  readonly changedPaths: readonly string[];
  readonly source: ImpactRelationshipSource;
}

const textEncoder = new TextEncoder();

/** Byte-safe truncation: the validator rejects over-budget fields, so the
 * analyzer guarantees its own output fits its limits. */
function fitBytes(text: string, maxBytes: number): string {
  let fitted = text;
  while (textEncoder.encode(fitted).length > maxBytes) {
    fitted = fitted.slice(0, fitted.length - 1);
  }
  return fitted;
}

function surfaceKindOf(path: string, source: ImpactRelationshipSource): ImpactSurfaceKind {
  if (source.autoloadName(path) !== null) {
    return "autoload";
  }
  if (path === "project.godot") {
    return "project-config";
  }
  const normalized = path.toLowerCase();
  if (normalized.endsWith(".tscn")) {
    return "scene";
  }
  if (normalized.endsWith(".tres") || normalized.endsWith(".theme")) {
    return "resource";
  }
  if (normalized.endsWith(".gd")) {
    return /(^|\/)(tests?|spec)(\/|\.)|\.test\.|\.spec\./.test(path) ? "test" : "script";
  }
  return "script";
}

const REGRESSION_BY_KIND: Readonly<
  Record<ImpactRelationKind, { readonly id: string; readonly title: string } | null>
> = {
  script_attachment: { id: "REGRESSION.SCRIPT_BEHAVIOR", title: "Scene script behavior" },
  scene_inheritance: { id: "REGRESSION.SCENE_INHERITANCE", title: "Scene inheritance" },
  scene_instancing: { id: "REGRESSION.SCENE_INSTANTIATION", title: "Scene instantiation" },
  resource_dependency: { id: "REGRESSION.RESOURCE_LOADING", title: "Resource loading" },
  script_dependency: { id: "REGRESSION.SCRIPT_DEPENDENCIES", title: "GDScript dependencies" },
  signal_connection: { id: "REGRESSION.SIGNAL_CALLBACKS", title: "Signal callback behavior" },
  autoload_global: { id: "REGRESSION.AUTOLOAD_GLOBAL_REACH", title: "Autoload/global state" },
  test_covers: null,
};

interface QueueEntry {
  readonly path: string;
  readonly depth: number;
}

/**
 * Derive the bounded review/validation context for the changed surfaces.
 * Deterministic: identical inputs produce identical manifests.
 */
export async function analyzeImpact(input: AnalyzeImpactInput): Promise<ReviewContextManifest> {
  const changedPaths = input.changedPaths
    .map((path) => path.trim())
    .filter(
      (path) =>
        path.length > 0 &&
        !path.includes("\\") &&
        !path.startsWith("/") &&
        !path.split("/").includes(".."),
    )
    .slice(0, REVIEW_CONTEXT_LIMITS.maxPrimaryChanges);
  const diagnostics: ImpactDiagnostic[] = [];
  if (input.changedPaths.length > REVIEW_CONTEXT_LIMITS.maxPrimaryChanges) {
    diagnostics.push({
      code: "IMPACT.PRIMARY_BOUND",
      message: `Impact analysis truncated to the first ${REVIEW_CONTEXT_LIMITS.maxPrimaryChanges} changed surfaces.`,
    });
  }
  const primaryChanges: ImpactSurface[] = changedPaths.map((path) => {
    const kind = surfaceKindOf(path, input.source);
    const autoload = input.source.autoloadName(path);
    return {
      path,
      kind,
      revision: input.source.currentRevision(path),
      confidence: "verified",
      evidence: "impact:changed-surface",
      ...(autoload === null ? {} : { note: `project autoload: ${autoload}` }),
    };
  });

  const visited = new Set<string>();
  const relatedSurfaces: ImpactRelation[] = [];
  // Undirected pair identity per relationship kind: each relationship is
  // recorded exactly once regardless of which endpoint is traversed first
  // (reverse edges never duplicate it), and distinct relationships to the
  // same surface are all kept.
  const recordedRelationKeys = new Set<string>();
  const stalePairs = new Set<string>();
  let relationsVisited = 0;
  let surfacesVisited = 0;
  let depthBoundHit = false;
  let surfaceBoundHit = false;
  let relationBoundHit = false;

  for (const primary of changedPaths) {
    if (visited.has(primary)) {
      continue;
    }
    const queue: QueueEntry[] = [{ path: primary, depth: 0 }];
    visited.add(primary);
    while (queue.length > 0) {
      const { path, depth } = queue.shift()!;
      surfacesVisited += 1;
      const edges: ImpactEdge[] = [...input.source.outgoing(path), ...input.source.incoming(path)];
      for (const edge of edges) {
        relationsVisited += 1;
        if (relationsVisited > REVIEW_CONTEXT_LIMITS.maxRelationsVisited) {
          relationBoundHit = true;
          break;
        }
        if (edge.stale) {
          stalePairs.add(`${edge.fromPath}->${edge.toPath}`);
          continue;
        }
        if (edge.fromPath !== path && edge.toPath !== path) {
          continue;
        }
        // Normalize to traversal direction: source = the surface being
        // traversed, target = the other side of the relationship.
        const sourcePath = path;
        const targetPath = edge.fromPath === path ? edge.toPath : edge.fromPath;
        const kind: ImpactRelationKind = edge.kind;
        const pairKey = `${kind}\u0000${[sourcePath, targetPath].sort().join("\u0000")}`;
        if (!recordedRelationKeys.has(pairKey)) {
          recordedRelationKeys.add(pairKey);
          const relation: ImpactRelation = {
            kind,
            sourcePath,
            targetPath,
            sourceRevision: input.source.currentRevision(sourcePath),
            targetRevision: input.source.currentRevision(targetPath),
            confidence: "verified",
            evidence: `index:${kind}`,
          };
          if (relatedSurfaces.length < REVIEW_CONTEXT_LIMITS.maxRelatedSurfaces) {
            relatedSurfaces.push(relation);
          } else {
            relationBoundHit = true;
          }
        }
        if (!visited.has(targetPath) && depth < REVIEW_CONTEXT_LIMITS.maxDepth) {
          if (surfacesVisited >= REVIEW_CONTEXT_LIMITS.maxSurfacesVisited) {
            surfaceBoundHit = true;
          } else {
            visited.add(targetPath);
            queue.push({ path: targetPath, depth: depth + 1 });
          }
        }
        if (depth >= REVIEW_CONTEXT_LIMITS.maxDepth && !visited.has(targetPath)) {
          depthBoundHit = true;
        }
      }
      if (relationBoundHit) {
        break;
      }
    }
    if (relationBoundHit) {
      break;
    }
  }
  if (depthBoundHit || surfaceBoundHit || relationBoundHit) {
    diagnostics.push({
      code: "IMPACT.TRAVERSAL_BOUND",
      message: [
        depthBoundHit ? "depth bound reached" : null,
        surfaceBoundHit ? "surface-count bound reached" : null,
        relationBoundHit ? "relation-count bound reached" : null,
      ]
        .filter((part) => part !== null)
        .join(", "),
    });
  }

  // Serialized signal connections of related scenes (bounded, static).
  const sceneSurfaces = new Set<string>(
    [
      ...primaryChanges.map((surface) => surface.path),
      ...relatedSurfaces.map((relation) => relation.targetPath),
    ].filter((path) => surfaceKindOf(path, input.source) === "scene"),
  );
  for (const scenePath of sceneSurfaces) {
    for (const connection of await input.source.signalConnections(scenePath)) {
      if (relatedSurfaces.length >= REVIEW_CONTEXT_LIMITS.maxRelatedSurfaces) {
        relationBoundHit = true;
        break;
      }
      relatedSurfaces.push({
        kind: "signal_connection",
        sourcePath: scenePath,
        targetPath: scenePath,
        sourceRevision: input.source.currentRevision(scenePath),
        targetRevision: input.source.currentRevision(scenePath),
        confidence: "verified",
        evidence: "index:signal_connection",
        note: `serialized connection ${connection.signal}: node ${connection.sourceNode} -> node ${connection.targetNode}.${connection.targetMethod}`,
      });
    }
  }

  // Candidate test surfaces (heuristic; never verified coverage).
  const candidateTestPaths: string[] = [];
  for (const primary of changedPaths) {
    for (const testPath of await input.source.candidateTests(primary)) {
      if (candidateTestPaths.length >= REVIEW_CONTEXT_LIMITS.maxCandidateTests) {
        break;
      }
      if (candidateTestPaths.includes(testPath) || testPath === primary) {
        continue;
      }
      candidateTestPaths.push(testPath);
      if (relatedSurfaces.length < REVIEW_CONTEXT_LIMITS.maxRelatedSurfaces) {
        relatedSurfaces.push({
          kind: "test_covers",
          sourcePath: primary,
          targetPath: testPath,
          sourceRevision: input.source.currentRevision(primary),
          targetRevision: input.source.currentRevision(testPath),
          confidence: "candidate",
          evidence: "convention:test-surface",
        });
      } else {
        relationBoundHit = true;
      }
    }
  }

  // Staleness diagnostics.
  for (const pair of stalePairs) {
    if (diagnostics.length >= REVIEW_CONTEXT_LIMITS.maxDiagnostics) {
      break;
    }
    diagnostics.push({
      code: "IMPACT.STALE_RELATIONSHIP",
      message: `Relationship ${pair} is stale (its recorded source revision is no longer current); it was excluded from current impact.`,
    });
  }

  // Autoload/global reach: high-reach risk signal, never verified impact
  // on every project surface.
  const autoloadChanged = changedPaths.filter((path) => input.source.autoloadName(path) !== null);
  if (autoloadChanged.length > 0) {
    diagnostics.push({
      code: "IMPACT.AUTOLOAD_GLOBAL",
      message: `Changed autoload(s) ${autoloadChanged.join(", ")} have global reach that cannot be enumerated statically; impact beyond direct relations is candidate, not verified.`,
    });
  }

  const regressionAreas = buildRegressionAreas(changedPaths, relatedSurfaces, input.source);
  const validation = buildValidationRecommendations(
    changedPaths,
    relatedSurfaces,
    input.source,
    autoloadChanged,
  );
  const evidence = buildEvidenceRefs(changedPaths, relatedSurfaces);

  const hasPartialCondition =
    stalePairs.size > 0 ||
    candidateTestPaths.length > 0 ||
    autoloadChanged.length > 0 ||
    validation.some((recommendation) => recommendation.priority === "runtime_evidence_unavailable");
  const hasBoundCondition = depthBoundHit || surfaceBoundHit || relationBoundHit;
  const completeness = hasPartialCondition ? "partial" : hasBoundCondition ? "bounded" : "complete";

  return validateReviewContextManifest({
    taskId: input.taskId,
    taskContractRevision: input.taskContractRevision,
    primaryChanges,
    relatedSurfaces,
    regressionAreas,
    validation,
    evidence,
    completeness,
    diagnostics,
  });
}

function buildRegressionAreas(
  changedPaths: readonly string[],
  relations: readonly ImpactRelation[],
  source: ImpactRelationshipSource,
): ImpactRegressionArea[] {
  const areas: ImpactRegressionArea[] = [];
  const surfacesByKind = new Map<ImpactRelationKind, string[]>();
  for (const relation of relations) {
    if (relation.confidence !== "verified") {
      continue;
    }
    const surfaces = surfacesByKind.get(relation.kind) ?? [];
    if (!surfaces.includes(relation.targetPath)) {
      surfaces.push(relation.targetPath);
    }
    surfacesByKind.set(relation.kind, surfaces);
  }
  const push = (id: string, title: string, reason: string, surfaces: readonly string[]) => {
    if (areas.length >= REVIEW_CONTEXT_LIMITS.maxRegressionAreas) {
      return;
    }
    areas.push({
      id: fitBytes(id, REVIEW_CONTEXT_LIMITS.maxEvidenceRefBytes),
      title: fitBytes(title, REVIEW_CONTEXT_LIMITS.maxReasonBytes),
      reason: fitBytes(reason, REVIEW_CONTEXT_LIMITS.maxReasonBytes),
      surfaces: surfaces.slice(0, 16),
    });
  };
  for (const kind of Object.keys(REGRESSION_BY_KIND) as ImpactRelationKind[]) {
    const definition = REGRESSION_BY_KIND[kind];
    const surfaces = surfacesByKind.get(kind) ?? [];
    if (definition === null || surfaces.length === 0) {
      continue;
    }
    push(
      definition.id,
      definition.title,
      `${surfaces.length} related surface(s) via ${kind}: ${surfaces.slice(0, 4).join(", ")}`,
      surfaces,
    );
  }
  if (changedPaths.includes("project.godot")) {
    push(
      "REGRESSION.PROJECT_CONFIG",
      "Project configuration",
      "project.godot changed: main scene, autoloads, and input actions may be affected.",
      ["project.godot"],
    );
  }
  const mainScene = source.mainScene();
  if (mainScene !== null && changedPaths.includes(mainScene)) {
    push(
      "REGRESSION.MAIN_SCENE",
      "Main scene",
      "The project main scene changed; launch/startup surfaces are directly related.",
      [mainScene],
    );
  }
  return areas;
}

function buildValidationRecommendations(
  changedPaths: readonly string[],
  relations: readonly ImpactRelation[],
  source: ImpactRelationshipSource,
  autoloadChanged: readonly string[],
): ImpactValidationRecommendation[] {
  const surfacesByKind = new Map<string, Set<string>>();
  const rationalesByKind = new Map<string, string>();
  const push = (
    kind: ValidationKind,
    priority: ValidationPriority,
    rationale: string,
    surfaces: readonly string[],
  ) => {
    const key = `${kind}:${priority}`;
    const merged = surfacesByKind.get(key) ?? new Set<string>();
    for (const surface of surfaces) {
      merged.add(surface);
    }
    surfacesByKind.set(key, merged);
    const prior = rationalesByKind.get(key) ?? "";
    const appended = fitBytes(
      prior.length === 0 ? rationale : `${prior} ${rationale}`,
      REVIEW_CONTEXT_LIMITS.maxRationaleBytes,
    );
    rationalesByKind.set(key, appended);
  };
  const scriptSurfaces = new Set<string>([
    ...changedPaths.filter((path) => surfaceKindOf(path, source) === "script"),
    ...relations
      .filter((relation) => relation.kind === "script_attachment")
      .map((relation) => relation.targetPath),
  ]);
  const changedSceneResource = new Set<string>(
    changedPaths.filter(
      (path) =>
        surfaceKindOf(path, source) === "scene" || surfaceKindOf(path, source) === "resource",
    ),
  );
  const relatedSceneResource = new Set<string>(
    relations
      .filter(
        (relation) =>
          (relation.kind === "scene_inheritance" ||
            relation.kind === "scene_instancing" ||
            relation.kind === "resource_dependency") &&
          (surfaceKindOf(relation.targetPath, source) === "scene" ||
            surfaceKindOf(relation.targetPath, source) === "resource"),
      )
      .map((relation) => relation.targetPath),
  );
  const testSurfaces = new Set<string>(
    relations
      .filter((relation) => relation.kind === "test_covers")
      .map((relation) => relation.targetPath),
  );
  const hasSignals = relations.some((relation) => relation.kind === "signal_connection");
  if (scriptSurfaces.size > 0) {
    push(
      "gdscript_check_only",
      "required_now",
      `${scriptSurfaces.size} script surface(s) changed or attached; check-only parse is required before any mutation is applied.`,
      [...scriptSurfaces],
    );
    push(
      "fresh_lsp_diagnostics",
      "recommended",
      "Script changes warrant fresh language-server diagnostics after application.",
      [...scriptSurfaces],
    );
  }
  if (changedSceneResource.size > 0) {
    push(
      "scene_resource_parse",
      "required_now",
      `${changedSceneResource.size} scene/resource surface(s) changed; reparse validation is required.`,
      [...changedSceneResource],
    );
  }
  if (relatedSceneResource.size > 0) {
    push(
      "scene_resource_parse",
      "recommended",
      `${relatedSceneResource.size} related scene/resource surface(s) should reparse after the change.`,
      [...relatedSceneResource],
    );
  }
  if (changedPaths.includes("project.godot")) {
    push(
      "project_config_checks",
      "required_now",
      "project.godot changed; main scene, autoload, and input-action structure must revalidate.",
      ["project.godot"],
    );
  }
  if (autoloadChanged.length > 0) {
    const autoloadList = autoloadChanged.slice(0, 8).join(", ");
    push(
      "broader_repo_validation",
      "recommended",
      `Changed autoload(s) ${autoloadList} have global reach; broader repository validation is recommended (impact beyond direct relations is candidate).`,
      autoloadChanged.slice(0, REVIEW_CONTEXT_LIMITS.maxRelatedSurfaces),
    );
  }
  if (testSurfaces.size > 0) {
    push(
      "specific_test_script",
      "recommended",
      `${testSurfaces.size} candidate test surface(s) identified by convention; run them to confirm coverage (candidate, not verified).`,
      [...testSurfaces],
    );
  }
  if (hasSignals || autoloadChanged.length > 0) {
    push(
      "runtime_validation",
      "runtime_evidence_unavailable",
      "Signal callbacks and autoload reach cannot be proven statically; runtime validation is required when runtime evidence becomes available.",
      [],
    );
  }
  const result: ImpactValidationRecommendation[] = [];
  for (const key of rationalesByKind.keys()) {
    const [kind, priority] = key.split(":") as [ValidationKind, ValidationPriority];
    result.push({
      kind,
      priority,
      rationale: rationalesByKind.get(key)!,
      surfaces: [...(surfacesByKind.get(key) ?? [])].slice(
        0,
        REVIEW_CONTEXT_LIMITS.maxRelatedSurfaces,
      ),
    });
  }
  return result.slice(0, REVIEW_CONTEXT_LIMITS.maxValidation);
}

function buildEvidenceRefs(
  changedPaths: readonly string[],
  relations: readonly ImpactRelation[],
): string[] {
  void changedPaths;
  const refs = new Set<string>(["impact:changed-surface"]);
  for (const relation of relations) {
    refs.add(relation.evidence);
  }
  return [...refs].slice(0, REVIEW_CONTEXT_LIMITS.maxEvidence);
}
