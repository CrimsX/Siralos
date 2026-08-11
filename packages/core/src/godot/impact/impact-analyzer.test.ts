import { describe, expect, it } from "vitest";
import {
  analyzeImpact,
  type ImpactEdge,
  type ImpactRelationshipSource,
} from "./impact-analyzer.js";
import { REVIEW_CONTEXT_LIMITS, validateReviewContextManifest } from "./review-context.js";

const REV = "rev_".padEnd(36, "a");

interface FixtureGraph {
  /** sourcePath -> outgoing edges */
  readonly outgoing?: Readonly<Record<string, readonly ImpactEdge[]>>;
  /** targetPath -> incoming edges (computed from outgoing when omitted) */
  readonly incoming?: Readonly<Record<string, readonly ImpactEdge[]>>;
  readonly signals?: Readonly<
    Record<
      string,
      readonly { signal: string; sourceNode: string; targetNode: string; targetMethod: string }[]
    >
  >;
  readonly autoloads?: readonly string[];
  readonly mainScene?: string | null;
  readonly currentRevisions?: Readonly<Record<string, string>>;
  readonly candidateTests?: Readonly<Record<string, readonly string[]>>;
}

function edge(kind: ImpactEdge["kind"], from: string, to: string, stale = false): ImpactEdge {
  return { kind, fromPath: from, toPath: to, stale };
}

function buildSource(graph: FixtureGraph): ImpactRelationshipSource {
  const outgoing = graph.outgoing ?? {};
  const incoming = graph.incoming ?? {};
  const signals = graph.signals ?? {};
  const autoloads = graph.autoloads ?? [];
  const currentRevisions = graph.currentRevisions ?? {};
  const candidateTests = graph.candidateTests ?? {};
  return {
    outgoing: (path) => outgoing[path] ?? [],
    incoming: (path) =>
      incoming[path] ??
      Object.values(outgoing)
        .flat()
        .filter((entry) => entry.toPath === path),
    signalConnections: (path) => Promise.resolve(signals[path] ?? []),
    autoloadName: (path) => (autoloads.includes(path) ? (path.split("/").pop() ?? path) : null),
    mainScene: () => graph.mainScene ?? null,
    currentRevision: (path) => currentRevisions[path] ?? REV,
    candidateTests: (path) => Promise.resolve(candidateTests[path] ?? []),
  };
}

const TASK = { taskId: "task-impact", taskContractRevision: 1 };

