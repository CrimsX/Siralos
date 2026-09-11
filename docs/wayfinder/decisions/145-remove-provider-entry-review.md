---
title: "Remove the Configured Provider Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-10"
ticket: "120"
supersedes: []
---

# Remove the Configured Provider Entry Review

Ticket [120](../tickets/120-remove-configured-provider.md) · entry
review [the Siralos TUI entry review](103-siralos-tui-entry-review.md)
· [Map](../siralos-roadmap.md)

> **Retroactive record — the inversion, stated plainly.** This entry
> review was written AFTER the change was implemented, verified, and
> committed (commit `8855be8` "feat(provider): remove the configured
> provider", 2026-09-10). There was no prior ticket and no prior entry
> review for this change; the approval that exists is an in-chat
> design approval from the human owner on 2026-09-10 — nothing more is
> claimed here, and nothing is backdated. The normal order (entry
> review authorizes → implementation lands) is inverted here: the
> implementation landed first and this review records what it should
> have authorized. The verdict below is therefore a retroactive PASS
> over the already-committed diff, not a pre-commit authorization.

> **Owner-approved 2026-09-10 (retroactive, in-chat design approval).**
> There was no way to delete a provider. The fix:
> `remove_profile_config()` beside `write_profile_config` on the same
> atomic-writer pattern (reads preserving bytes, removes only the
> `[profile]` table, refuses the write unless every other top-level
> item still serializes identically, re-parses the temp as Absent,
> then renames; symlinks and non-regular targets refused; temp deleted
> on failure); a missing file or absent `[profile]` is a truthful
> no-op that leaves bytes untouched; reachable from both frontends
> through one implementation (a `"- Remove provider"` picker row shown
> only when a provider exists, and `/provider remove` in the shared
> catalog), both gated by a y/N confirmation; the stdio arm wires the
> existing input-queue approval helper for this command only and the
> dormant `Approved.`/`Denied.` path is preserved unchanged.

## 2. The Fixes

| Fix                                              | The change                                                                                                                                                                                                                                                                                                                                | The evidence                            |
| ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------- |
| F1 ATOMIC REMOVE                                 | New `remove_profile_config()` mirrors `write_profile_config`: byte-preserving read, removes only the `[profile]` table, refuses the write unless every other top-level item still serializes identically, re-parses the temp file as Absent, then renames; symlinks and non-regular targets refused; the temp file is deleted on failure. | the committed writer diff; 13 new tests |
| F2 TRUTHFUL NO-OP                                | A missing file or an absent `[profile]` is a no-op that leaves bytes untouched and says so — removal never fabricates or rewrites what is not there.                                                                                                                                                                                      | no-op tests in the committed diff       |
| F3 ONE IMPLEMENTATION, BOTH FRONTENDS            | A `"- Remove provider"` picker row (shown only when a provider exists) and `/provider remove` in the shared catalog both route through the one removal implementation; both are gated by a y/N confirmation.                                                                                                                              | the committed frontend diff             |
| F4 STDIO APPROVAL WIRING, DORMANT PATH PRESERVED | The stdio arm wires the existing input-queue approval helper for this command only; the dormant `Approved.`/`Denied.` path is preserved unchanged.                                                                                                                                                                                        | the committed stdio diff                |

## 3. Criteria → Evidence

| Criterion                                                   | Evidence                                                                                                                                                                                                                                                                                                             | Verdict |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Removal is atomic and byte-preserving outside `[profile]`   | same-pattern writer tests: other top-level items serialize identically or the write is refused; temp re-parsed as Absent before rename                                                                                                                                                                               | pass    |
| Absent provider is a truthful no-op                         | missing-file and absent-`[profile]` tests leave bytes untouched                                                                                                                                                                                                                                                      | pass    |
| Both frontends reach one implementation behind confirmation | picker-row + `/provider remove` tests; y/N confirmation on both arms                                                                                                                                                                                                                                                 | pass    |
| Dormant approval path untouched                             | `Approved.`/`Denied.` path preserved unchanged in the committed diff                                                                                                                                                                                                                                                 | pass    |
| No regression                                               | `cargo test --workspace --all-targets --all-features --locked` exit 0 (614 core / 306 adapters / 25 conformance / 227 cli, 0 failed; 13 new); `npm run check:rust` green; `npm run check:differential` exit 0, parity 352/352 applicable required (evidence gathered by the orchestrator and cited here, not re-run) | pass    |

## 4. Result

Entry review PASS (retroactive): provider removal as committed in
`8855be8` is the correct fix — an atomic writer beside the existing
one, a truthful no-op when there is nothing to remove, one
implementation behind y/N confirmation in both frontends, and the
dormant approval path preserved. This review authorized nothing (the
code had already landed); it records what authorization would have
covered.

(End of file - total 48 lines)
