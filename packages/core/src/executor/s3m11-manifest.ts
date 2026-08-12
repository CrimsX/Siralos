import type { MilestoneManifest } from "./milestone-manifest.js";
import { createMilestoneManifest } from "./milestone-manifest.js";

/**
 * Stage 3 — Milestone 11 manifest: Unified Godot-Native Development
 * Workflow.
 *
 * Represents the milestone in compact structured form: goal,
 * deliverables, invariants, non-goals, acceptance IDs, required tests,
 * and architecture concerns. It does not restate Git policy, standard
 * validation, generic security rules, or architecture principles — the
 * Execution Contract supplies those.
 */

export const S3M11_MILESTONE_MANIFEST: MilestoneManifest = createMilestoneManifest({
  id: "S3M11",
  title: "Unified Godot-Native Development Workflow",
  goal: "Complete the Stage 3 Godot-native development loop: one host-owned workflow that routes script-only, native-only, and bounded mixed script/scene/resource tasks through planning, surface discovery, a unified multi-target change set with per-target revision/approval binding, a checkpoint-then-apply batch, per-surface verification, cross-surface consistency, impact-driven validation, independent review, bounded repair, and host-observed acceptance — without a second workflow engine, without raw scene/resource text editing, and without weakening the fail-closed identity-bound apply gate.",
  prerequisites: [
    {
      id: "prereq-script-loop",
      description: "Stage 2 GDScript development and repair loop (ADR 0012, 0013).",
    },
    {
      id: "prereq-native-mutation",
      description: "Stage 3 milestone 10 approved scene/resource mutation (ADR 0026).",
    },
    {
      id: "prereq-impact",
      description: "Stage 3 milestone 9 review context and impact intelligence (ADR 0025).",
    },
    {
      id: "prereq-intelligence",
      description: "Stage 3 milestone 8 read-only scene/resource intelligence (ADR 0021).",
    },
    {
      id: "prereq-foundations",
      description:
        "Task runtime, host-controlled planning, approvals, checkpoints, revisions, and executor briefing foundations (ADRs 0014, 0020, 0022).",
    },
  ],
  deliverables: [
    {
      id: "deliver-routing",
      description:
        "Deterministic host-owned surface routing: script-only, native-only, or mixed, from host-observed evidence, never model claims.",
    },
    {
      id: "deliver-changeset",
      description:
        "Unified multi-target change set: every target retains its own source revision, prepared fingerprint, approval state, and verification slot.",
    },
    {
      id: "deliver-order",
      description:
        "Explicit, evidenced apply order derived from cross-target dependencies with a deterministic tie-break; no hardcoded script-first/scene-first.",
    },
    {
      id: "deliver-apply",
      description:
        "Checkpoint-then-apply across every target: all source revisions revalidated before any write, one lock, sequential hash-verified application.",
    },
    {
      id: "deliver-verify",
      description:
        "Per-surface post-apply verification: GDScript parser/fresh-LSP evidence and native reparse/semantic-effect evidence; a step succeeds only when its verification passes.",
    },
    {
      id: "deliver-consistency",
      description:
        "Cross-surface consistency checks where statically supportable, with runtime-only concerns disclosed honestly.",
    },
    {
      id: "deliver-blocked",
      description:
        "Structured blocked dispositions for unsupported requirements; prior successful changes are preserved.",
    },
  ],
  invariants: [
    {
      id: "invariant-host-owned",
      description: "TaskState owns workflow progress; no model-controlled workflow state exists.",
    },
    {
      id: "invariant-approval-exact",
      description:
        "Approval binds the exact prepared change set; revision, operation, target, or repair changes invalidate the affected approval.",
    },
    {
      id: "invariant-checkpoint-order",
      description:
        "Checkpoints are created before any workspace mutation and cover every changed file.",
    },
    {
      id: "invariant-verify-not-write",
      description:
        "A successful write is not success: per-surface verification is required after mutation.",
    },
    {
      id: "invariant-no-raw-native",
      description:
        "Native changes never route through raw .tscn/.tres text editing; scene/resource paths stay refused at the raw change-set boundary.",
    },
    {
      id: "invariant-review-read-only",
      description:
        "Independent review is read-only with fresh, bounded context; the reviewer never mutates directly.",
    },
    {
      id: "invariant-repair-fresh",
      description:
        "Repairs prepare fresh mutations from current revisions with new approvals; stale prepared changes and approvals are never reused.",
    },
    {
      id: "invariant-acceptance-host",
      description:
        "Completion requires host-observed evidence; an executor claim alone cannot complete.",
    },
    {
      id: "invariant-no-runtime",
      description:
        "Inspection, mutation, and verification are static: no Godot project runtime is launched.",
    },
    {
      id: "invariant-no-second-engine",
      description: "No generic workflow engine or second state machine is introduced.",
    },
  ],
  nonGoals: [
    "Runtime Godot QA or in-engine validation.",
    "Real provider integrations, persistence, or multi-agent functionality.",
    "A generic transaction engine or distributed multi-file transactions.",
    "Raw text editing of .tscn/.tres files.",
    "Automatic rollback of successful changes when a later review fails.",
    "Permanent accumulation of all task source/context.",
  ],
  acceptance: [
    {
      id: "S3M11.ROUTING.SURFACE",
      description:
        "/develop routes script-only, native-only, and mixed tasks deterministically from host-observed evidence, never model claims.",
      evidenceKinds: ["workspace_read", "validation_result"],
    },
    {
      id: "S3M11.CHANGESET.MULTI_TARGET",
      description:
        "A bounded unified change set carries multiple targets, each retaining its own source revision, prepared fingerprint, approval state, and verification slot.",
      evidenceKinds: ["change_preview", "validation_result"],
    },
    {
      id: "S3M11.ORDER.DERIVED",
      description:
        "Apply order is explicit and evidenced: dependency-derived with a deterministic tie-break, never a hardcoded surface-first rule.",
      evidenceKinds: ["workspace_read", "parser_result"],
    },
    {
      id: "S3M11.APPROVAL.BOUND",
      description:
        "A combined preview may group related changes, but authorization binds the exact prepared change set; any material change invalidates the affected approval.",
      evidenceKinds: ["change_preview", "validation_result"],
    },
    {
      id: "S3M11.STALE.NO_PARTIAL",
      description:
        "An externally changed target blocks the whole apply: no target is mutated under a stale prepared approval, and the newer revision is preserved.",
      evidenceKinds: ["validation_result", "workspace_read"],
    },
    {
      id: "S3M11.CHECKPOINT.ALL",
      description:
        "A checkpoint is created before the mutation and covers every affected file of the batch.",
      evidenceKinds: ["checkpoint", "validation_result"],
    },
    {
      id: "S3M11.VERIFY.PER_SURFACE",
      description:
        "GDScript targets verify through check-only/parser and fresh LSP evidence; native targets through reparse and semantic-effect verification.",
      evidenceKinds: ["parser_result", "lsp_result", "validation_result"],
    },
    {
      id: "S3M11.CONSISTENCY.CROSS_SURFACE",
      description:
        "Script/scene/resource relationships are checked where statically supportable; runtime-only concerns are disclosed, never claimed resolved.",
      evidenceKinds: ["validation_result", "review_result"],
    },
    {
      id: "S3M11.IMPACT.VALIDATION",
      description:
        "Validation is targeted from impact evidence derived over the actual changed surfaces and relationships.",
      evidenceKinds: ["validation_result", "review_result"],
    },
    {
      id: "S3M11.REVIEW.READ_ONLY",
      description:
        "Independent review is read-only with bounded context: contract, acceptance criteria, actual diff/changeset, semantic evidence, manifest, and validation evidence only.",
      evidenceKinds: ["review_result"],
    },
    {
      id: "S3M11.REPAIR.FRESH",
      description:
        "Blocking review findings enter a bounded repair loop with fresh current revisions, fresh preparation, and new approval; stale repair artifacts are unusable.",
      evidenceKinds: ["review_result", "validation_result"],
    },
    {
      id: "S3M11.ACCEPTANCE.HOST_EVIDENCE",
      description:
        "Final completion requires host-observed acceptance evidence; an executor completion claim alone cannot complete the task.",
      evidenceKinds: ["review_result", "validation_result"],
      standardIds: ["STANDARD.FULL_VALIDATION"],
    },
    {
      id: "S3M11.BLOCKED.HONEST",
      description:
        "Unsupported requirements produce a structured blocked disposition with a concrete explanation; prior successful changes are preserved.",
      evidenceKinds: ["validation_result", "review_result"],
    },
    {
      id: "S3M11.UNDO.COMPATIBLE",
      description:
        "Mixed tasks remain recoverable through the existing checkpoint/undo semantics; successful changes are never auto-reverted on review failure.",
      evidenceKinds: ["checkpoint", "validation_result"],
    },
    {
      id: "S3M11.CONTEXT.PHASED",
      description:
        "Provider context and tool surfaces are phase-specific and bounded: planning, mutation, review, and repair receive only their required context.",
      evidenceKinds: ["workspace_read", "review_result"],
      standardIds: ["STANDARD.NO_TOOL_LEAKAGE"],
    },
    {
      id: "S3M11.TOOLS.NO_RAW_NATIVE",
      description:
        "No raw .tscn/.tres text-edit fallback exists; native mutation stays structural and prepare-only on the provider surface.",
      standardIds: ["STANDARD.NO_TOOL_LEAKAGE", "STANDARD.NO_WORKSPACE_MUTATION"],
    },
    {
      id: "S3M11.SECURITY.NO_RUNTIME",
      description: "Static inspection, mutation, and verification launch no Godot project runtime.",
      standardIds: ["STANDARD.NO_PROCESS_EXECUTION"],
    },
    {
      id: "S3M11.REGRESSION",
      description: "Stage 2 GDScript development and Stage 3.8/3.9/3.10 behavior remain green.",
      evidenceKinds: ["validation_result", "review_result"],
    },
  ],
  requiredTests: [
    {
      id: "test-unified-models",
      description:
        "Unit fixtures: surface routing, unified change set binding, derived ordering, cross-surface consistency, blocked dispositions.",
    },
    {
      id: "test-effect-mixed",
      description:
        "Effect test: end-to-end mixed task through plan -> inspect -> prepare -> approve -> checkpoint -> mutate -> verify -> impact -> validate -> review -> acceptance -> complete with no raw-text bypass.",
    },
    {
      id: "test-effect-stale-second-target",
      description:
        "Effect test: an externally changed target before apply blocks every target; no mutation under the stale approval.",
    },
    {
      id: "test-effect-repair",
      description:
        "Effect test: a deterministic blocking reviewer finding triggers a fresh repair cycle (fresh revisions/preparation/approval/apply/verify/re-review); the reviewer cannot mutate.",
    },
    {
      id: "test-effect-acceptance-integrity",
      description:
        "Effect test: an executor completion claim with a missing required validation never reaches TaskState complete.",
    },
    {
      id: "test-effect-context-isolation",
      description:
        "Effect test: planner, implementer, and reviewer provider requests carry only their phase context and tool surface.",
    },
    {
      id: "test-effect-no-runtime",
      description: "Effect test: a normal static mixed fixture launches no Godot project runtime.",
    },
    {
      id: "test-effect-undo",
      description:
        "Effect test: a completed mixed task remains recoverable through the existing undo/checkpoint semantics.",
    },
    {
      id: "test-regression",
      description:
        "Regression: Stage 2 development tasks and Stage 3.8/3.9/3.10 behavior remain green.",
    },
  ],
  architectureConcerns: [
    "godot",
    "godot-static-inspection",
    "workspace",
    "task-runtime",
    "projection",
    "planning",
    "security",
    "executor-briefing",
  ],
});