describe("impact analyzer — script/scene/resource impact", () => {
  it("identifies scenes attaching a changed script (script attachment)", async () => {
    const source = buildSource({
      outgoing: {
        "scenes/player.tscn": [
          edge("script_attachment", "scenes/player.tscn", "scripts/player.gd"),
        ],
        "scenes/enemy.tscn": [edge("script_attachment", "scenes/enemy.tscn", "scripts/enemy.gd")],
      },
    });
    const manifest = await analyzeImpact({ ...TASK, changedPaths: ["scripts/player.gd"], source });
    expect(manifest.primaryChanges[0]!.kind).toBe("script");
    expect(manifest.primaryChanges[0]!.confidence).toBe("verified");
    const attachments = manifest.relatedSurfaces.filter(
      (relation) => relation.kind === "script_attachment",
    );
    // Traversal direction: changed script -> attaching scene.
    expect(attachments.map((relation) => relation.targetPath)).toEqual(["scenes/player.tscn"]);
    expect(attachments.map((relation) => relation.sourcePath)).toEqual(["scripts/player.gd"]);
    // The unrelated enemy scene never enters the context.
    expect(
      manifest.relatedSurfaces.some((relation) => relation.sourcePath === "scenes/enemy.tscn"),
    ).toBe(false);
    expect(manifest.regressionAreas.some((area) => area.id === "REGRESSION.SCRIPT_BEHAVIOR")).toBe(
      true,
    );
  });

  it("keeps inheritance impact distinct from instancing impact", async () => {
    const source = buildSource({
      outgoing: {
        "scenes/base.tscn": [
          edge("scene_inheritance", "scenes/child.tscn", "scenes/base.tscn"),
          edge("scene_instancing", "scenes/level.tscn", "scenes/base.tscn"),
        ],
      },
    });
    const manifest = await analyzeImpact({ ...TASK, changedPaths: ["scenes/base.tscn"], source });
    const kinds = manifest.relatedSurfaces.map((relation) => relation.kind).sort();
    expect(kinds).toContain("scene_inheritance");
    expect(kinds).toContain("scene_instancing");
    const inheritance = manifest.relatedSurfaces.find(
      (relation) => relation.kind === "scene_inheritance",
    );
    const instancing = manifest.relatedSurfaces.find(
      (relation) => relation.kind === "scene_instancing",
    );
    // Traversal direction: changed base scene -> inheriting/instancing scenes.
    expect(inheritance?.targetPath).toBe("scenes/child.tscn");
    expect(instancing?.targetPath).toBe("scenes/level.tscn");
    // Distinct regression areas, never conflated.
    expect(
      manifest.regressionAreas.some((area) => area.id === "REGRESSION.SCENE_INHERITANCE"),
    ).toBe(true);
    expect(
      manifest.regressionAreas.some((area) => area.id === "REGRESSION.SCENE_INSTANTIATION"),
    ).toBe(true);
  });

  it("identifies direct dependents of a changed resource", async () => {
    const source = buildSource({
      outgoing: {
        "scenes/player.tscn": [
          edge("resource_dependency", "scenes/player.tscn", "resources/theme.tres"),
        ],
        "resources/menu.tres": [
          edge("resource_dependency", "resources/menu.tres", "resources/theme.tres"),
        ],
      },
    });
    const manifest = await analyzeImpact({
      ...TASK,
      changedPaths: ["resources/theme.tres"],
      source,
    });
    expect(
      manifest.relatedSurfaces.filter((relation) => relation.kind === "resource_dependency").length,
    ).toBe(2);
    expect(manifest.regressionAreas.some((area) => area.id === "REGRESSION.RESOURCE_LOADING")).toBe(
      true,
    );
    expect(
      manifest.validation.some(
        (recommendation) =>
          recommendation.kind === "scene_resource_parse" &&
          recommendation.priority === "recommended",
      ),
    ).toBe(true);
  });

  it("is cycle-safe: a scene loop never revisits surfaces and records each relationship once", async () => {
    const source = buildSource({
      outgoing: {
        "scenes/a.tscn": [edge("scene_instancing", "scenes/a.tscn", "scenes/b.tscn")],
        "scenes/b.tscn": [edge("scene_instancing", "scenes/b.tscn", "scenes/a.tscn")],
      },
    });
    const manifest = await analyzeImpact({ ...TASK, changedPaths: ["scenes/a.tscn"], source });
    const relations = manifest.relatedSurfaces.filter(
      (relation) => relation.kind === "scene_instancing",
    );
    // a <-> b is ONE relationship: the reverse edge never duplicates it.
    expect(relations.length).toBe(1);
    expect(relations[0]!.targetPath).toBe("scenes/b.tscn");
  });

  it("enforces traversal depth and relation bounds with honest completeness", async () => {
    const longChain: Record<string, readonly ImpactEdge[]> = {};
    for (let index = 0; index < 10; index += 1) {
      longChain[`scenes/s${index}.tscn`] = [
        edge("scene_instancing", `scenes/s${index}.tscn`, `scenes/s${index + 1}.tscn`),
      ];
    }
    const source = buildSource({ outgoing: longChain });
    const manifest = await analyzeImpact({ ...TASK, changedPaths: ["scenes/s0.tscn"], source });
    // Depth is bounded (maxDepth 2): s0 -> s1 -> s2 traversed; the s2 -> s3
    // boundary edge is recorded but not expanded.
    const targets = manifest.relatedSurfaces.map((relation) => relation.targetPath);
    expect(targets).toEqual(["scenes/s1.tscn", "scenes/s2.tscn", "scenes/s3.tscn"]);
    expect(
      manifest.diagnostics.some((diagnostic) => diagnostic.code === "IMPACT.TRAVERSAL_BOUND"),
    ).toBe(true);
    expect(manifest.completeness).toBe("bounded");
  });

  it("is deterministic: identical inputs produce identical manifests", async () => {
    const source = buildSource({
      outgoing: {
        "scenes/player.tscn": [
          edge("script_attachment", "scenes/player.tscn", "scripts/player.gd"),
        ],
      },
    });
    const first = await analyzeImpact({ ...TASK, changedPaths: ["scripts/player.gd"], source });
    const second = await analyzeImpact({ ...TASK, changedPaths: ["scripts/player.gd"], source });
    expect(JSON.stringify(first)).toBe(JSON.stringify(second));
    expect(Object.isFrozen(first)).toBe(true);
  });
});

