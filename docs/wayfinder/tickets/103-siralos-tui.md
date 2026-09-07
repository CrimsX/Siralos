---
title: "The Siralos TUI"
label: "wayfinder:ticket"
status: closed
date: 2026-08-31
supersedes: []
blockedBy: []
---

# Ticket 103 — The Siralos TUI

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** none (authorized by decision 103)

## Question

The TUI is a frontend over the existing interactive session seam per the approved entry review (decision 103). It composes the same seams the stdio frontend uses and does not replace the session.

Sequence: T1 shell -> T2 approvals surface -> T3 context pane -> T4 render-model pinning.

- T1 shell — the TUI shell (transcript pane, input line, status line over the live session).
- T2 approvals surface — the approval surface (host-gated approval prompts as TUI modals).
- T3 context pane — the context pane (the decision 100 audit — counters and tick ring — and tool activity, live).
- T4 render-model pinning — render-model pinning (TestBackend frame snapshots as differential expectations, corpus bump).

Blocked by: none (authorized by decision 103).

## Resolution

Open — entry review PASS per [decision 103](../decisions/103-siralos-tui-entry-review.md) (HITL 2026-08-31, C1–C6 approved, ratatui + crossterm pinned). T1 authorized as the first slice.

Closed — T1-T4 delivered per decisions 104-108; the roll-up is [decisions/109-siralos-tui-rollup.md](../decisions/109-siralos-tui-rollup.md) (HITL 2026-08-31).
