---
title: "The Audit Remediation Pass"
label: "wayfinder:ticket"
status: closed
date: 2026-08-31
supersedes: []
---

# Ticket 105 — The Audit Remediation Pass

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Authorized by:** [decision 112](../decisions/112-audit-remediation-entry-review.md)

## Question

The external-model project audit (five focused chunks, file:line evidence) found two real bugs and several mechanical issues. The mechanical remediation batch is authorized now; six HITL questions raised by the audit are presented OPEN in decision 112 and are explicitly NOT implemented in this pass — they need user direction.

## Mechanical batch (authorized by decision 112)

- R1 (HIGH) inspect slash fix — `parse_node_id_input` rejects `/` while scan node ids ARE slash paths; derivation only fires on `Success` so demand events never fire for rejected inspects.
- R2 double-scroll fix — `handle_key` and the TUI loop both apply `+-10`.
- R3 single catalog — two catalogs both claiming single source.
- R4 echo sanitizer gap — user echo unsanitized before the single output boundary.
- R5 status sanitizer gap — provider/model interpolations unsanitized.
- R6 compose ordering — `compose_session` after `TerminalGuard::enter` hides startup diagnostics.
- R7 harness gate mirror — `context_session_record` missing the applied-profile gate the live session enforces.
- R8 docs staleness sweep — `README.md`, `docs/development/PROJECT_CONTEXT.md`, `AGENTS.md`, `ARCHITECTURE.md` claims contradict the code.
- R9 tripwire tests for R1 (part of R1).

## Resolution

Open — entry review PASS per [decision 112](../decisions/112-audit-remediation-entry-review.md) (HITL 2026-08-31, mechanical batch R1–R9 authorized, six HITL questions OPEN and out of scope). Implementation tracked in [decision 113](../decisions/113-audit-remediation-pass.md).

Closed — the mechanical batch per decisions 112-113 and the HITL rulings per decision 114 (HITL 2026-08-31).