describe("impact analyzer — signals, autoloads, tests, staleness", () => {
  it("represents serialized signal connections honestly and recommends runtime validation", async () => {
    const source = buildSource({
      outgoing: {
        "scenes/player.tscn": [
          edge("script_attachment", "scenes/player.tscn", "scripts/player.gd"),
        ],
      },
      signals: {
        "scenes/player.tscn": [
          {
            signal: "health_changed",
            sourceNode: "Player",
            targetNode: "HUD",
            targetMethod: "_on_health_changed",
          },
        ],
      },
    });
    const manifest = await analyzeImpact({ ...TASK, changedPaths: ["scripts/player.gd"], source });
    const signals = manifest.relatedSurfaces.filter(
      (relation) => relation.kind === "signal_connection",
    );
    expect(signals.length).toBe(1);
    expect(signals[0]!.note).toContain("health_changed");
    expect(manifest.regressionAreas.some((area) => area.id === "REGRESSION.SIGNAL_CALLBACKS")).toBe(
      true,
    );
    expect(
      manifest.validation.some(
        (recommendation) =>
          recommendation.kind === "runtime_validation" &&
          recommendation.priority === "runtime_evidence_unavailable",
      ),
    ).toBe(true);
  });

  it("handles autoload reach conservatively: broad risk, never verified impact on everything", async () => {
    const source = buildSource({
      autoloads: ["scripts/game_state.gd"],
      outgoing: {
        "scenes/player.tscn": [
          edge("script_attachment", "scenes/player.tscn", "scripts/player.gd"),
        ],
      },
    });
    const manifest = await analyzeImpact({
      ...TASK,
      changedPaths: ["scripts/game_state.gd"],
      source,
    });
    expect(manifest.primaryChanges[0]!.kind).toBe("autoload");
    expect(manifest.primaryChanges[0]!.note).toContain("game_state");
    expect(
      manifest.diagnostics.some((diagnostic) => diagnostic.code === "IMPACT.AUTOLOAD_GLOBAL"),
    ).toBe(true);
    expect(
      manifest.validation.some(
        (recommendation) => recommendation.kind === "broader_repo_validation",
      ),
    ).toBe(true);
    // The unrelated player scene is NOT marked impacted by the autoload.
    expect(
      manifest.relatedSurfaces.some((relation) => relation.targetPath === "scenes/player.tscn"),
    ).toBe(false);
    expect(manifest.completeness).toBe("partial");
  });

  it("keeps candidate test surfaces distinct from verified impact", async () => {
    const source = buildSource({
      outgoing: {
        "scenes/player.tscn": [
          edge("script_attachment", "scenes/player.tscn", "scripts/player.gd"),
        ],
      },
      candidateTests: {
        "scripts/player.gd": ["tests/player_test.gd"],
      },
    });
    const manifest = await analyzeImpact({ ...TASK, changedPaths: ["scripts/player.gd"], source });
    const testRelation = manifest.relatedSurfaces.find(
      (relation) => relation.kind === "test_covers",
    );
    expect(testRelation?.confidence).toBe("candidate");
    expect(testRelation?.evidence).toBe("convention:test-surface");
    expect(
      manifest.diagnostics.some((diagnostic) => diagnostic.code === "IMPACT.CANDIDATE_TESTS"),
    ).toBe(false);
    expect(
      manifest.validation.some((recommendation) => recommendation.kind === "specific_test_script"),
    ).toBe(true);
    expect(manifest.completeness).toBe("partial");
  });

  it("never presents stale relationships as current", async () => {
    const source = buildSource({
      outgoing: {
        "scenes/player.tscn": [
          edge("script_attachment", "scenes/player.tscn", "scripts/player.gd", true),
        ],
      },
      currentRevisions: { "scenes/player.tscn": REV },
    });
    const manifest = await analyzeImpact({ ...TASK, changedPaths: ["scripts/player.gd"], source });
    expect(manifest.relatedSurfaces).toHaveLength(0);
    expect(
      manifest.diagnostics.some((diagnostic) => diagnostic.code === "IMPACT.STALE_RELATIONSHIP"),
    ).toBe(true);
    expect(manifest.completeness).toBe("partial");
  });

  it("recommends validation reflecting the observed impact", async () => {
    const source = buildSource({
      outgoing: {
        "scenes/player.tscn": [
          edge("script_attachment", "scenes/player.tscn", "scripts/player.gd"),
          edge("scene_inheritance", "scenes/boss.tscn", "scenes/player.tscn"),
        ],
      },
    });
    const manifest = await analyzeImpact({ ...TASK, changedPaths: ["scripts/player.gd"], source });
    const kinds = manifest.validation.map((recommendation) => recommendation.kind);
    expect(kinds).toContain("gdscript_check_only");
    expect(kinds).toContain("fresh_lsp_diagnostics");
    expect(
      manifest.validation.find((recommendation) => recommendation.kind === "gdscript_check_only")
        ?.priority,
    ).toBe("required_now");
  });

  it("flags project.godot changes with configuration validation", async () => {
    const source = buildSource({});
    const manifest = await analyzeImpact({ ...TASK, changedPaths: ["project.godot"], source });
    expect(manifest.primaryChanges[0]!.kind).toBe("project-config");
    expect(manifest.regressionAreas.some((area) => area.id === "REGRESSION.PROJECT_CONFIG")).toBe(
      true,
    );
    expect(
      manifest.validation.some(
        (recommendation) =>
          recommendation.kind === "project_config_checks" &&
          recommendation.priority === "required_now",
      ),
    ).toBe(true);
  });
});

