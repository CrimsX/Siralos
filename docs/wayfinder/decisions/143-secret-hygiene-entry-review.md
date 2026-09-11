---
title: "Secret Hygiene Skips Git-Ignored Paths Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-10"
ticket: "119"
supersedes: []
---

# Secret Hygiene Skips Git-Ignored Paths Entry Review

Ticket [119](../tickets/119-secret-hygiene-gitignored-paths.md) ·
entry review [the Siralos TUI entry
review](103-siralos-tui-entry-review.md) · [Map](../siralos-roadmap.md)

> **Retroactive record — the inversion, stated plainly.** This entry
> review was written AFTER the change was implemented, verified, and
> committed (commit `c384764` "fix(scripts): secret hygiene skips
> git-ignored paths", 2026-09-10, with formatting follow-up `bbee359`).
> There was no prior ticket and no prior entry review for this change;
> the approval that exists is an in-chat design approval from the
> human owner on 2026-09-10 — nothing more is claimed here, and nothing
> is backdated. The normal order (entry review authorizes →
> implementation lands) is inverted here: the implementation landed
> first and this review records what it should have authorized. The
> verdict below is therefore a retroactive PASS over the
> already-committed diff, not a pre-commit authorization.

> **Owner-approved 2026-09-10 (retroactive, in-chat design approval).**
> `npm run check` failed at `check:secrets` for any user who stores a
> verbatim key in the workspace `siralos.toml` — the very thing
> decisions 137/138 introduced. That file is deliberately gitignored
> (`.gitignore:29`), so a normal `git add` cannot publish it and the
> publication guardrail should not fail on it. The fix:
> `collectIgnoredPaths()` resolves the ignored subset with one batched
> `git check-ignore -z --stdin` call (git owns ignore matching);
> tracked and untracked-but-NOT-ignored files are still scanned;
> detection patterns, wording, and output format are unchanged; the
> check fails closed (empty ignored set = scan everything) when git
> cannot be launched or exits anything but 0/1.

## 2. The Fixes

| Fix                                  | The change                                                                                                                                                                                                          | The evidence                                                                                                          |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| F1 IGNORED PATHS RESOLVED BY GIT     | New `collectIgnoredPaths()`: one batched `git check-ignore -z --stdin` call over the candidate paths; git owns ignore matching, so nested `.gitignore` rules and global excludes behave exactly as `git add` would. | the committed script diff                                                                                             |
| F2 STILL SCANS WHAT CAN BE PUBLISHED | Tracked files and untracked-but-NOT-ignored files are still scanned with unchanged detection patterns, wording, and output format — the guardrail loses no coverage over publishable content.                       | orchestrator-verified: a temporary NON-ignored key-shaped probe file still fails the check and is named in the output |
| F3 FAIL CLOSED                       | When git cannot be launched or exits anything but 0/1, the ignored set is empty and everything is scanned — an unavailable git never silently narrows the scan.                                                     | the fail-closed branch in the committed diff                                                                          |
| F4 FORMAT FOLLOW-UP                  | `bbee359` repairs the 139/140 records' formatting, which had been committed before `format:check` ran over them.                                                                                                    | the `bbee359` diff (wayfinder records only)                                                                           |

## 3. Criteria → Evidence

| Criterion                                                  | Evidence                                                                                                              | Verdict |
| ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ------- |
| Owner's gitignored `siralos.toml` no longer fails the gate | orchestrator-verified: with the owner's `siralos.toml` in place, `check:secrets` passes                               | pass    |
| Non-ignored secrets still caught and named                 | orchestrator-verified: a temporary NON-ignored key-shaped probe file still fails the check and is named in the output | pass    |
| No pattern/wording/format drift                            | detection patterns, wording, and output format unchanged in the committed diff                                        | pass    |
| Fail closed without git                                    | empty ignored set (scan everything) when git cannot launch or exits outside 0/1                                       | pass    |
| JS gates green                                             | orchestrator-verified: `npm run format:check` and `npm run lint` pass                                                 | pass    |

## 4. Result

Entry review PASS (retroactive): the git-ignored skip as committed in
`c384764` (plus the `bbee359` formatting repair) is the correct fix —
git owns ignore matching, publishable files keep full coverage, the
check fails closed without git, and the open item from decisions
139/140 is closed. This review authorized nothing (the code had already
landed); it records what authorization would have covered. All
evidence above was gathered by the orchestrator and is cited here,
not re-run.

(End of file - total 48 lines)
