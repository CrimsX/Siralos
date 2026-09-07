---
title: "The Siralos TUI Roll-Up"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "103"
supersedes: []
---

# 109 — The Siralos TUI Roll-Up

Ticket [103](../tickets/103-siralos-tui.md) · entry review [103](103-siralos-tui-entry-review.md) · Map.

> **User-directed 2026-08-31 (session HITL).** The TUI arc is complete: T1-T4 delivered and pinned, the consolidation debt is settled to the terminal-I/O residual with its why, the TUI is the default frontend on TTYs with a --stdio escape hatch, and the render model is corpus-pinned. This record consolidates the arc and closes ticket 103; no behavior changes.

## 2. Realized slices — outcome + decision anchor

| Slice                       | One-line outcome                                                                                                                                                                                                                                                                      | Anchor       |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ |
| The entry review            | C1-C6 approved, ratatui 0.30.2 + crossterm 0.29.0 pinned in the CLI crate only                                                                                                                                                                                                        | Decision 103 |
| T1 the shell                | The pure deterministic render model over TestBackend, the sanitizer boundary intact, non-TTY fallback                                                                                                                                                                                 | Decision 104 |
| The default-entry amendment | TUI default on TTYs, --stdio escape hatch, silent non-TTY stdio                                                                                                                                                                                                                       | Decision 105 |
| T2 the approval surface     | Modals over the unchanged host gates, the same evaluation one-definition proven, keys suppressed while pending                                                                                                                                                                        | Decision 106 |
| T3 the context pane         | The decision 100 audit and tool activity live, single-sourced from the ContextMetrics the /context segment uses, byte-transparent when off                                                                                                                                            | Decision 107 |
| T4 render-model pinning     | Four TestBackend frame snapshots in the differential corpus, the dispatch and session-composition debt settled — parse_slash_command, dispatch_stdio_command/dispatch_tui_command, compose_session/SessionComposition single shared, the terminal-I/O residual permanent with its why | Decision 108 |

## 3. Verification criteria → evidence

| #   | Criterion                                        | Evidence                                                                                                                                                                                                                                                                                                                       |
| --- | ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| (1) | The decision 103 red lines held                  | Sanitizer single boundary, input-queue read owner, command catalog, host-gated approvals, no threads, no new authority — per the decision 104–108 criteria tables, all pass                                                                                                                                                    |
| (2) | Stdio byte-unchanged for scripts and CI          | Existing tests throughout — all pre-existing `interactive::tests` pass untouched across T1–T4; see decisions 104–108 §3                                                                                                                                                                                                        |
| (3) | The render model deterministic and corpus-pinned | Four TestBackend frame snapshots at corpus v76/357; audit Differential audit: parity held (352/352 applicable required scenarios; 4 explicit platform skips; 0 accepted informational deviations).                                                                                                                             |
| (4) | The consolidation ledger final                   | parse_slash_command, dispatch_stdio_command/dispatch_tui_command, compose_session/SessionComposition, handle_key, the approval evaluation, the audit/pane gating shared; the residual is per-frontend terminal I/O only, permanent with its why (unsafe_code = forbid bars pointer tricks) — per the decision 108 final ledger |
| (5) | Fresh full-gate evidence                         | fmt/clippy/tests clean, expectations 118 records, pinned v32 oracle untouched                                                                                                                                                                                                                                                  |

## 4. Result

The Siralos TUI arc is Verified complete: T1-T4 as authorized by the decision 103 entry review with the user's TUI-default amendment, the consolidation debt settled to the permanent terminal-I/O residual, and the render model pinned in the differential corpus. Ticket 103 is closed. Deferred/known limitations recorded: the blocking provider round freezes the redraw (no threads is a frozen-clause consequence; revisit only if the run model ever changes); the stdio frontend remains the --stdio escape hatch and the silent non-TTY path.