describe("review context model validation", () => {
  it("validates and detaches a complete manifest", () => {
    const manifest = validateReviewContextManifest({
      taskId: "task-1",
      taskContractRevision: 1,
      primaryChanges: [
        {
          path: "scripts/player.gd",
          kind: "script",
          revision: REV,
          confidence: "verified",
          evidence: "impact:changed-surface",
        },
      ],
      relatedSurfaces: [
        {
          kind: "script_attachment",
          sourcePath: "scenes/player.tscn",
          targetPath: "scripts/player.gd",
          sourceRevision: REV,
          targetRevision: REV,
          confidence: "verified",
          evidence: "index:scene_uses_script",
        },
      ],
      regressionAreas: [
        {
          id: "REGRESSION.SCRIPT_BEHAVIOR",
          title: "Scene script behavior",
          reason: "1 related surface via script_attachment.",
          surfaces: ["scenes/player.tscn"],
        },
      ],
      validation: [
        {
          kind: "gdscript_check_only",
          priority: "required_now",
          rationale: "Script changed.",
          surfaces: ["scripts/player.gd"],
        },
      ],
      evidence: ["impact:changed-surface"],
      completeness: "complete",
      diagnostics: [],
    });
    expect(Object.isFrozen(manifest)).toBe(true);
    expect(manifest.relatedSurfaces[0]!.sourceRevision).toBe(REV);
  });

  it("rejects malformed manifests", () => {
    expect(() =>
      validateReviewContextManifest({
        taskId: "task-1",
        taskContractRevision: 0,
        primaryChanges: [],
        relatedSurfaces: [],
        regressionAreas: [],
        validation: [],
        evidence: [],
        completeness: "complete",
        diagnostics: [],
      }),
    ).toThrow(/positive safe-integer/);
    expect(() =>
      validateReviewContextManifest({
        taskId: "task-1",
        taskContractRevision: 1,
        primaryChanges: [
          {
            path: "../escape.gd",
            kind: "script",
            revision: null,
            confidence: "verified",
            evidence: "impact:changed-surface",
          },
        ],
        relatedSurfaces: [],
        regressionAreas: [],
        validation: [],
        evidence: [],
        completeness: "complete",
        diagnostics: [],
      }),
    ).toThrow(/traverse parents/);
    expect(() =>
      validateReviewContextManifest({
        taskId: "task-1",
        taskContractRevision: 1,
        primaryChanges: [],
        relatedSurfaces: [
          {
            kind: "not_a_kind" as never,
            sourcePath: "a.gd",
            targetPath: "b.gd",
            sourceRevision: null,
            targetRevision: null,
            confidence: "verified",
            evidence: "x",
          },
        ],
        regressionAreas: [],
        validation: [],
        evidence: [],
        completeness: "complete",
        diagnostics: [],
      }),
    ).toThrow(/Invalid impact relation kind/);
  });

  it("honors the hard bounds", () => {
    const many = Array.from(
      { length: REVIEW_CONTEXT_LIMITS.maxPrimaryChanges + 1 },
      (_, index) => ({
        path: `scripts/s${index}.gd`,
        kind: "script" as const,
        revision: null,
        confidence: "verified" as const,
        evidence: "impact:changed-surface",
      }),
    );
    expect(() =>
      validateReviewContextManifest({
        taskId: "task-1",
        taskContractRevision: 1,
        primaryChanges: many,
        relatedSurfaces: [],
        regressionAreas: [],
        validation: [],
        evidence: [],
        completeness: "complete",
        diagnostics: [],
      }),
    ).toThrow(/at most/);
  });
});
