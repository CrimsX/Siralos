---
title: "Provider interface: the work deferred beside the implementation"
label: "wayfinder:ticket"
status: closed
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

## Resolution (2026-09-12)

The entry review took the second acceptance branch for all four items:
[decision 176](../decisions/176-provider-interface-entry-review.md) retires each one
with a reason AND the trigger that would change the answer.

- **Deferred tool loading** — retired: six tools with short definitions, and the
  mechanism would cost more than it saves. Trigger: evidence that definition bytes
  are a material share of the context budget, or a large optional-domain Tool
  surface.
- **Reasoning-effort tier gating** — retired from the neutral request: a vendor knob
  would be the first vendor-specific field in it; a profile that wants different
  reasoning picks a different model. Trigger: a provider that REQUIRES the field —
  arriving as a profile key mapped by the adapter.
- **Structured output** — retired as a duplicate: the Tool-call protocol already is
  the structured-output surface. Trigger: a need outside the tool loop, answered by
  another typed Tool.
- **Protocol shaping** (decision 132) — retired: the adapters own shaping; a form
  would restate adapter rules in the UI. Trigger: opaque failures on a
  field-heavy protocol, answered by a truthful adapter error, not form-side shaping.

Nothing in the provider interface is left "deferred" without an owner any more.
