---
id: ADR-0026
status: accepted
domains: [godot, mutation, security]
paths:
  [
    packages/core/src/godot/scene-mutation/**,
    packages/adapters/src/godot/scene-mutation/**,
    packages/adapters/src/godot/tools/godot-prepare-change-tools.ts,
  ]
supersedes: []
---

# ADR 0026 — Approved Scene and Resource Mutation

- Status: accepted (Stage 3 milestone 10)
- Date: current milestone
- Related: ADR 0012 (GDScript development workflow and change-set apply),
  ADR 0021 (read-only scene/resource intelligence), ADR 0025 (impact
  intelligence), ADR 0005 (approved workspace mutations)

## Problem

Stage 3.8 gave Siralos revision-aware understanding of `.tscn`/`.tres`
structure and Stage 3.9 gave it impact intelligence, but every mutation
request was refused: scene/resource files are not arbitrary text-edit
targets, and a raw text write would bypass exact-revision binding,
previews, approval, checkpoints, and semantic verification.

## Decision

Add structured, approval-gated native mutation as its own
application-owned surface:

- **Typed mutation operations** (`scene-mutation/operations.ts`): scene
  (set/remove property, add/remove node, script attachment, resource
  reference change, add/remove signal connection, subresources) and
  resource (property, reference, subresource) operations, validated
  against the parsed document (unknown node paths, missing ext ids, and
  duplicate sub ids reject) and bounded (≤32 operations, structured
  values only — opaque constructors are refused).
- **Prepared mutations** (`prepared.ts`): an immutable artifact binding
  the exact target revision, the exact source SHA-256, the operation
  set, the complete preview (structural summary + exact unified diff),
  the expected semantic effect, the deterministic serialized output, and
  a fingerprint over all of it. Approval binds the fingerprint; any
  material change produces a new identity, so old approvals can never
  satisfy it. Plan approval and native-mutation approval stay separate.
- **Deterministic structural serialization** (`serializer.ts`):
  full-document serialization with stable section ordering, preserved
  ext/sub ids, UIDs, node paths, parent/owner, and untouched property
  raw text (no formatting churn); new/changed values serialize from the
  structured variant model. A prepared mutation whose serialized output
  does not reparse is never approvable (prepare-time self-check). If a
  complete preview cannot be produced (diff too large), approval/apply
  is refused.
- **Semantic verification** (`verify.ts`): after apply, the target is
  reparsed at its new revision and the derived model is checked against
  the expected semantic effect. A successful write is NOT success: a
  failed or uncertain verification is reported as such with recovery
  evidence (the checkpoint is retained), never as success.
- **Application-owned apply orchestration** (`scene-mutation-service.ts`):
  approval binding → revision revalidation (expected SHA-256 vs current;
  mismatch is a hard stale rejection, rev_B preserved, no rebase under
  old approval) → checkpoint-before-apply → structural apply through the
  existing change-set protocol (exact preimage verification, sequential
  hash-verified apply, recovery on partial failure) → reparse →
  semantic verification. The production gate stays fail-closed
  (`canApplyIdentityBound: false`); tests inject in-memory primitives.
- **Prepare-only provider tools** (`godot.prepare_scene_change` /
  `godot.prepare_resource_change`): providers can only PREPARE; apply is
  host-orchestrated through approval. The raw change-set boundary
  continues to refuse scene/resource paths, so there is no raw-text
  backdoor.

## Rejected

- **Generic text mutation for `.tscn`/`.tres`** — the raw change-set
  boundary refuses scene/resource paths; native mutation is structured
  only.
- **Approval without a full preview** — a truncated diff or a
  non-reparsing output is unapprovable by construction.
- **Stale automatic rebase/reapply** — a revision mismatch is a hard
  rejection; the executor must inspect/reprepare/reapprove.
- **Success based only on write completion** — semantic verification of
  the reparsed revision is mandatory; write ≠ verified success.
- **Eager whole-project reserialization** — only the target document is
  serialized, preserving untouched raw formatting.
- **Runtime Godot loading as ordinary mutation verification** — no Godot
  process; runtime validation remains deferred evidence.
- **Global UID allocation** — Siralos preserves serialized UIDs and never
  invents global identities without an authoritative mechanism.
- **A generic transaction engine** — multi-file apply stays bounded and
  serialized with explicit partial/uncertain recovery evidence.

## Consequences

- Scene/resource mutation is safe by construction: every apply is
  revision-bound, previewed, approved by exact fingerprint, checkpointed,
  structurally applied, reparsed, and semantically verified.
- Impact intelligence (Stage 3.9) consumes the resulting change for
  validation and independent review; context stays bounded to the target
  and directly required relationships.
- The production surface remains fail-closed until an identity-bound
  commit primitive exists — exactly like the GDScript change-set
  machinery — while the full protocol is tested through in-memory
  primitives.
