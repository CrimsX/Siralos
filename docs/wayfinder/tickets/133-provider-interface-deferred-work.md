---
title: "Provider interface: the work deferred beside the implementation"
label: "wayfinder:ticket"
status: open
date: "2026-09-12"
supersedes: []
---

# Provider interface: the work deferred beside the implementation

Two decisions recorded deferred work in prose and nothing else owns it (found by the
ticket 131 sweep). Neither is committed; this ticket exists so the deferral has a
home instead of a sentence.

## 1. From decision 102 (provider-interface takeaways)

- **Deferred tool loading** — tools are registered up front; loading them on demand
  is unbuilt.
- **Reasoning-effort tier gating** — the request carries no reasoning-effort field.
- **Structured output** — no response-format request/validation surface.

## 2. From decision 132 (six-field provider form)

- **Protocol shaping** — per-protocol request shaping beyond what the chat path
  needs is "recorded as future work".

## Acceptance

An entry review that either scopes one or more of these into a slice with a frozen
contract (subjects, evidence, bounds) or retires each with a reason. Nothing here is
implemented before that review: the point is to stop carrying unowned intent.

## Out of scope

- Multi-provider routing or model selection policy (ADR 0036: routing is not
  foundational state semantics; HAR-022 stays NOT DUE).
- Any change to the credential boundary: credentials stay env-only and never enter
  context (HAR-037).
