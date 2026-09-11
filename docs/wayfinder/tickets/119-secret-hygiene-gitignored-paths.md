---
title: "Secret Hygiene Skips Git-Ignored Paths"
label: "wayfinder:ticket"
status: closed
date: "2026-09-10"
supersedes: []
---

# Secret Hygiene Skips Git-Ignored Paths

`npm run check` failed at `check:secrets` for any user who stores a
verbatim key in the workspace `siralos.toml` — the very thing
decisions 137/138 introduced. That file is deliberately gitignored
(`.gitignore:29`), so a normal `git add` cannot publish it and the
publication guardrail should not fail on it. The fix:
`collectIgnoredPaths()` resolves the ignored subset with one batched
`git check-ignore -z --stdin` call (git owns ignore matching);
tracked and untracked-but-NOT-ignored files are still scanned;
detection patterns, wording, and output format are unchanged; the
check fails closed (empty ignored set = scan everything) when git
cannot be launched or exits anything but 0/1.

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `c384764`, with formatting follow-up `bbee359`)
> BEFORE this ticket and its entry review were written. There was no
> prior ticket and no prior entry review for this change; the approval
> that exists is an in-chat design approval from the human owner on
> 2026-09-10 — nothing more is claimed here, and nothing is backdated.
> The entry-review inversion is named in [the retroactive entry
> review](../decisions/143-secret-hygiene-entry-review.md): the record
> follows the implementation rather than authorizing it.

Authorized by
[the Secret Hygiene Skips Git-Ignored Paths Entry Review (retroactive)](../decisions/143-secret-hygiene-entry-review.md).
Implemented and recorded in
[decision 144](../decisions/144-secret-hygiene-gitignored-paths.md).

This closes the open item recorded in decisions 139/140 (the
`check:secrets` failure on the untracked, gitignored workspace
`siralos.toml` storing a literal key). The `bbee359` follow-up is the
formatting repair for the 139/140 records, which had been committed
before `format:check` ran over them.
