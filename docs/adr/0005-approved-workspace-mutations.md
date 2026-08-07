# ADR 0005: Approved workspace mutations

Status: accepted

## Context

Solaris can inspect the workspace but not change it. The sandbox and permission foundation (ADR 0004) exists, but no provider-accessible mutation does. This milestone adds the first three: create one UTF-8 text file, apply exact text replacements to one existing UTF-8 text file, and delete one existing UTF-8 text file — each gated by capability policy, a complete preview, and one-time user approval.

## Decision

- **Approval is separate from sandbox enforcement.** A permission decision gates whether an operation proceeds; the sandbox profile defines the restrictions under which it runs. Approving a change means "apply this exact prepared mutation once" — never "run unrestricted", never a session-wide grant, never a sandbox expansion.
- **Approval is one-time only.** Core owns a UI-independent `ApprovalReviewer` port; the CLI implements the interactive reviewer; decisions are never reusable; a revised proposal requires a new approval.
- **Write tools are hidden under `inspect`.** The application filters the provider tool list through the capability policy, so `workspace.write` tools are absent when the policy denies writes.
- **Exact SHA-256 hash preconditions are mandatory.** `workspace.read` returns the complete-file hash; edit and delete require the exact expected hash; a mismatch is a `conflict`, never a silent apply. Timestamps and sizes are not conflict preconditions.
- **Exact text replacement is the initial edit model.** Sequential exact `oldText`/`newText` replacements, each matching exactly once; no regex, no replace-all, no line-number editing, no provider-supplied patch parsing.
- **Raw overwrite and generic patch parsing are deferred.** Every mutation goes through prepare → review → revalidate → apply, and prepared mutations are opaque, single-use objects that expire.
- **Diffs must be complete before approval; truncated diffs cannot be approved.** If the complete unified diff exceeds the configured preview limit, preparation fails with an actionable message instead of hiding hunks.
- **Protected paths cannot be overridden by approval.** `.git/`, `.solaris/`, `.env`, `.env.*`, `*.pem`, `*.key`, the user-level Solaris configuration, and anything outside the workspace stay denied regardless of the reviewer's decision.
- **Only UTF-8 text files are supported.** Binary files, directories, and oversized files are rejected; directory and binary mutations are deferred.
- **Mutations are serialized** by a small in-process lock; preconditions are revalidated after acquiring it and immediately before the final filesystem replacement. Cancellation before the commit section prevents mutation; cancellation during the commit section completes verification before reporting.
- **The final replacement is a cautious single-file strategy**: exclusive creation; temp-file staging with flush and exclusive names for updates; revalidation; rename (with a documented Windows unlink+rename fallback); post-write byte and hash verification; temporary-file cleanup.
- **Git checkpoints and undo are the next milestone**; they are not implemented here.

## Consequences

Positive:

- The first provider-accessible mutations execute under the full security stack from day one: policy, approval, path safety, conflict detection, serialization, and verification.
- Providers can never approve, retry-denied, or bypass a decision; every retry produces a new proposal and a new approval.
- The fake provider can demonstrate the full create → read → edit → delete workflow deterministically.

Negative:

- Every mutation requires interactive approval by default; this is deliberate until a reviewed policy for broader grants exists.
- The Windows replacement fallback (unlink + rename) has a brief window where the file is absent; documented as a platform limitation.
- Exact-text editing is restrictive by design; regex and patch-based editing are deferred.

## Alternatives rejected

- Direct overwrite without preconditions: defeats conflict safety.
- Provider-supplied unified diffs: untrusted patch parsing is a large, risky surface.
- Session-wide or "approve all" grants: not justified before real provider usage exists.
- Project-configurable write permissions: an untrusted repository must not broaden its own permissions.
- Multiple-file atomic transactions: the current requirement is single-file mutations.
