---
title: "The Verbatim Credential and the Protocol Picker"
label: "wayfinder:ticket"
status: "open"
date: "2026-08-31"
supersedes: []
---

# The Verbatim Credential and the Protocol Picker

The api key field stores what the user types verbatim (env: prefix for the env-var form, otherwise the literal key — the form is an interface, not a validator; the config is gitignored; display surfaces redact key: values), the model picker gained a sliding viewport so the selection is always visible, provider errors render honestly and bounded (URL + status + a truncated body), and the api protocol is a selectable picker over openai-completions, openai-responses, and anthropic-messages with the legacy aliases accepted in the parse; the request-shaping use remains future work.

Authorized by [The Verbatim Credential and the Protocol Picker Entry Review](../decisions/137-marker-protocol-entry-review.md).
