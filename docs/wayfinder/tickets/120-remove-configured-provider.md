---
title: "Remove the Configured Provider"
label: "wayfinder:ticket"
status: "open"
date: "2026-09-10"
supersedes: []
---

# Remove the Configured Provider

There was no way to delete a provider. The fix: `remove_profile_config()`
beside `write_profile_config` on the same atomic-writer pattern (reads
preserving bytes, removes only the `[profile]` table, refuses the write
unless every other top-level item still serializes identically,
re-parses the temp as Absent, then renames; symlinks and non-regular
targets refused; temp deleted on failure); a missing file or absent
`[profile]` is a truthful no-op that leaves bytes untouched; reachable
from both frontends through one implementation (a `"- Remove provider"`
picker row shown only when a provider exists, and `/provider remove`
in the shared catalog), both gated by a y/N confirmation; the stdio
arm wires the existing input-queue approval helper for this command
only and the dormant `Approved.`/`Denied.` path is preserved unchanged.

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `9bcc507`) BEFORE this ticket and its entry review
> were written. There was no prior ticket and no prior entry review
> for this change; the approval that exists is an in-chat design
> approval from the human owner on 2026-09-10 — nothing more is claimed
> here, and nothing is backdated. The entry-review inversion is named
> in [the retroactive entry
> review](../decisions/145-remove-provider-entry-review.md): the record
> follows the implementation rather than authorizing it.

Authorized by
[the Remove the Configured Provider Entry Review (retroactive)](../decisions/145-remove-provider-entry-review.md).
Implemented and recorded in
[decision 146](../decisions/146-remove-configured-provider.md).

(End of file - total 41 lines)
