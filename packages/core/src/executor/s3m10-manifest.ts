import type { MilestoneManifest } from "./milestone-manifest.js";
import { createMilestoneManifest } from "./milestone-manifest.js";

/**
 * Stage 3 — Milestone 10 manifest: Approved Scene and Resource Mutation.
 *
 * Represents the milestone in compact structured form: goal,
 * deliverables, invariants, non-goals, acceptance IDs, required tests,
 * and architecture concerns. It does not restate Git policy, standard
 * validation, generic security rules, or architecture principles — the
 * Execution Contract supplies those.
 */

export const S3M10_MILESTONE_MANIFEST: MilestoneManifest = createMilestoneManifest({
  id: "S3M10",
  title: "Approved Scene and Resource Mutation",
  goal: "Add safe, structured, approval-gated mutation of Godot .tscn/.tres files: typed operations validated against the exact source revision, complete previews, revision-bound approval, checkpoints, deterministic structural serialization, post-apply reparse and semantic verification, and impact-aware validation — without treating scene/resource files as arbitrary text-edit targets.",
  prerequisites: [
    {
      id: "prereq-scene-resource",
      description: "Stage 3 milestone 8 revision-aware scene/resource parsing and semantic models.",
    },
    {
      id: "prereq-impact",
      description: "Stage 3 milestone 9 impact analysis consuming resulting native changes.",
    },
    {
      id: "prereq-approval",
      description:
        "Existing approval, checkpoint, revision, and change-set apply infrastructure (ADR 0012).",
    },
  ],
  deliverables: [
    {
      id: "deliver-mutation-model",
      description:
        "Typed scene/resource mutation operations with validation and bounded semantic expectations.",
    },
    {
      id: "deliver-prepared",
      description:
        "Immutable prepared mutations: exact revision, operation set, complete preview, fingerprint.",
    },
    {
      id: "deliver-serializer",
      description:
        "Deterministic structural .tscn/.tres serializer preserving stable identities and untouched formatting.",
    },
    {
      id: "deliver-verify",
      description:
        "Post-apply semantic verification: reparsed state must match the intended effect; failure is explicit.",
    },
    {
      id: "deliver-service",
      description:
        "Application-owned mutation service: prepare and approval-bound apply with checkpoint/revalidation/reparse/verify.",
    },
    {
      id: "deliver-tools",
      description:
        "Prepare-only provider tools (godot.prepare_scene_change / godot.prepare_resource_change) with no apply bypass.",
    },
  ],
  invariants: [
    {
      id: "invariant-revision-bound",
      description:
        "Every prepared mutation binds the exact source revision; stale source rejects before apply.",
    },
    {
      id: "invariant-preview-complete",
      description: "Approval never relies on a truncated preview; an unapprovable preview refuses.",
    },
    {
      id: "invariant-checkpoint-order",
      description: "Checkpoints are created before any workspace mutation.",
    },
    {
      id: "invariant-no-raw-bypass",
      description:
        "Native mutations never route through generic raw text editing; scene/resource paths remain refused at the raw change-set boundary.",
    },
    {
      id: "invariant-verify-not-write",
      description:
        "A successful write is not success: semantic verification of the reparsed revision is required.",
    },
    {
      id: "invariant-no-process",
      description: "Mutation and verification are static: no Godot/project process is launched.",
    },
    {
      id: "invariant-derived",
      description:
        "Mutation grants no capability; approval and checkpoint systems remain the authorization/recovery owners.",
    },
  ],
  nonGoals: [
    "Runtime Godot QA or in-engine validation.",
    "Global UID allocation without an authoritative mechanism.",
    "A generic transaction engine or distributed multi-file transactions.",
    "Rewriting entire documents when a safe narrower serialization exists.",
    "Scene/resource mutation without approval, checkpoint, revalidation, or verification.",
  ],
  acceptance: [
    {
      id: "S3M10.PREPARE.PREVIEW",
      description:
        "A prepared mutation carries a complete preview (structural summary + exact serialized diff) before approval.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M10.APPROVAL.BINDING",
      description:
        "Approval binds the exact prepared mutation fingerprint; a changed operation/revision/target invalidates it.",
      evidenceKinds: ["parser_result", "validation_result"],
    },
    {
      id: "S3M10.REVISION.STALE",
      description:
        "Stale source revisions reject before apply; the newer revision is preserved and the old approval is not reused.",
      evidenceKinds: ["validation_result", "review_result"],
    },
    {
      id: "S3M10.CHECKPOINT.ORDER",
      description: "A checkpoint is created before the mutation applies.",
      evidenceKinds: ["parser_result", "validation_result"],
    },
    {
      id: "S3M10.APPLY.DETERMINISTIC",
      description:
        "Structural apply is deterministic and the serialized output re-parses as valid Godot text syntax.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M10.IDENTITY.PRESERVED",
      description:
        "Stable Godot identities (ext/sub ids, UIDs, node paths, parent/owner) remain valid and unrenumbered.",
      evidenceKinds: ["parser_result", "workspace_read"],
    },
    {
      id: "S3M10.VERIFY.SEMANTIC",
      description:
        "Success requires reparsed semantic state matching the intended effect, not just a successful write.",
      evidenceKinds: ["parser_result", "validation_result"],
    },
    {
      id: "S3M10.VERIFY.FAILURE",
      description:
        "A failed semantic verification is reported failed/uncertain with recovery evidence, never success.",
      evidenceKinds: ["validation_result", "review_result"],
    },
    {
      id: "S3M10.TOOLS.PREPARE_ONLY",
      description:
        "Provider-visible tools are prepare-only; no raw native write tool and no apply bypass exist.",
      standardIds: ["STANDARD.NO_TOOL_LEAKAGE"],
    },
    {
      id: "S3M10.DEVELOP.NATIVE",
      description:
        "The approved native mutation flow (prepare -> approve -> checkpoint -> apply -> verify) is functional end to end.",
      evidenceKinds: ["review_result", "validation_result"],
    },
    {
      id: "S3M10.CONTEXT.BOUNDED",
      description:
        "Native mutation context remains bounded to the target and directly required relationships.",
      evidenceKinds: ["validation_result", "review_result"],
    },
    {
      id: "S3M10.SECURITY.NO_PROCESS",
      description: "Mutation and verification launch no Godot/project process.",
      standardIds: ["STANDARD.NO_PROCESS_EXECUTION"],
    },
    {
      id: "S3M10.SECURITY.NO_MUTATION_BYPASS",
      description:
        "Native mutation cannot bypass approval/revision/checkpoint flow; generic raw text mutation of scene/resource paths stays refused.",
      standardIds: ["STANDARD.NO_WORKSPACE_MUTATION"],
    },
  ],
  requiredTests: [
    {
      id: "test-mutation-fixtures",
      description:
        "Unit fixtures: property/node/script/connection/subresource operations, serialization determinism, identity preservation, verification.",
    },
    {
      id: "test-effect-stale",
      description:
        "Effect test: prepare at rev_A, approve, external change to rev_B, apply rejected with rev_B preserved.",
    },
    {
      id: "test-effect-approval",
      description: "Effect test: a modified prepared operation requires a new approval.",
    },
    {
      id: "test-effect-checkpoint-order",
      description: "Effect test: checkpoint creation precedes the workspace mutation.",
    },
    {
      id: "test-effect-semantic-verify",
      description: "Effect test: success is based on reparsed semantic state, not the write alone.",
    },
    {
      id: "test-effect-verify-failure",
      description:
        "Effect test: a deterministic fixture where serialized output cannot satisfy the expectation reports failed/uncertain.",
    },
    {
      id: "test-effect-no-bypass",
      description:
        "Effect test: the provider-visible schema has no raw native write tool and the raw change-set boundary refuses scene/resource paths.",
    },
    {
      id: "test-effect-context",
      description:
        "Effect test: mutating one scene in a large project keeps provider-visible context bounded.",
    },
    {
      id: "test-regression",
      description:
        "Regression: Stage 3.8 inspection, Stage 3.9 impact, and GDScript /develop flows remain green.",
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
  ],
});
