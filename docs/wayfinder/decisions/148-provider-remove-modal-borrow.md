---
title: "Scope the Modal Borrow So Provider Removal Does Not Panic"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-10"
ticket: "121"
supersedes: []
---

# Scope the Modal Borrow So Provider Removal Does Not Panic

Ticket [121](../tickets/121-provider-remove-modal-borrow.md) · entry
review [147](147-provider-remove-modal-borrow-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `8306de8` "fix(tui): scope the modal borrow so
> /provider remove does not panic", 2026-09-10) BEFORE this record was
> written. There was no prior ticket and no prior entry review for this
> change; the provenance is an owner bug report in-session about a
> feature that had just shipped ("unable to remove providers, crashes
> app") — nothing more is claimed here, and nothing is backdated. The
> entry review
> ([147](147-provider-remove-modal-borrow-entry-review.md)) is itself
> retroactive and names the inversion.

## 2. The Implemented

| Fix                       | The change                                                                                                                                                                                                                                                                                            | The evidence                                |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| M1 SCOPED DECISION BORROW | The modal branch (`crates/siralos-cli/src/interactive.rs`) takes its decision inside a block that ends the mutable borrow before the body re-borrows: the if-let scrutinee `RefMut` from `handle_modal_key(&mut tui_state.borrow_mut(), key)` no longer outlives into a `tui_state.borrow()`.         | the committed `interactive.rs` diff         |
| M2 TESTABLE DECISION STEP | The step is extracted as `handle_pending_approval_key(&RefCell<TuiState>, ...)` so the loop's own decision path is unit-testable rather than only reachable through a real terminal.                                                                                                                  | three new `interactive::tests::tui_modal_*` |
| M3 PATHS PRESERVED        | The dormant approval path still reports `Approved.`/`Denied.`; removal still resolves through the single outcome both frontends call.                                                                                                                                                                 | the committed diff                          |
| M4 ATTRIBUTION, HONESTLY  | The broken shape predates the feature — the dormant `Approved.`/`Denied.` branch had the same shape — but nothing had ever set `pending_approval` (approvals dormant per decision 114), so the deletion feature shipped in `8855be8` was the first thing to make it reachable and is what exposed it. | decisions 114 and 145/146                   |
| M5 CLASS SWEEP            | A sweep of all 55 `borrow()`/`borrow_mut()` sites found no other reachable occurrence of the class.                                                                                                                                                                                                   | the orchestrator's sweep, cited not re-run  |

## 3. Criteria → Evidence

| Criterion                                                   | Evidence                                                                                                                                                                                                                                                                                                               | Verdict  |
| ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| The remove modal answers without panic; removal is effected | three new `interactive::tests::tui_modal_*` tests panicked with "RefCell already mutably borrowed" before the change and pass after                                                                                                                                                                                    | pass     |
| Edition-2024 scrutinee semantics established                | standalone rustc probe: the identical pattern fails to compile under editions 2015/2021 and panics under 2024 (`Cargo.toml:14`)                                                                                                                                                                                        | pass     |
| Frontend contracts unchanged                                | dormant path still reports `Approved.`/`Denied.`; removal resolves through the single outcome both frontends call                                                                                                                                                                                                      | pass     |
| No regression                                               | `cargo test --workspace --all-targets --all-features --locked` exit 0 (614 core / 306 adapters / 25 conformance / 230 cli, 0 failed); `npm run check:rust` green; `npm run check:differential` exit 0, parity held 352/352 applicable required (evidence gathered by the orchestrator and cited here, not re-invented) | pass     |
| Docs/context gates                                          | `npm run check:docs` and `npm run check:context` outcomes reported below                                                                                                                                                                                                                                               | reported |

## 4. Result

The modal-borrow panic is fixed as committed in `8306de8`: answering
the "- Remove provider" confirmation (y, n, or Esc) no longer kills
the app, the provider is removed through the one outcome both
frontends call, and the dormant approval path is untouched. The defect
predates the deletion feature that exposed it; this record's own
feature (ticket 120, decisions 145/146) shipped with the defect
reachable. Retroactive record closed; ticket 121 done.

(End of file - total 48 lines)
