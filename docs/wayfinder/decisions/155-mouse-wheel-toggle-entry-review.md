---
title: "Mouse-Wheel Scrolling and the /mouse Toggle Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-11"
ticket: "125"
supersedes: []
---

# Mouse-Wheel Scrolling and the /mouse Toggle Entry Review

Ticket [125](../tickets/125-mouse-wheel-toggle.md) · entry review
[the Siralos TUI entry review](103-siralos-tui-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record -- the inversion, stated plainly.** This entry review was
> written AFTER the change was implemented, verified, and committed (commit
> `8214729`, 2026-09-11). There was no prior ticket and no prior entry review
> for this change; the approval that exists is an in-chat design approval from
> the human owner on 2026-09-11, who chose option (b) -- wheel scrolling with a
> toggle that returns the mouse to the terminal -- after being told the
> tradeoff. Nothing more is claimed here, and nothing is backdated. The verdict
> below is a retroactive PASS over the committed diff, not a pre-commit
> authorization.

> **User-directed 2026-09-11 (retroactive).** The transcript could not be
> scrolled with the wheel and the crate had no mouse handling whatsoever, so
> the terminal never delivered the events. The delivery: capture paired around
> the alternate screen, a wheel step on the existing clamp, a testable seam for
> the wiring, and `/mouse` as the reversible escape hatch.

## 2. The Fixes

| Fix                            | The change                                                                                                                                                                                                  | The evidence                                                                          |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| F1 CAPTURE PAIRED              | Capture is enabled after raw mode plus the alternate screen and released before leaving it; every entry-unwind path restores what it already changed, and the state flag and terminal escape move together. | `TerminalGuard::enter`/`set_mouse_capture` in `crates/siralos-cli/src/tui.rs`         |
| F2 WHEEL ON THE EXISTING CLAMP | `handle_mouse` routes `ScrollUp`/`ScrollDown` through `MOUSE_WHEEL_STEP` onto the existing `scroll_offset`/`max_scroll` clamping -- no second scroll mechanism.                                             | `tui::tests::mouse_wheel_scroll_clamps_at_both_bounds`                                |
| F3 TESTABLE WIRING             | The live loop calls one seam (`handle_tui_mouse`) rather than the helper directly, so the WIRING is covered by a test -- the gap that let two earlier attempts ship a feature with zero call sites.         | `interactive::tests::tui_mouse_wheel_seam_scrolls_transcript`                         |
| F4 REVERSIBLE CAPTURE          | `/mouse` (one catalog entry, both frontends) flips capture and reports the resulting state; stdio prints a truthful line instead of pretending.                                                             | `tui::tests::mouse_toggle_flips_capture_state_with_expected_message`                  |
| F5 MODAL DISCIPLINE            | Wheel events are ignored while a modal, picker or the add-form is open -- the same discipline that ignores non-modal keys.                                                                                  | `tui::tests::mouse_wheel_ignored_while_modal_or_form_open` (full `TuiState` equality) |

## 3. Criteria -> Evidence

| Criterion                                | Evidence                                                                                                              |
| ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| The wheel scrolls the transcript         | F2 + F3 tests; the seam moves `scroll_offset` and the loop consumes it                                                |
| Scrolling cannot run past either end     | F2 test clamps at the top and never goes below zero                                                                   |
| The user can get native selection back   | F4 test pins the toggle state and both messages                                                                       |
| A modal cannot be disturbed by the mouse | F5 test asserts the whole state is unchanged                                                                          |
| The repo gate stays green                | `npm run check` exit 0 at `8214729` (316 adapters / 25 conformance / 247 cli / 614 core; differential parity 352/352) |

## 4. Result

**PASS (retroactive).** Shape, error handling and the existing seams are
consistent with the TUI arc; the reversible-capture requirement the owner
attached to the approval is the part that makes the feature safe to ship.
The terminal-level behaviour remains unverified by any test and is recorded as
such in the ticket.
