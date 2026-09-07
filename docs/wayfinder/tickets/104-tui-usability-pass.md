---
title: "The TUI Usability Pass"
label: "wayfinder:ticket"
status: open
date: 2026-08-31
supersedes: []
---

# Ticket 104 — The TUI Usability Pass

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Authorized by:** [decision 110](../decisions/110-tui-usability-entry-review.md)

## Question

The user-reported usability pass over the decision 103-109 TUI — the shell, approval modals, context pane, and corpus-pinned render model as delivered — surfaces real usage friction that the entry review root-caused to two defects and six discoverability/completeness gaps.

## Fixes U1–U8 (authorized by decision 110)

- U1 INPUT LATENCY (defect): the loop drains the event queue (poll with a short timeout until empty) and draws ONCE per drained batch — keystrokes display immediately; the bounded idle poll remains for the ready-redraw. The submit-during-drain dispatch still freezes the redraw for its duration (the documented T1 limitation, unchanged).
- U2 COMMAND PALETTE: typing / opens a palette popup above the input line listing the command catalog — derived from ONE new catalog fn over the SAME SlashCommand vocabulary (no parallel list) — filtered live by the current input; display-only (Enter still submits the typed line); bounded height.
- U3 UNKNOWN-COMMAND HONESTY (defect): an unknown /-command in the TUI produces an explicit line "unknown command - available: ..." listing the catalog instead of falling through to the prompt path. TUI-only; the stdio fall-through is untouched.
- U4 TIMESTAMPS: transcript entries carry caller-supplied timestamps rendered as a dim line below each message; live sessions stamp with UTC HH:MM:SS computed std-only (civil-from-days math, no new dependency, labeled UTC); differential fixtures carry fixed values so the pinned frames stay deterministic. The tui-render record builder and scenarios re-pin at corpus v77.
- U5 STATUS PROVIDER/MODEL: the status line displays the applied profile's provider / model (e.g. example-vendor / model-a) or "no provider configured" honestly; sourced from the same composed profile the session already holds.
- U6 MODES: NOT APPLICABLE — Siralos has no mode system (approval is per-tool-call, profiles are startup config); shift+tab is recorded as unused; no change.
- U7 /provider AND /model: two new display-only SlashCommand variants in the SHARED vocabulary — /provider prints the applied provider (and credential availability), /model prints the model; both read the same composed profile source as U5; they land in the shared parse + both dispatchers (an additive improvement for stdio too — the worker updates any stdio test that pinned the old fall-through for these inputs, recording it); runtime switching is OUT OF SCOPE.
- U8 /evolve DISCOVERY: a new display-only /evolve variant listing the four Stage 6 bounded evolution surfaces (corpus, workflow, proposal, packaging — crates/siralos-core/src/evolution.rs, decisions 58-59) and stating that execution is host-gated (escalation Profile->Host per the Stage 6 design); it lands in the shared vocabulary + catalog; RUNNING an evolution from the command is OUT OF SCOPE.

## Resolution

Open — entry review PASS per [decision 110](../decisions/110-tui-usability-entry-review.md) (HITL 2026-08-31, U1–U8 approved, two defects root-caused). Implementation tracked in [decision 111](../decisions/111-tui-usability-pass.md).
