---
title: "The TUI Interaction Pass"
label: "wayfinder:ticket"
status: open
date: 2026-08-31
supersedes: []
---

# Ticket 109 — The TUI Interaction Pass

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`  
**Authorized by:** [decision 123](../decisions/123-tui-interaction-entry-review.md)

## Question

The user's third live-usage round from the TUI reports six issues: the palette still fails to retain while typing (root-caused as a wiring gap), missing arrow-key palette navigation, truncated command display, no command history, a right-edge layout gap, and the need for a `/models` fetch from the provider.

## Fixes I1–I6 (authorized by decision 123)

- **I1 PALETTE RETENTION (the real fix):** `handle_key` calls `state.update_palette()` after every mutation of `state.input` (Char appended, Backspace, Tab completion, any edit) so the live palette recomputes on every keystroke. The unit tests that tested `update_palette` directly remain; new tests verify `handle_key` itself updates the palette (typing `/` -> `/p` -> `/pr` each step has a non-empty filtered palette).
- **I2 ARROW-KEY PALETTE NAVIGATION:** `TuiState` gains `palette_selected: Option<usize>`; Up/Down move through the filtered palette entries (when the palette is `Some`); the selected entry renders highlighted (reversed style); Enter on a selected entry fills the input with the full command name (replacing the partial `/pr` with `/provider`) and clears the palette; Tab also completes to the selected entry. When the palette is `None` or the input does not start with `/`, Up/Down do command history (I4) instead.
- **I3 FULL COMMAND DISPLAY:** the palette shows all filtered entries (remove the at-most-8 bound; the palette popup grows to fit, bounded by the terminal height minus the input/status rows; if more entries than fit, a scroll indicator appears). When the input is exactly `/`, all commands display.
- **I4 COMMAND HISTORY:** `TuiState` gains `prompt_history: Vec<String>` (submitted prompts, oldest first, bounded to 100) and `history_index: Option<usize>`; pressing Up when the palette is `None` recalls the previous prompt into the input (moving backward through the stack); Down moves forward; reaching past the newest restores the pre-navigation input; the history is per-session (in-memory, no persistence). Enter appends to the history.
- **I5 RIGHT-EDGE GAP:** investigate the layout — the context pane is a fixed 40-column constraint; when the pane is off the transcript should span the full terminal width; when on, the pane + transcript should fill the full width with no gap (check the ratatui layout constraints in `draw_with_pane` and the off/on layout variants; fix any percentage/length constraint that leaves unfilled space).
- **I6 /models FETCH:** a new `/models` behavior: when the session has a configured provider (endpoint + credential), `/models` performs a blocking GET to the provider's `/models` endpoint (the OpenAI-compatible shape: GET `{endpoint}/models` with Bearer auth from the `HostCredential`) using the same bounded reqwest pattern as `GenericProvider`; parses the OpenAI models response shape `{data: [{id: "..."}]}`; displays the fetched model list as transcript lines (host-generated, sanitized); the freeze during the fetch is documented (the synchronous architecture constraint). If the provider returns an error or the shape is unrecognized, an honest error line is displayed. If no provider is configured, `/models` reports that. The `/models` fetch is a new network surface — it uses the same credential, endpoint, and recording-hygiene rules as the existing provider path (no credential values in output, bounded responses). This is implemented as a provider-surface fn (a models-list call alongside the existing completion call) — not a new tool; it is invoked from the `/models` command dispatch.

## Red lines

The sanitizer is the single output boundary; the input queue the single read owner; approvals host-gated; no threads; no persistence; the stdio frontend byte-unchanged; decisions ≤ 120 untouched (121/122 are landed and stay).

## Resolution

Open — entry review PASS per [decision 123](../decisions/123-tui-interaction-entry-review.md) (HITL 2026-08-31). Implementation tracked in [decision 124](../decisions/124-tui-interaction-pass.md).
