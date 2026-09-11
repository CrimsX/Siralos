---
title: "The Profile Endpoint as Base URL and the Protocol Chat Path Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-11"
ticket: "122"
supersedes: []
---

# The Profile Endpoint as Base URL and the Protocol Chat Path Entry Review

Ticket [122](../tickets/122-profile-endpoint-protocol.md) · entry
review [the Siralos TUI entry review](103-siralos-tui-entry-review.md)
· [Map](../siralos-roadmap.md)

> **Retroactive record — the inversion, stated plainly.** This entry
> review was written AFTER the change was implemented, verified, and
> committed (commits `1ba70f3` "fix(provider): treat the profile
> endpoint as a base URL" and `853684e` "docs(core): the profile
> protocol now selects the chat POST path"). There was no prior ticket,
> no prior entry review, and no prior human approval for this change,
> and this record does not invent or backdate any of them. The
> provenance is an owner bug report — the endpoint 404 was reported
> live, confirmed by the orchestrator before the change (POST to the
> bare base returned 404; the same body to base + `/chat/completions`
> returned 200) — not a design approval. The normal order (entry review
> authorizes → implementation lands) is inverted here: the
> implementation landed first and this review records what it should
> have authorized. The verdict below is therefore a retroactive PASS
> over the already-committed diff, not a pre-commit authorization.

> **Owner-reported (retroactive).** The `[profile]` endpoint override
> meant two different things: chat completions used it VERBATIM as the
> POST URL (`generic.rs:8`, `client.post(endpoint)`) while the model
> listing appended `/models` to it as a base. The TUI add-form teaches
> the base form, so a user who followed it got a working model picker
> and a 404 on every chat request. The fix: pure helpers
> `chat_url(endpoint, protocol)` and `models_url(endpoint)` —
> `openai-completions` -> `/chat/completions`, `openai-responses` ->
> `/responses`, `anthropic-messages` -> `/messages`; an endpoint already
> ending in the protocol segment is still used verbatim, so stored
> full-path configs keep working; `GenericProvider` carries the profile
> protocol (`with_protocol`, default `openai-completions`) and the
> registry gained `from_provider_str_with_protocol`, wired from both
> CLI construction paths.

## 2. The Fixes

| Fix                                 | The change                                                                                                                                                                                                                      | The evidence                                |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| F1 PURE URL HELPERS                 | `chat_url(endpoint, protocol)` and `models_url(endpoint)` select the POST path from the profile protocol: `openai-completions` -> `/chat/completions`, `openai-responses` -> `/responses`, `anthropic-messages` -> `/messages`. | the committed provider diff                 |
| F2 VERBATIM FULL-PATH PASSTHROUGH   | An endpoint already ending in the protocol segment is used verbatim, so stored full-path configs keep working — no forced migration of existing profiles.                                                                       | the committed diff; passthrough tests       |
| F3 PROTOCOL CARRIED BY THE PROVIDER | `GenericProvider` carries the profile protocol (`with_protocol`, default `openai-completions`); the registry gained `from_provider_str_with_protocol`, wired from both CLI construction paths.                                  | the committed registry/CLI diff             |
| F4 PARITY PRESERVED                 | One post-freeze candidate-authored expectation was updated (decision 40 C7); the frozen v32 oracle is untouched; parity 352/352.                                                                                                | the committed expectation diff; audit below |

## 3. Criteria → Evidence

| Criterion                                                             | Evidence                                                                                                                                                                                                            | Verdict |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Base-form endpoints chat successfully; full-path configs keep working | orchestrator's live probe before the change (bare base 404, base + `/chat/completions` 200); verbatim passthrough for endpoints already ending in the protocol segment                                              | pass    |
| Protocol selects the chat POST path for all three protocols           | `chat_url` mapping tests for `openai-completions` / `openai-responses` / `anthropic-messages`                                                                                                                       | pass    |
| No regression                                                         | `npm run check` exit 0 after the change; `npm run check:differential` exit 0, parity 352/352 applicable required, 4 platform skips, 0 deviations (evidence gathered by the orchestrator and cited here, not re-run) | pass    |

## 4. Result

Entry review PASS (retroactive): treating the profile endpoint as a
base URL with the protocol selecting the chat POST path, as committed
in `1ba70f3` + `853684e`, is the correct fix — base-form configs chat
successfully, stored full-path configs pass through verbatim, and the
frozen oracle is untouched. This review authorized nothing (the code
had already landed); it records what authorization would have covered.
