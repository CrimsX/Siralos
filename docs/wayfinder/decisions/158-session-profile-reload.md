---
title: "Session Profile Reload (/reload)"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-11"
ticket: "126"
supersedes: []
---

# Session Profile Reload (/reload)

Ticket [126](../tickets/126-session-profile-reload.md) · entry review
[157](157-reload-entry-review.md) · [Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and committed
> (commits `ba66019`, `1bf4f55`, `453e31d`, `54128d6`) BEFORE this record was
> written. There was no prior ticket and no prior entry review; the approval
> that exists is an in-chat design approval from the human owner on 2026-09-11
> for a reload command that re-reads declarative configuration only and never
> widens authority -- nothing more is claimed here, and nothing is backdated.
> The entry review ([157](157-reload-entry-review.md)) is itself retroactive
> and names the inversion.

## 2. The Implemented

| Slice                              | The change                                                                                                                                                                                                                                                                                                               | The evidence                                                                                                                                                           |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M1 REPORT (`ba66019`)              | `/reload` re-reads through the startup loader and composition path and reports what would change; the reporting path is pure, so it cannot mutate. `/reload` joins the shared catalog (15 entries) and works in stdio without a TTY.                                                                                     | four reload tests; catalog counts moved 14 -> 15                                                                                                                       |
| M2 REFACTOR (`1bf4f55`)            | `reload_report` returns the recomposed model alongside the report, so an applying step never has to re-read or re-validate; the six-plus call sites destructure and otherwise behave identically.                                                                                                                        | pure refactor, behaviour and message text unchanged                                                                                                                    |
| M3 APPLY, MODEL ONLY (`453e31d`)   | A changed model moves the live provider cell the NEXT request reads (`set_live_model`) and both display holders; the file's display name is adopted because the file owns it; the profile file is never written; an unchanged model is a no-op so the pure path stays byte-identical. Wired at all three dispatch sites. | `interactive::tests::reload_applies_the_recomposed_model_to_the_live_session`                                                                                          |
| M4 TRUTHFUL NON-APPLICATION        | Invalid -> the diagnostic verbatim, nothing changed. Absent -> startup semantics. Refused -> nothing reaches the apply step (the recomposition yields no applicable model).                                                                                                                                              | `reload_reports_invalid_diagnostic_verbatim_and_changes_nothing`, `reload_absent_follows_startup_semantics`, and the invariant test's `recomposed.is_none()` assertion |
| M5 AUTHORITY INVARIANT (`54128d6`) | A profile declaring `[profile.permissions] workspace.write = "allow"` while the Host grants only `workspace.read` is REFUSED: the effective rules stay exactly the Host's own, no profile applies, and composing authority before and after a full reload yields the same value.                                         | `interactive::tests::reload_cannot_widen_session_authority`                                                                                                            |

## 3. Scope Limit, Recorded

Only the MODEL applies live. Provider, endpoint, protocol and credential changes
are REPORTED as needing a restart to converge; applying them live needs the same
live-cell treatment inside the adapters (`GenericProvider` holds the endpoint,
protocol and credential as plain fields today). This record does not claim
otherwise, and the report wording says "restart to converge" for exactly those
fields.

## 4. Criteria -> Evidence

| Criterion                                        | Evidence                                                                                                                                                                |
| ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Recomposition uses the startup path              | M1 (same loader, same composition)                                                                                                                                      |
| Reporting cannot mutate live state               | M1 + M2 (pure)                                                                                                                                                          |
| A changed model applies to the NEXT request      | M3 -- asserted on the live accessor, not a label                                                                                                                        |
| Bad or absent input changes nothing and says why | M4                                                                                                                                                                      |
| A reload cannot widen authority                  | M5 -- the owner's acceptance criterion                                                                                                                                  |
| The repository gate holds                        | `npm run check` exit 0 at `54128d6` -- 316 adapters / 25 conformance / 253 cli / 614 core, 0 failed; differential parity 352/352 with 4 platform skips and 0 deviations |

## 5. Result

The restart friction is reduced: a model change in `siralos.toml` now applies
live, and the parts that still need a restart say so instead of pretending. The
invariant the owner attached to the approval -- a reload never widens authority
-- is pinned by a test rather than asserted in prose.
