# ADR 0005: Approved workspace mutations

Status: accepted

> **Status note (fail-closed).** The mutation design in this ADR is **not offered at this stage**: every entry point of `workspace.create_file`, `workspace.edit_file`, `workspace.delete_file`, and `/undo` fails closed as `unavailable` before any write, approval, or checkpoint, because Node offers no directory-relative (openat/renameat) primitive and a same-user process can swap a parent or target at any instruction boundary. The machinery exists as tested internal code the product cannot reach; no approval for mutations is ever requested, and checkpoints may still be listed (empty). This ADR documents the design for a future commit primitive that can mechanically bind the operation to the exact object.

## Context

Solaris can inspect the workspace but not change it. The sandbox and permission foundation (ADR 0004) exists, but no provider-accessible mutation does. This milestone adds the first three: create one UTF-8 text file, apply exact text replacements to one existing UTF-8 text file, and delete one existing UTF-8 text file — each gated by capability policy, a complete preview, and one-time user approval.

## Decision

- **Approval is separate from sandbox enforcement.** A permission decision gates whether an operation proceeds; the sandbox profile defines the restrictions under which it runs. Approving a change means "apply this exact prepared mutation once" — never "run unrestricted", never a session-wide grant, never a sandbox expansion.
- **Approval is one-time only, bound to the exact plan.** Core owns a UI-independent `ApprovalReviewer` port; the CLI implements the interactive reviewer; decisions are never reusable; a revised proposal requires a new approval. Every workspace-write approval request carries a SHA-256 digest over the immutable plan (path, operation, before/after content hashes), and `apply` verifies the plan it is asked to execute against the approved digest. An ordinary tool whose capability requires approval but cannot produce a reviewable immutable plan is denied without execution (fail closed).
- **Write tools are hidden under `inspect`.** The application filters the provider tool list through the capability policy, so `workspace.write` tools are absent when the policy denies writes.
- **Exact SHA-256 hash preconditions are mandatory.** `workspace.read` returns the complete-file hash; edit and delete require the exact expected hash; a mismatch is a `conflict`, never a silent apply. Timestamps and sizes are not conflict preconditions.
- **Exact text replacement is the initial edit model.** Sequential exact `oldText`/`newText` replacements, each matching exactly once; no regex, no replace-all, no line-number editing, no provider-supplied patch parsing.
- **Raw overwrite and generic patch parsing are deferred.** Every mutation goes through prepare → review → revalidate → apply, and prepared mutations are opaque, single-use objects that expire.
- **Diffs must be complete before approval; truncated diffs cannot be approved.** If the complete unified diff exceeds the configured preview limit, preparation fails with an actionable message instead of hiding hunks.
- **Protected paths cannot be overridden by approval.** `.git/`, `.solaris/`, `.env`, `.env.*`, `*.pem`, `*.key`, the user-level Solaris configuration, and anything outside the workspace stay denied regardless of the reviewer's decision.
- **Only UTF-8 text files are supported.** Binary files, directories, and oversized files are rejected; directory and binary mutations are deferred.
- **Mutations are serialized** by a small in-process lock whose queued waiters respond promptly to cancellation; preconditions (canonical containment and the exact SHA-256) are revalidated after acquiring the lock and immediately before the irreversible commit point (exclusive create, replacement commit, unlink). Cancellation before the commit section prevents mutation; once the commit point is reached, cancellation cannot interrupt verification and checkpoint finalization.
- **The final replacement is a cautious single-file strategy, identity-bound on every platform**: exclusive creation; temp-file staging with flush and exclusive names for updates; revalidation; a shared safe-replacement primitive that always first moves the target to a same-directory quarantine (an atomic displacement of exactly the object at the target), verifies the displaced object against the expected hash, then commits the staged content or performs the deletion with an exclusive absence-preserving primitive — the commit and every rollback use a hard link that atomically requires the destination to remain absent (failing on `EEXIST`), so a target that appears after the quarantine displacement is never overwritten on any platform, and a successful direct rename is never treated as a compare-and-swap. Rollback conflicts (a newer object occupying the target) return an explicit uncertain-state result that preserves both the quarantine and the later target — never an overwrite, never success — and the only valid copy is never unlinked before replacement is committed. Deletion uses the same displacement-verify-unlink discipline. Exclusive creation verifies every parent component's identity immediately before the open and proves the created object's identity (handle versus path) before any bytes are written, closing the parent-swap window; cleanup removes only the identity-proven object. Post-write byte and hash verification follows, and temporary-file and quarantine cleanup happens only after the new content is verified and the checkpoint lifecycle is durably finalized. Where the filesystem cannot provide the absence-preserving primitive (hard links unsupported), the operation fails closed.
- **Protected-path matching is case-insensitive on Windows and macOS** (treated conservatively as case-insensitive) and applied during preparation and immediately before commit, so `.GIT`, `.Git`, `.ENV` and equivalents cannot address protected paths; Windows junctions and reparse points are rejected through canonical containment.
- **Git checkpoints and undo are the next milestone**; they are not implemented here.

## Consequences

Positive:

- The first provider-accessible mutations would execute under the full security stack from day one: policy, approval, path safety, conflict detection, serialization, and verification. At this stage they fail closed as `unavailable` instead (see the status note above).
- Providers can never approve, retry-denied, or bypass a decision; every retry produces a new proposal and a new approval.
- The fake provider can demonstrate the full create → read → edit → delete workflow deterministically, exercising the fail-closed path until the commit primitive exists.

Negative:

- Every mutation requires interactive approval by default; this is deliberate until a reviewed policy for broader grants exists.
- The quarantine dance keeps the original in a same-directory `.solaris-quarantine-*` file until the displaced object is verified and the new content is verified and the checkpoint lifecycle is durably finalized; the path is reported in every failure result and never cleaned before verification or finalization.
- Exact-text editing is restrictive by design; regex and patch-based editing are deferred.

## Alternatives rejected

- Direct overwrite without preconditions: defeats conflict safety.
- Provider-supplied unified diffs: untrusted patch parsing is a large, risky surface.
- Session-wide or "approve all" grants: not justified before real provider usage exists.
- Project-configurable write permissions: an untrusted repository must not broaden its own permissions.
- Multiple-file atomic transactions: the current requirement is single-file mutations.
