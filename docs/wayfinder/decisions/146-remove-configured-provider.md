---
title: "Remove the Configured Provider"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-10"
ticket: "120"
supersedes: []
---

# Remove the Configured Provider

Ticket [120](../tickets/120-remove-configured-provider.md) · entry
review [145](145-remove-provider-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `9bcc507` "feat(provider): remove the configured
> provider", 2026-09-10) BEFORE this record was written. There was no
> prior ticket and no prior entry review for this change; the approval
> that exists is an in-chat design approval from the human owner on
> 2026-09-10 — nothing more is claimed here, and nothing is backdated.
> The entry review ([145](145-remove-provider-entry-review.md)) is
> itself retroactive and names the inversion.

## 2. The Implemented

| Fix                                   | The change                                                                                                                                                                                                                                                                                                                                            | The evidence                        |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| M1 ATOMIC REMOVE                      | `remove_profile_config()` beside `write_profile_config` on the same atomic-writer pattern: byte-preserving read, removes only the `[profile]` table, refuses the write unless every other top-level item still serializes identically, re-parses the temp as Absent, then renames; symlinks and non-regular targets refused; temp deleted on failure. | the committed writer diff           |
| M2 TRUTHFUL NO-OP                     | Missing file or absent `[profile]` leaves bytes untouched and says so.                                                                                                                                                                                                                                                                                | no-op tests in the committed diff   |
| M3 BOTH FRONTENDS, ONE IMPLEMENTATION | `"- Remove provider"` picker row (shown only when a provider exists) and `/provider remove` in the shared catalog route through the one implementation, both behind y/N confirmation.                                                                                                                                                                 | the committed frontend diff         |
| M4 STDIO WIRING                       | The stdio arm wires the existing input-queue approval helper for this command only; the dormant `Approved.`/`Denied.` path is preserved unchanged.                                                                                                                                                                                                    | the committed stdio diff            |
| M5 TESTS                              | 13 new tests pin the writer guarantees, the no-op, both frontend arms, and the confirmation gate.                                                                                                                                                                                                                                                     | the new tests in the committed diff |

## 3. Criteria → Evidence

| Criterion                              | Evidence                                                                                                                                                                                                                                                                                                                       | Verdict  |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------- |
| Provider deletable from both frontends | picker-row and `/provider remove` tests pass                                                                                                                                                                                                                                                                                   | pass     |
| Atomicity and byte-preservation hold   | writer-guarantee tests pass (identical reserialization required; Absent re-parse before rename; symlink/non-regular refusal; temp cleanup)                                                                                                                                                                                     | pass     |
| Absent provider is a truthful no-op    | no-op tests pass with bytes untouched                                                                                                                                                                                                                                                                                          | pass     |
| No regression                          | `cargo test --workspace --all-targets --all-features --locked` exit 0 (614 core / 306 adapters / 25 conformance / 227 cli, 0 failed; 13 new); `npm run check:rust` green; `npm run check:differential` exit 0, parity held 352/352 applicable required (evidence gathered by the orchestrator and cited here, not re-invented) | pass     |
| Docs/context gates                     | `npm run check:docs` and `npm run check:context` outcomes reported below                                                                                                                                                                                                                                                       | reported |

## 4. Result

Provider removal is complete as committed in `9bcc507`: a provider can
now be deleted from either frontend through one confirmed
implementation on the established atomic-writer pattern, and deleting
nothing is a truthful no-op. Retroactive record closed; ticket 120
done.

(End of file - total 48 lines)
