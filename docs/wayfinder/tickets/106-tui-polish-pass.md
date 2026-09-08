---
title: "The TUI Polish Pass"
label: "wayfinder:ticket"
status: open
date: 2026-08-31
supersedes: []
---

# Ticket 106 — The TUI Polish Pass

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Authorized by:** [decision 117](../decisions/117-tui-polish-entry-review.md)

## Question

Two drivers: (a) the user reports input STILL feels very delayed in the live TUI — root-caused: the drain's inner `poll(Duration::from_millis(15))` WAITS up to 15ms for more keys before the drain exits and the draw runs, so every keystroke's screen update is gated on that timeout (`interactive.rs` ~1926–1932); (b) the user asked for a nicer-looking TUI referencing the MiMo-Code agent's aesthetic ([github.com/XiaomiMiMo/MiMo-Code](https://github.com/XiaomiMiMo/MiMo-Code)) — the takeaways adopted: rounded borders, role-colored transcript lines, a session header bar, styled palette popup, and a context-usage readout in the status line.

## Fixes P1–P6 (authorized by decision 117)

- **P1 LATENCY (priority one):** restructure the loop — the inner drain uses `poll(Duration::ZERO)` (only already-queued events; NEVER waits), and after the drain the draw runs immediately; the outer loop keeps ONE bounded idle poll (raise 15ms → 50ms) so the pane/status redraw still happens when idle. A keystroke is handled and drawn with no intermediate wait; the submit path (pending_submit dispatch after the drain) is unchanged.
- **P2 HEADER BAR:** a top header row (1 line): `" Siralos "` left, `"provider / model"` right (the same composed profile source the status line uses; absent → `"no provider configured"`), styled reversed/accent; the transcript/input/status layout shrinks by 1 row. OFF-independent — the header always shows.
- **P3 ROLE COLORS:** transcript lines styled by role — user echo lines (`"> ..."`) in a distinct accent (Cyan) with the timestamp dim/gray below; assistant/system lines default; host notices (`unknown command`, `Approved.`/`Denied.`) in Yellow. Styles are part of the deterministic render model (TestBackend frames carry styles).
- **P4 ROUNDED BORDERS + PALETTE STYLING:** the context pane and the palette popup use `BorderType::Rounded`; the palette popup gets a bordered, title-styled popup (title `" commands "`) with the matching prefix highlighted; the approval modal keeps its backdrop (restyled rounded).
- **P5 CONTEXT-USAGE STATUS:** when the context subsystem is opted in AND built, the status line gains a usage readout: `"ctx <assembled>/<4096>"` (the assembled unique-digest total from the same `ContextMetrics` the pane uses — single source) appended to the composed status; absent → unchanged.
- **P6 RENDER RE-PIN:** the `tui-render` scenarios re-pin at corpus v78 (the header bar changes the layout rows; styles ride the TestBackend frames; the record builder renders the same 4 scenarios over 80×24 with the fixed fixture timestamps). Manifest `corpusVersion` 78, count 357 unchanged, digests refresh; the 4 expectation records updated in place via the throwaway OS-temp node `canonicalRecordDocument` script (delete after; diff surgical; `context-benchmark` BYTE-IDENTICAL).

## Red lines

The sanitizer is the single output boundary; the input queue the single read owner; approvals host-gated; no threads; no persistence; the stdio frontend byte-unchanged; OFF byte-transparency for the pane; decisions ≤116 untouched.

## Resolution

Open — entry review PASS per [decision 117](../decisions/117-tui-polish-entry-review.md) (HITL 2026-08-31, P1–P6 approved with the latency fix as priority one). Implementation tracked in [decision 118](../decisions/118-tui-polish-pass.md).
