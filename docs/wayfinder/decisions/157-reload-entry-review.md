---
title: "Session Profile Reload (/reload) Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-11"
ticket: "126"
supersedes: []
---

# Session Profile Reload (/reload) Entry Review

Ticket [126](../tickets/126-session-profile-reload.md) · entry review
[the Siralos TUI entry review](103-siralos-tui-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record -- the inversion, stated plainly.** This entry review was
> written AFTER the change was implemented, verified, and committed (commits
> `ba66019`, `1bf4f55`, `453e31d`, `54128d6`, 2026-09-11). There was no prior
> ticket and no prior entry review for this change; the approval that exists is
> an in-chat design approval from the human owner on 2026-09-11 -- a reload
> command that re-reads declarative configuration only and never widens
> authority. Nothing more is claimed here, and nothing is backdated. The verdict
> below is a retroactive PASS over the committed diff, not a pre-commit
> authorization.

## 2. The Fixes

| Fix                         | The change                                                                                                                                                                | The evidence                                                                                                                    |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| F1 SAME COMPOSITION PATH    | `/reload` re-reads through `load_workspace_profile` and recomposes through the composition path startup uses -- no second composition, no parallel validator.             | `recompose_provider_snapshot` (`crates/siralos-cli/src/interactive.rs`)                                                         |
| F2 PURE REPORT              | The report is produced by a pure function, so the reporting half cannot mutate live state; the model travels with it as `(report, Option<ReloadedModel>)`.                | `reload_report`; `interactive::tests::reload_reports_edited_profile_changes_without_mutating`                                   |
| F3 LIVE APPLY, MODEL ONLY   | A changed model moves the provider cell the NEXT request reads (`set_live_model`) and the display holders; the file is never written.                                     | `apply_reloaded_model`; `interactive::tests::reload_applies_the_recomposed_model_to_the_live_session`                           |
| F4 TRUTHFUL NON-APPLICATION | An invalid profile reports its diagnostic verbatim and changes nothing; an absent profile follows startup semantics.                                                      | `interactive::tests::reload_reports_invalid_diagnostic_verbatim_and_changes_nothing`, `reload_absent_follows_startup_semantics` |
| F5 AUTHORITY INVARIANT      | A profile requesting more than the Host grants is refused: the effective rules stay the Host's, no profile applies, and the refused profile never reaches the apply step. | `interactive::tests::reload_cannot_widen_session_authority`                                                                     |

## 3. Criteria -> Evidence

| Criterion                               | Evidence                                                                                                                         |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| A config change need not mean a restart | F1 + F3; the live accessor changes, not a label                                                                                  |
| Reporting cannot mutate                 | F2 (pure function)                                                                                                               |
| Bad input changes nothing and says why  | F4                                                                                                                               |
| A reload cannot widen authority         | F5 -- the acceptance criterion the owner attached                                                                                |
| The repository gate holds               | `npm run check` exit 0 at `54128d6` -- 316 adapters / 25 conformance / 253 cli / 614 core, 0 failed; differential parity 352/352 |

## 4. Result

**PASS (retroactive), with a recorded scope limit.** The authority invariant is
the part that made this safe to ship and it is pinned by a test. The limit is
equally part of the verdict: only the model applies live; provider, endpoint,
protocol and credential changes still report that a restart is required to
converge.
