---
title: "The Profile Endpoint as Base URL and the Protocol Chat Path"
label: "wayfinder:ticket"
status: closed
date: "2026-09-11"
supersedes: []
---

# The Profile Endpoint as Base URL and the Protocol Chat Path

The `[profile]` endpoint override meant two different things: chat
completions used it VERBATIM as the POST URL (`generic.rs:8`,
`client.post(endpoint)`) while the model listing appended `/models` to
it as a base. The TUI add-form teaches the base form, so a user who
followed it got a working model picker and a 404 on every chat request.
The fix: pure helpers `chat_url(endpoint, protocol)` and
`models_url(endpoint)` — `openai-completions` -> `/chat/completions`,
`openai-responses` -> `/responses`, `anthropic-messages` ->
`/messages`; an endpoint already ending in the protocol segment is
still used verbatim, so stored full-path configs keep working.
`GenericProvider` carries the profile protocol (`with_protocol`,
default `openai-completions`) and the registry gained
`from_provider_str_with_protocol`, wired from both CLI construction
paths.

> **Retroactive record.** This change was implemented, verified, and
> committed (commits `1ba70f3` "fix(provider): treat the profile
> endpoint as a base URL" and `853684e` "docs(core): the profile
> protocol now selects the chat POST path") BEFORE this ticket and its
> entry review were written. There was no prior ticket and no prior
> entry review for this change; the provenance is an owner bug report —
> the endpoint 404 was reported live (POST to the bare base returned
> 404; the same body to base + `/chat/completions` returned 200) —
> nothing more is claimed here, and nothing is backdated. The
> entry-review inversion is named in [the retroactive entry
> review](../decisions/149-profile-endpoint-protocol-entry-review.md):
> the record follows the implementation rather than authorizing it.

Authorized by
[the Profile Endpoint as Base URL and the Protocol Chat Path Entry Review (retroactive)](../decisions/149-profile-endpoint-protocol-entry-review.md).
Implemented and recorded in
[decision 150](../decisions/150-profile-endpoint-protocol.md).
