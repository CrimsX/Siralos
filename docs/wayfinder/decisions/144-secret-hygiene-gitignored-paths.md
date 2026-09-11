---
title: "Secret Hygiene Skips Git-Ignored Paths"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-10"
ticket: "119"
supersedes: []
---

# Secret Hygiene Skips Git-Ignored Paths

Ticket [119](../tickets/119-secret-hygiene-gitignored-paths.md) ·
entry review [143](143-secret-hygiene-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `c384764` "fix(scripts): secret hygiene skips
> git-ignored paths", 2026-09-10, with formatting follow-up `bbee359`
> "style(wayfinder): prettier-format the model-id charset records")
> BEFORE this record was written. There was no prior ticket and no
> prior entry review for this change; the approval that exists is an
> in-chat design approval from the human owner on 2026-09-10 — nothing
> more is claimed here, and nothing is backdated. The entry review
> ([143](143-secret-hygiene-entry-review.md)) is itself retroactive
> and names the inversion.

## 2. The Implemented

| Fix                       | The change                                                                                                                                                                                                                             | The evidence                                 |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| M1 IGNORED SUBSET VIA GIT | `collectIgnoredPaths()` resolves the git-ignored subset with one batched `git check-ignore -z --stdin` call — git owns ignore matching (`.gitignore:29` covers the workspace `siralos.toml`, so a normal `git add` cannot publish it). | the committed script diff                    |
| M2 COVERAGE PRESERVED     | Tracked and untracked-but-NOT-ignored files are still scanned; detection patterns, wording, and output format are unchanged.                                                                                                           | the committed diff; orchestrator probe below |
| M3 FAIL CLOSED            | Git unlaunchable or exiting anything but 0/1 yields an empty ignored set: everything is scanned.                                                                                                                                       | the fail-closed branch in the committed diff |
| M4 FORMAT REPAIR          | `bbee359` prettier-formats the 139/140 records, which had been committed before `format:check` ran over them.                                                                                                                          | the `bbee359` diff (wayfinder records only)  |

## 3. Criteria → Evidence

| Criterion                                         | Evidence                                                                                                              | Verdict  |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | -------- |
| Gitignored workspace key no longer fails the gate | orchestrator-verified: with the owner's `siralos.toml` in place, `check:secrets` passes                               | pass     |
| Publishable secrets still caught                  | orchestrator-verified: a temporary NON-ignored key-shaped probe file still fails the check and is named in the output | pass     |
| JS gates green                                    | orchestrator-verified: `npm run format:check` and `npm run lint` pass                                                 | pass     |
| Docs/context gates                                | `npm run check:docs` and `npm run check:context` outcomes reported below                                              | reported |

## 4. Result

The git-ignored skip is complete as committed in `c384764` (plus
`bbee359`): the publication guardrail no longer fails on files git
itself will not publish, coverage over publishable files is unchanged,
and git's absence widens rather than narrows the scan. This closes the
open item recorded in decisions 139/140. Retroactive record closed;
ticket 119 done. All evidence above was gathered by the orchestrator
and is cited here, not re-run.
