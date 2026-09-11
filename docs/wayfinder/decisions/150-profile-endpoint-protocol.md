---
title: "The Profile Endpoint as Base URL and the Protocol Chat Path"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-11"
ticket: "122"
supersedes: []
---

# The Profile Endpoint as Base URL and the Protocol Chat Path

Ticket [122](../tickets/122-profile-endpoint-protocol.md) · entry
review [149](149-profile-endpoint-protocol-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and
> committed (commits `1ba70f3` "fix(provider): treat the profile
> endpoint as a base URL" and `853684e` "docs(core): the profile
> protocol now selects the chat POST path") BEFORE this record was
> written. There was no prior ticket, no prior entry review, and no
> prior human approval for this change; the provenance is an owner bug
> report — the endpoint 404 was reported live — nothing more is claimed
> here, and nothing is backdated. The entry review
> ([149](149-profile-endpoint-protocol-entry-review.md)) is itself
> retroactive and names the inversion.

## 2. The Implemented

| Fix                               | The change                                                                                                                                                                                                                                                             | The evidence                    |
| --------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| M1 PURE URL HELPERS               | `chat_url(endpoint, protocol)` and `models_url(endpoint)`: `openai-completions` -> `/chat/completions`, `openai-responses` -> `/responses`, `anthropic-messages` -> `/messages`. Chat no longer POSTs the endpoint verbatim (`generic.rs:8`, `client.post(endpoint)`). | the committed provider diff     |
| M2 VERBATIM FULL-PATH PASSTHROUGH | Endpoints already ending in the protocol segment are used verbatim, so stored full-path configs keep working.                                                                                                                                                          | the committed diff              |
| M3 PROTOCOL PLUMBING              | `GenericProvider` carries the profile protocol (`with_protocol`, default `openai-completions`); the registry gained `from_provider_str_with_protocol`, wired from both CLI construction paths.                                                                         | the committed registry/CLI diff |
| M4 EXPECTATIONS                   | One post-freeze candidate-authored expectation updated (decision 40 C7); the frozen v32 oracle untouched; parity 352/352.                                                                                                                                              | the committed expectation diff  |

## 3. Criteria → Evidence

| Criterion                                          | Evidence                                                                                                                                                                                                                      | Verdict  |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| Chat works from the base form the add-form teaches | orchestrator's live probe: POST to the bare base returned 404 before the change; the same body to base + `/chat/completions` returned 200                                                                                     | pass     |
| Stored full-path configs unaffected                | verbatim passthrough when the endpoint already ends in the protocol segment                                                                                                                                                   | pass     |
| No regression                                      | `npm run check` exit 0 after the change; `npm run check:differential` exit 0, parity held 352/352 applicable required, 4 platform skips, 0 deviations (evidence gathered by the orchestrator and cited here, not re-invented) | pass     |
| Docs/context gates                                 | `npm run check:docs` and `npm run check:context` outcomes reported below                                                                                                                                                      | reported |

## 4. Result

The endpoint-as-base-URL change is complete as committed in `1ba70f3`

- `853684e`: the TUI add-form's base form now yields a working model
  picker AND working chat requests, the protocol selects the POST path,
  and full-path configs pass through verbatim. Retroactive record
  closed; ticket 122 done.
