# Ticket — Enable/Activate Host-Gated UI Entry Review

**Type:** `wayfinder:grilling` HITL · **Status:** OPEN (resolver in session)
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Add Plugin UI Entry Review](38-add-plugin-ui-entry-review.md) (PASS, slice implemented at `7fe9b66`)

## Decision to make

Freeze the implementation contract for `Enable`/`Activate` Host-gated UI for installed domain plugins (decision 34 §2 Host authority, decision 38 §3 deferred). No code until authorized.

## Open questions for HITL

1. Command surface for enable/activate (separate commands vs subcommands)?
2. Host authority gate: which capabilities gate Enable vs Activate (empty vs declared)?
3. Whether Enable writes `siralos.toml` `enabled = true` or is memory-only until Activate?
4. Failure presentation when Enable/Activate is Host-denied or component missing?
