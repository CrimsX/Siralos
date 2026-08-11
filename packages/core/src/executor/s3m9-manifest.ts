import type { MilestoneManifest } from "./milestone-manifest.js";
import { createMilestoneManifest } from "./milestone-manifest.js";

/**
 * Stage 3 — Milestone 9 manifest: Godot Review Context and Impact
 * Intelligence.
 *
 * The second real manifest. Represents the milestone in compact,
 * structured form: goal, deliverables, invariants, non-goals, acceptance
 * IDs, required tests, and the architecture concerns used for
 * deterministic context selection. It does not restate Git policy,
 * standard validation, generic security rules, or architecture
 * principles — the Execution Contract supplies those.
 *
 * Acceptance IDs are stable (S3M9.*) and decoupled from individual test
 * filenames; evidence-backed evaluation maps them to host-observed
 * evidence kinds (parser/workspace-read/validation/review results).
 */

export const S3M9_MILESTONE_MANIFEST: MilestoneManifest = createMilestoneManifest({
  id: "S3M9",
  title: "Godot Review Context and Impact Intelligence",
  goal: "Derive bounded, revision-aware, evidence-backed impact analysis for changed Godot surfaces (scripts, scenes, resources, autoloads, project configuration) into a structured ReviewContextManifest consumed read-only by planner, validation, and the independent reviewer.",
  prerequisites: [
    {
      id: "prereq-scene-resource",
      description:
        "Stage 3 milestone 8 revision-aware scene/resource parsing and relationship index.",
    },
    {
      id: "prereq-revisions",
      description: "Stage 3 milestone 3 workspace revision handles and structural reads.",
    },
    {
      id: "prereq-planning",
      description:
        "Stage 3 milestone 7 host-controlled planning with verified/candidate touchpoints.",
    },
    {
      id: "prereq-reviewer",
      description: "Stage 3 milestone 2 independent ChangeReviewer and quality gates.",
    },
  ],
  deliverables: [
    {
      id: "deliver-impact-model",
      description:
        "Structured immutable ReviewContextManifest: primary/related surfaces, regression areas, validation recommendations, evidence, completeness.",
    },
    {
      id: "deliver-analyzer",
      description:
        "Deterministic bounded impact analyzer over the existing revision-aware relationship index.",
    },
    {
      id: "deliver-impact-tool",
      description:
        "Read-only impact-analysis surface (godot.review_context) with bounded traversal and honest completeness.",
    },
    {
      id: "deliver-reviewer-integration",
      description:
        "Independent reviewer receives a bounded ReviewContextManifest with its review request.",
    },
    {
      id: "deliver-develop-integration",
      description:
        "/develop derives review context from changed surfaces before independent review where appropriate.",
    },
  ],
  invariants: [
    {
      id: "invariant-no-process",
      description: "Impact analysis is static: it launches no Godot/project process.",
    },
    {
      id: "invariant-no-mutation",
      description: "Impact analysis performs no source mutation and creates no checkpoint.",
    },
    {
      id: "invariant-derived",
      description: "Review context is derived state, not task authority; it grants no capability.",
    },
    {
      id: "invariant-revision-bound",
      description:
        "Every impact claim is revision-bound; stale relationships are never presented as current.",
    },
    {
      id: "invariant-verified-vs-candidate",
      description: "Verified impact requires evidence; heuristic impact stays candidate.",
    },
    {
      id: "invariant-honest-completeness",
      description:
        "Absence of a static relationship is never claimed as proof of runtime non-impact; completeness is disclosed.",
    },
  ],
  nonGoals: [
    "Scene or resource mutation of any kind.",
    "Runtime QA, project execution, or runtime impact proof.",
    "A generic dependency-graph engine or eager whole-project indexing.",
    "Loading all related source into reviewer context.",
    "Treating heuristic test matches as verified coverage.",
  ],
  acceptance: [
    {
      id: "S3M9.IMPACT.SCRIPT_ATTACHMENT",
      description: "A changed script identifies the scenes currently attaching it.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M9.IMPACT.SCENE_INHERITANCE",
      description:
        "A changed base scene identifies scenes inheriting from it; inheritance stays distinct from instancing.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M9.IMPACT.SCENE_INSTANCING",
      description: "A changed instanced scene identifies scenes instantiating it.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M9.IMPACT.RESOURCE_DEPENDENCY",
      description: "A changed resource identifies direct dependent surfaces.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M9.IMPACT.SIGNALS",
      description:
        "Serialized signal connections are represented honestly, distinct from runtime validity.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M9.IMPACT.AUTOLOAD_REACH",
      description:
        "A changed autoload broadens risk conservatively without claiming verified impact on every project surface.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M9.IMPACT.TEST_SURFACES",
      description:
        "Candidate test surfaces are identified with evidence status and never promoted to verified coverage.",
      evidenceKinds: ["workspace_read", "review_result"],
    },
    {
      id: "S3M9.REVISION.STALE",
      description:
        "Stale relationship evidence is rejected or clearly marked stale, never current.",
      evidenceKinds: ["parser_result", "validation_result"],
    },
    {
      id: "S3M9.CONTEXT.BOUNDED",
      description:
        "Impact traversal is bounded and cycle-safe; a leaf change never expands to the whole project.",
      evidenceKinds: ["validation_result", "review_result"],
    },
    {
      id: "S3M9.SECURITY.NO_PROCESS",
      description: "Static impact analysis launches no Godot/project process.",
      standardIds: ["STANDARD.NO_PROCESS_EXECUTION"],
    },
    {
      id: "S3M9.SECURITY.NO_MUTATION",
      description: "Impact analysis performs no workspace mutation and creates no checkpoint.",
      standardIds: ["STANDARD.NO_WORKSPACE_MUTATION"],
    },
    {
      id: "S3M9.TOOLS.READ_ONLY",
      description: "No impact-analysis mutation tool leaks into provider-visible tools.",
      standardIds: ["STANDARD.NO_TOOL_LEAKAGE"],
    },
    {
      id: "S3M9.DEVELOP.REVIEW_INTEGRATION",
      description:
        "The independent reviewer receives a bounded review context derived from changed surfaces.",
      evidenceKinds: ["review_result", "validation_result"],
    },
  ],
  requiredTests: [
    {
      id: "test-impact-fixtures",
      description:
        "Unit fixtures: script attachment, inheritance vs instancing, resource dependents, signals, autoload reach, stale relationships, bounds, cycles, determinism.",
    },
    {
      id: "test-effect-no-process",
      description: "Effect test: the real impact-analysis path spawns no Godot/project process.",
    },
    {
      id: "test-effect-no-mutation",
      description:
        "Effect test: impact analysis leaves workspace, Git, and checkpoint state unchanged.",
    },
    {
      id: "test-effect-bounded-context",
      description:
        "Effect test: a leaf change in a large fixture yields a bounded neighborhood, not the whole project.",
    },
    {
      id: "test-effect-stale",
      description:
        "Effect test: impact derived from rev_A is rejected/marked stale after the source moves to rev_B.",
    },
    {
      id: "test-effect-reviewer-containment",
      description:
        "Effect test: the reviewer request carries bounded impact/evidence without unrelated repository/docs.",
    },
    {
      id: "test-regression-s3m8",
      description:
        "Regression: Stage 3.8 inspection and existing GDScript /develop flows remain green.",
    },
  ],
  architectureConcerns: [
    "godot",
    "godot-static-inspection",
    "task-runtime",
    "workspace-revision",
    "projection",
    "planning",
  ],
});
