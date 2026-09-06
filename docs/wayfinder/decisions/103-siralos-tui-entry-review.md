---
title: "The Siralos TUI Entry Review"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "103"
supersedes: []
---

# 103 — The Siralos TUI Entry Review

Governing plan 68 · entry review — · Map.

> **User-directed 2026-08-31 (session HITL): C1–C6 approved as drafted, with C2 decided for ratatui + crossterm (pinned versions) over hand-rolled ANSI. Authorized as the next implementation arc: T1 the TUI shell.**

## 2. Approved criteria (C1–C6) — as presented to and approved by the user 2026-08-31

- C1 Architecture: a TUI frontend over the existing session seam, not a replacement — it composes the same seams the stdio frontend uses (terminal sanitizer as the single output boundary, input queue as the single interactive-read owner, command catalog as the vocabulary source, approvals as host-gated). The TUI is a pure state-to-frame render model — deterministic and headless-testable.
- C2 Dependencies: ratatui + crossterm, pinned versions, added to the CLI crate only (dependency direction preserved: cli may depend on infrastructure-adjacent crates; core and adapters gain nothing). Rationale: TestBackend renders frames headlessly into buffers, fitting the differential-test discipline (render snapshots become corpus-pinnable).
- C3 Slices: T1 the TUI shell (transcript pane, input line, status line over the live session); T2 the approval surface (host-gated approval prompts as TUI modals — same gates, new presentation); T3 the context pane (the decision 100 audit — counters and tick ring — and tool activity, live); T4 render-model pinning (TestBackend frame snapshots as differential expectations, corpus bump).
- C4 Safety: no new authority anywhere; the sanitizer/input-queue/command-catalog triad untouched; approvals remain host-gated — the TUI changes presentation only; the non-TTY path falls back to the stdio frontend.
- C5 Testing: headless frame-buffer tests per slice plus differential pinning of the render model at each corpus bump; determinism byte-equal as usual.
- C6 Out of scope: multi-agent panes, workflows, session messaging, themes, mouse interaction — ADR 0036 uncommitted machinery stays out.

## 3. Criteria → evidence

| Criterion       | Authorization source                                                                                                                                                                                                                                                                                  | Status |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| C1 Architecture | Presented draft approved by the user 2026-08-31 (ratatui recommendation accepted) — C1 as drafted; recon evidence: interactive.rs 1875 lines, run_interactive_session generic over R/W with 40+ headless tests, B3b/B4 wiring present, no TUI dependencies in the workspace                           | pass   |
| C2 Dependencies | Presented draft approved by the user 2026-08-31 (ratatui + crossterm pinned versions over hand-rolled ANSI — C2 decided); recon evidence: interactive.rs 1875 lines, run_interactive_session generic over R/W with 40+ headless tests, B3b/B4 wiring present, no TUI dependencies in the workspace    | pass   |
| C3 Slices       | Presented draft approved by the user 2026-08-31 (T1–T4 as drafted, T1 authorized first); recon evidence: interactive.rs 1875 lines, run_interactive_session generic over R/W with 40+ headless tests, B3b/B4 wiring present, no TUI dependencies in the workspace                                     | pass   |
| C4 Safety       | Presented draft approved by the user 2026-08-31 (no new authority, triad untouched, host-gated approvals, non-TTY fallback); recon evidence: interactive.rs 1875 lines, run_interactive_session generic over R/W with 40+ headless tests, B3b/B4 wiring present, no TUI dependencies in the workspace | pass   |
| C5 Testing      | Presented draft approved by the user 2026-08-31 (headless + differential pinning, byte-equal determinism); recon evidence: interactive.rs 1875 lines, run_interactive_session generic over R/W with 40+ headless tests, B3b/B4 wiring present, no TUI dependencies in the workspace                   | pass   |
| C6 Out of scope | Presented draft approved by the user 2026-08-31 (multi-agent/workflow machinery out of scope per ADR 0036); recon evidence: interactive.rs 1875 lines, run_interactive_session generic over R/W with 40+ headless tests, B3b/B4 wiring present, no TUI dependencies in the workspace                  | pass   |

## 4. Result

Entry review PASS: T1 (the TUI shell) is authorized as the first slice with ratatui + crossterm pinned in the CLI crate; the session seams compose unchanged; T2-T4 follow with their own records.
