---
title: "Session Profile Reload: Live Endpoint and Protocol"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "127"
supersedes: []
---

# Session Profile Reload: Live Endpoint and Protocol

Ticket [127](../tickets/127-reload-live-endpoint-protocol.md) · entry review
[159](159-reload-live-apply-entry-review.md) · [Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and committed
> (commits `6d74e80`, `283d022`, `f00e4ec`) BEFORE this record was written.
> There was no prior ticket and no prior entry review; the provenance is the
> owner's in-chat direction on 2026-09-12 to continue the work directly, plus
> the deferred item decision 158 §3 had already recorded. Nothing more is
> claimed here, and nothing is backdated.

## 2. The Implemented

| Slice                        | The change                                                                                                                                                                                                                                                                                                                                | The evidence                                                                                                             |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| M1 LIVE CELLS (`6d74e80`)    | The endpoint base, the protocol and the model are live cells inside the providers, read when the request is built; `HostProvider` gained `set_live_endpoint`/`live_endpoint`/`set_live_protocol`/`live_protocol` beside the existing model pair (`SessionProvider` forwards the same four).                                               | `provider::tests::live_endpoint_and_protocol_switches_reach_the_next_request` -- the NEXT request's URL follows the cell |
| M2 APPLY (`283d022`)         | `ReloadedConfig` carries model, display name, endpoint and protocol; `apply_reloaded_config` writes each CHANGED cell to the live provider and appends one `applied: ... (live, no restart)` line per apply; both dispatch sites (stdio + TUI) hand the endpoint and protocol holders down as `&mut` so the status readouts stay in step. | `interactive::tests::reload_applies_the_recomposed_model_to_the_live_session` (model, display name, endpoint, protocol)  |
| M3 TRUTHFUL TAIL (`f00e4ec`) | The edited-profile report ended `; live session unchanged`, which the apply step contradicted one line later; the tail now states the diff only and each part names its disposition -- a changed provider name or credential says `(restart to converge)`.                                                                                | `reload_reports_edited_profile_changes_without_mutating` (asserts `would change` and the absence of the old claim)       |

## 3. Scope Limit, Recorded

A changed provider NAME and a changed CREDENTIAL still require a restart, and
now say so on the report instead of relying on prose: the provider name is the
live session's display identity, and there is no live credential cell (the
credential is resolved once at composition). Decision 158's limit is therefore
half-consumed, and this record claims only the half it consumed.

> **Correction (2026-09-12, decision 162 M9).** The paragraph above is superseded for the CREDENTIAL: the credential became a live cell, so `/reload` resolves and applies a changed credential for the generic provider (and CLEARS it when a profile stops declaring one), reporting what it could not apply instead of staying silent. A changed provider NAME still needs a restart -- it is the live session's display identity. The limit left standing is narrower than this record states.

## 4. Criteria -> Evidence

| Criterion                                         | Evidence                                                                                                                                     |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| A reload's endpoint change reaches the wire       | M1 (URL built from the cell) + M2                                                                                                            |
| A reload's protocol change reaches the wire       | M1 + M2 (`live_protocol` asserted, not a label)                                                                                              |
| Only changed fields write; unchanged fields no-op | M2 -- the pure report path stays byte-identical                                                                                              |
| Endpoint values are never echoed                  | M2 (the report says `endpoint changed`) and the pre-existing no-echo assertion in `reload_reports_edited_profile_changes_without_mutating`   |
| Nothing that needs a restart pretends otherwise   | M3 + §3                                                                                                                                      |
| A reload cannot widen authority                   | decision 158 M5 untouched (`reload_cannot_widen_session_authority` still green)                                                              |
| The repository gate holds                         | `npm run check` exit 0 at `f00e4ec` -- 317 adapters / 25 conformance / 253 cli (1 ignored) / 614 core, 0 failed; differential parity 352/352 |

## 5. Result

The restart friction is reduced by the half that had a mechanical cause: the
endpoint base and the protocol are live cells now, so `/reload` moves them the
same way it already moved the model, and it names each apply. The half without a
live cell -- a changed provider name, a changed credential -- says `(restart to
converge)` on the report instead of being described in a document, and the
sentence that used to deny the whole thing (`; live session unchanged`) is gone
and pinned by a test.
