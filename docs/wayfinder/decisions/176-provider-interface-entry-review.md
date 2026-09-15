---
title: "Provider Interface Entry Review: Four Deferrals Ruled On"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "133"
supersedes: []
---

# Provider Interface Entry Review: Four Deferrals Ruled On

Ticket [133](../tickets/133-provider-interface-deferred-work.md) ·
[102](102-provider-interface-takeaways.md) · [132](132-six-field-provider-form.md) ·
[Map](../siralos-roadmap.md)

## 1. Why this is an entry review and not a slice

Ticket 133 exists because two decisions recorded deferred provider work in prose and
nothing owned it. Its acceptance has two branches — freeze a slice, or retire each
item with a reason — and this record takes the second branch for all four, with the
trigger that would change each answer. "Not now" with a named trigger is a ruling;
"not now" alone is the state that produced this ticket.

What was checked, rather than recalled: the registered Tool surface is the three
workspace tools plus `context.search`/`context.inspect`/`context.expand`; there is no
`reasoning_effort` (or equivalent) anywhere in the tree; and per-protocol request
shaping already lives in the adapters (`openai.rs`, `anthropic.rs`, `generic.rs`,
`sse.rs`, `tool_names.rs`).

## 2. The rulings

**Deferred tool loading — retired.** Tools are registered up front and the surface is
six tools with short definitions. Loading them on demand would add a mechanism
(dynamic registration, a second visibility path, a new failure mode) against a
measured cost that nobody has shown to matter. _Trigger:_ evidence that definition
bytes are a material share of the context budget (a projection/pressure measurement),
or an optional domain shipping a Tool surface large enough that eager loading is
wasteful.

**Reasoning-effort tier gating — retired from the neutral request.** The request model
is provider-neutral by design (ADR 0002, ADR 0036); a vendor knob would be the first
vendor-specific field in it, and a profile wanting different reasoning already has the
honest lever — pick a different model or provider. _Trigger:_ a provider whose API
REQUIRES the field rather than defaulting it, or the owner asking for the knob — and
then it arrives as a profile key mapped by the adapter, never as a neutral request
field.

**Structured output — retired as a duplicate.** The tool-call protocol already IS the
structured-output surface: a typed call with a validated input, a paired result, and a
host gate in the middle. A JSON-mode request plus a response validator would add a
second parsing path and a second failure taxonomy beside the one the loop relies on.
_Trigger:_ a genuine need for structured output OUTSIDE the tool loop — and then the
answer is another typed Tool, not a request mode.

**Protocol shaping (decision 132) — retired.** The add-form stores the protocol and
the adapters own the shaping that follows from it. A form that previewed or validated
protocol-specific fields would restate adapter rules in the UI layer — the duplicated
authority this codebase keeps refusing. _Trigger:_ a protocol whose required fields
differ enough that a wrong profile fails opaquely; the fix there is a truthful adapter
error that names the missing field, not form-side shaping.

## 3. What this closes

Ticket 133. Nothing in the provider interface is left "deferred" without an owner:
each item is either retired with a trigger (above) or belongs to a decision that owns
it (the credential boundary, HAR-037; routing, HAR-022 / ADR 0036, deliberately not
committed).

## 4. Evidence

- `crates/siralos-adapters/src/tool/` (three workspace tools) and the `context.*` tool
  ids; zero matches for `reasoning_effort` across `crates/**`.
- The adapter files above, which shape per protocol today.
- `npm run check` exit 0 with this record in place (documentation-only change).
