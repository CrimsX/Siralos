import type { MilestoneManifest } from "./milestone-manifest.js";
import { createMilestoneManifest } from "./milestone-manifest.js";

/**
 * Stage 3 — Milestone 8 manifest: Read-Only Godot Scene and Resource
 * Intelligence (executor briefing foundation).
 *
 * The first real manifest. It represents the milestone in compact,
 * structured form: goal, deliverables, invariants, non-goals, acceptance
 * IDs, required tests, and the architecture concerns used for
 * deterministic context selection. It does not restate Git policy,
 * standard validation, generic security rules, or architecture
 * principles — the Execution Contract supplies those.
 *
 * Acceptance IDs are stable (S3M8.*) and decoupled from individual test
 * filenames so tests, reports, milestone evaluation, and future
 * evolution can reference them.
 */

export const S3M8_MILESTONE_MANIFEST: MilestoneManifest = createMilestoneManifest({
  id: "S3M8",
  title: "Read-Only Godot Scene and Resource Intelligence",
  goal: "Add revision-aware read-only .tscn/.tres semantic intelligence: deterministic text parsing, scene and resource semantic models, a relationship index, read-only inspection tools, and planning/runtime evidence integration.",
  prerequisites: [
    {
      id: "prereq-task-runtime",
      description: "Stage 3 milestone 1 task runtime with revisioned contracts and evidence.",
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
  ],
  deliverables: [
    {
      id: "deliver-parser",
      description: "Text resource parser (.tscn/.tres) with deterministic bounds.",
    },
    {
      id: "deliver-scene-model",
      description: "Scene semantic model: nodes, scripts, groups, signals, ownership.",
    },
    {
      id: "deliver-resource-model",
      description: "Resource semantic model: ext/subresource identity and UIDs.",
    },
    {
      id: "deliver-relations",
      description: "Relationship index: dependencies, inheritance, instancing, references.",
    },
    {
      id: "deliver-inspect-tools",
      description: "Read-only inspection tools (inspect scene/resource, dependencies).",
    },
    {
      id: "deliver-evidence",
      description: "Revision-aware planning/runtime evidence integration.",
    },
  ],
  nonGoals: [
    "Scene or resource mutation of any kind.",
    "Launching a Godot process for static inspection.",
    "Semantic/vector retrieval over scenes.",
    "GDScript editing or development workflow changes.",
    "Generic workflow DSL or multi-agent task graphs.",
  ],
  invariants: [
    {
      id: "invariant-no-mutation",
      description: "No scene/resource mutation; inspection is strictly read-only.",
    },
    {
      id: "invariant-no-process",
      description: "Static inspection launches no Godot process.",
    },
    {
      id: "invariant-exact-revision",
      description: "Parsed state binds to the exact workspace revision it was read from.",
    },
    {
      id: "invariant-stale-rejected",
      description: "Stale derived state is rejected, never silently reused.",
    },
    {
      id: "invariant-inheritance-vs-instancing",
      description: "Inheritance is never conflated with instancing.",
    },
    {
      id: "invariant-parent-vs-owner",
      description: "Node parent relationships are never conflated with node ownership.",
    },
  ],
  acceptance: [
    {
      id: "S3M8.PARSE.TSCN",
      description: "A representative .tscn scene parses into the semantic model.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M8.PARSE.TRES",
      description: "A representative .tres resource parses into the semantic model.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M8.IDENTITY.UID",
      description: "Ext/subresource and UID identity relationships resolve.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M8.NODES.SCRIPTS",
      description: "Nodes, script attachments, groups, and signals are represented.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M8.DEPENDENCIES",
      description: "Scene/resource dependencies resolve through the relationship index.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M8.PROJECT.MAIN_AUTOLOADS",
      description: "Project main scene and autoloads are inspectable.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M8.REVISION.STALE",
      description: "Stale derived state is rejected against the bound workspace revision.",
      evidenceKinds: ["parser_result", "validation_result"],
    },
    {
      id: "S3M8.SECURITY.NO_PROCESS",
      description: "Static inspection launches no Godot process and executes nothing.",
      standardIds: ["STANDARD.NO_PROCESS_EXECUTION"],
    },
    {
      id: "S3M8.SECURITY.NO_MUTATION",
      description: "Scene/resource inspection performs no workspace mutation.",
      standardIds: ["STANDARD.NO_WORKSPACE_MUTATION"],
    },
    {
      id: "S3M8.TOOLS.NO_NATIVE_WRITE",
      description:
        "No native scene/resource mutation tool leaks into the provider-visible tool surface.",
      standardIds: ["STANDARD.NO_TOOL_LEAKAGE"],
    },
    {
      id: "S3M8.DEVELOP.REFUSE_NATIVE_MUTATION",
      description: "/develop refuses required scene/resource edits (no mutation capability).",
      evidenceKinds: ["review_result", "validation_result"],
    },
  ],
  requiredTests: [
    {
      id: "test-parser-fixtures",
      description: "Representative .tscn/.tres parsing fixtures with bounds.",
    },
    {
      id: "test-effect-no-process",
      description:
        "Final-boundary effect test: inspection launches no process and mutates nothing.",
    },
    {
      id: "test-tool-surface",
      description: "Final-boundary test: provider-visible tools carry no native mutation surface.",
    },
    {
      id: "test-develop-refusal",
      description: "Final-boundary test: /develop refuses required scene/resource edits.",
    },
  ],
  architectureConcerns: [
    "task-runtime",
    "workspace-revision",
    "godot-static-inspection",
    "planning",
  ],
});
