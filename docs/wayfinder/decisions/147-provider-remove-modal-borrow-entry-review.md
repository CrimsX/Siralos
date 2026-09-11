---
title: "Scope the Modal Borrow So Provider Removal Does Not Panic Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-10"
ticket: "121"
supersedes: []
---

# Scope the Modal Borrow So Provider Removal Does Not Panic Entry Review

Ticket [121](../tickets/121-provider-remove-modal-borrow.md) · entry
review [the Siralos TUI entry review](103-siralos-tui-entry-review.md)
· [Map](../siralos-roadmap.md)

> **Retroactive record — the inversion, stated plainly.** This entry
> review was written AFTER the change was implemented, verified, and
> committed (commit `8306de8` "fix(tui): scope the modal borrow so
> /provider remove does not panic", 2026-09-10). There was no prior
> ticket, no prior entry review, and no prior human approval for this
> change, and this record does not invent or backdate any of them. The
> provenance is an owner bug report in-session about a feature that had
> just shipped ("unable to remove providers, crashes app"), not a
> design approval. The normal order (entry review authorizes →
> implementation lands) is inverted here: the implementation landed
> first and this review records what it should have authorized. The
> verdict below is therefore a retroactive PASS over the
> already-committed diff, not a pre-commit authorization.

> **Owner-reported 2026-09-10 (retroactive).** Choosing "- Remove
> provider" in the TUI opened the confirmation modal and the app died
> the moment the user answered it (y, n, or Esc); the provider was
> never removed. Root cause: the event loop's modal branch
> (`crates/siralos-cli/src/interactive.rs`) held the `RefMut` created
> in its if-let scrutinee —
> `handle_modal_key(&mut tui_state.borrow_mut(), key)` — for the whole
> body. Under edition 2024 (`Cargo.toml:14`) that temporary lives to
> the end of the if-let, so the first `tui_state.borrow()` inside the
> body panicked with "RefCell already mutably borrowed". The
> orchestrator established the semantics with a standalone rustc probe:
> the identical pattern fails to compile under editions 2015/2021 and
> panics under 2024. The broken shape predates the feature — the
> dormant `Approved.`/`Denied.` branch had the same shape — but nothing
> had ever set `pending_approval` (approvals are dormant per decision
> 114), so the deletion feature shipped in `8855be8` was the first
> thing to make it reachable. The feature did not introduce the defect
> and is what exposed it. The fix: take the decision inside a block
> that ends the mutable borrow before the body re-borrows, extracted
> into `handle_pending_approval_key(&RefCell<TuiState>, ...)` so the
> loop's decision path is unit-testable rather than only reachable
> through a real terminal; the dormant approval path still reports
> `Approved.`/`Denied.`, and removal still resolves through the single
> outcome both frontends call.

## 2. The Fixes

| Fix                       | The change                                                                                                                                                                                   | The evidence                                    |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| F1 SCOPED DECISION BORROW | The modal branch takes its decision inside a block that ends the mutable borrow before the body re-borrows, so no `borrow()` runs while the scrutinee `RefMut` is alive.                     | the committed `interactive.rs` diff             |
| F2 TESTABLE DECISION STEP | The step is extracted as `handle_pending_approval_key(&RefCell<TuiState>, ...)` so the loop's own decision path is covered by unit tests rather than only reachable through a real terminal. | three new `interactive::tests::tui_modal_*`     |
| F3 DORMANT PATH PRESERVED | The dormant approval path still reports `Approved.`/`Denied.`; removal still resolves through the single outcome both frontends call — no frontend contract changes.                         | the committed diff; borrower sweep below        |
| F4 CLASS SWEEP            | A sweep of all 55 `borrow()`/`borrow_mut()` sites found no other reachable occurrence of the class (scrutinee-held `RefMut` re-borrowed in the body).                                        | the orchestrator's sweep cited here, not re-run |

## 3. Criteria → Evidence

| Criterion                                                    | Evidence                                                                                                                                                                                                                                                                                                     | Verdict |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------- |
| Answering the remove modal no longer panics; removal happens | three new `interactive::tests::tui_modal_*` tests: each panicked with "RefCell already mutably borrowed" before the change and passes after                                                                                                                                                                  | pass    |
| Root cause is the edition-2024 scrutinee lifetime, as stated | orchestrator's standalone rustc probe: the identical pattern fails to compile under editions 2015/2021 and panics under 2024; `Cargo.toml:14` pins edition 2024                                                                                                                                              | pass    |
| Dormant approval path and frontend contract unchanged        | `Approved.`/`Denied.` reporting preserved in the committed diff; removal resolves through the single outcome both frontends call                                                                                                                                                                             | pass    |
| No other reachable occurrence of the class                   | sweep of all 55 `borrow()`/`borrow_mut()` sites found no other reachable occurrence                                                                                                                                                                                                                          | pass    |
| No regression                                                | `cargo test --workspace --all-targets --all-features --locked` exit 0 (614 core / 306 adapters / 25 conformance / 230 cli, 0 failed); `npm run check:rust` green; `npm run check:differential` exit 0, parity 352/352 applicable required (evidence gathered by the orchestrator and cited here, not re-run) | pass    |

## 4. Result

Entry review PASS (retroactive): scoping the modal borrow as
committed in `8306de8` is the correct fix — the decision is taken
inside a block that ends the mutable borrow, the step is extracted so
the loop's decision path is unit-tested, the dormant approval path is
preserved, and the class sweep shows no other reachable occurrence.
This review authorized nothing (the code had already landed); it
records what authorization would have covered.
