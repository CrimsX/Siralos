---
title: "Session Profile Reload: Live Endpoint and Protocol Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "127"
supersedes: []
---

# Session Profile Reload: Live Endpoint and Protocol Entry Review

Ticket [127](../tickets/127-reload-live-endpoint-protocol.md) · entry review
[the Session Profile Reload entry review](157-reload-entry-review.md) ·
[the Session Profile Reload record](158-session-profile-reload.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record -- the inversion, stated plainly.** This entry review was
> written AFTER the change was implemented, verified, and committed (commits
> `6d74e80`, `283d022`, `f00e4ec`, 2026-09-12). There was no prior ticket and
> no prior entry review for this change; the provenance is the owner's in-chat
> direction on 2026-09-12 to continue the work directly, plus the deferred item
> decision 158 §3 had already recorded -- not a new design approval. The
> `/reload` approval that exists (2026-09-11, decision 157) is for a reload
> that re-reads declarative configuration only and never widens authority;
> applying the endpoint and protocol live stays inside that, because a reload
> applies exactly what a restart would apply from the same file and never more.
> Nothing more is claimed here, and nothing is backdated. The verdict below is a
> retroactive PASS over the committed diff, not a pre-commit authorization.

## 2. The Fixes

| Fix                             | The change                                                                                                                                                                                                                               | The evidence                                                                                                                                                                       |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F1 LIVE CELLS IN THE ADAPTERS   | The endpoint base and the protocol are live cells (`Rc<RefCell<..>>`) read when the request is built, not at construction, so a session can move them without being rebuilt.                                                             | `provider::tests::live_endpoint_and_protocol_switches_reach_the_next_request` asserts the URL the NEXT request builds                                                              |
| F2 APPLY TAKES THE WHOLE CONFIG | `ReloadedModel` -> `ReloadedConfig`: the apply step receives the model id, its display name, the endpoint base and the protocol as one value, and an all-empty recomposition still yields `None`.                                        | `reloaded_config`; the empty-snapshot guard                                                                                                                                        |
| F3 APPLY REPORTS ITSELF         | Each CHANGED cell is written to the live provider and named on the report (`applied: ... (live, no restart)`); an unchanged field stays a no-op, so the pure report path is byte-identical, and endpoint VALUES are never echoed.        | `apply_reloaded_config`; `interactive::tests::reload_applies_the_recomposed_model_to_the_live_session` (model, display name, endpoint and protocol asserted on the live accessors) |
| F4 TRUTHFUL TAIL                | The edited-profile report no longer ends `; live session unchanged` while the apply step runs in the same command; the parts name their own disposition instead, and a changed provider name or credential says `(restart to converge)`. | `reload_reports_edited_profile_changes_without_mutating` (asserts `would change` and the ABSENCE of the old claim)                                                                 |

## 3. Criteria -> Evidence

| Criterion                                        | Evidence                                                                                                                                     |
| ------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| An endpoint change reaches the next request      | F1 (the request URL, not a label) + F3                                                                                                       |
| A protocol change reaches the next request       | F1 + F3 (asserted on `live_protocol`)                                                                                                        |
| Reporting cannot mutate live state               | decision 158 F2 -- the report is still one pure function; only the apply step writes                                                         |
| What cannot be applied live is said, not implied | F4 -- provider name and credential carry `(restart to converge)`                                                                             |
| A reload cannot widen authority                  | decision 158 F5 untouched; the reload applies the same file a restart would read                                                             |
| The repository gate holds                        | `npm run check` exit 0 at `f00e4ec` -- 317 adapters / 25 conformance / 253 cli (1 ignored) / 614 core, 0 failed; differential parity 352/352 |

## 4. Result

**PASS (retroactive), with a narrowed scope limit.** Decision 158's limit is
half-consumed: model, endpoint and protocol now apply live, and the report says
which fields did. The remaining restriction is stated on the report rather than
in prose -- a changed provider name is display identity for the live session,
and there is no live credential cell, so both still require a restart. The live
routing itself is evidenced mechanically (the next request's URL), and the
terminal-level confirmation remains the owner's.
