---
title: "Mouse-Wheel Scrolling and the /mouse Toggle"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-11"
ticket: "125"
supersedes: []
---

# Mouse-Wheel Scrolling and the /mouse Toggle

Ticket [125](../tickets/125-mouse-wheel-toggle.md) · entry review
[155](155-mouse-wheel-toggle-entry-review.md) · [Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and committed
> (commit `8214729` "feat(tui): mouse-wheel scrolling with a /mouse capture
> toggle") BEFORE this record was written. There was no prior ticket and no
> prior entry review; the approval that exists is an in-chat design approval
> from the human owner on 2026-09-11 (option b) -- nothing more is claimed
> here, and nothing is backdated. The entry review ([155](155-mouse-wheel-toggle-entry-review.md))
> is itself retroactive and names the inversion.

## 2. The Implemented

| Fix                                    | The change                                                                                                                                                                                              | The evidence                                                                  |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| M1 CAPTURE AROUND THE ALTERNATE SCREEN | Capture is enabled after raw mode plus the alternate screen and released before leaving it; each entry-unwind path restores the state it had already changed, and the flag and escape stay in lockstep. | `TerminalGuard::enter` / `set_mouse_capture`, `crates/siralos-cli/src/tui.rs` |
| M2 WHEEL STEP ON THE EXISTING CLAMP    | `handle_mouse` maps `ScrollUp`/`ScrollDown` onto `MOUSE_WHEEL_STEP` and the existing `scroll_offset`/`max_scroll` clamping; scrolling cannot leave the transcript's range.                              | `tui::tests::mouse_wheel_scroll_clamps_at_both_bounds`                        |
| M3 TESTABLE WIRING                     | The live loop routes mouse events through one seam (`handle_tui_mouse`), following the precedent `handle_pending_approval_key` set for the modal crash, so the wiring itself is unit-covered.           | `interactive::tests::tui_mouse_wheel_seam_scrolls_transcript`                 |
| M4 REVERSIBLE CAPTURE                  | `/mouse` joins the shared catalog (both frontends) and flips capture, reporting the resulting state; in stdio, where there is no TTY mouse, it reports truthfully rather than pretending.               | `tui::tests::mouse_toggle_flips_capture_state_with_expected_message`          |
| M5 MODAL DISCIPLINE                    | Wheel events are ignored while a modal, picker or the add-form is open -- the same discipline that ignores non-modal keys -- pinned by a whole-state equality assertion.                                | `tui::tests::mouse_wheel_ignored_while_modal_or_form_open`                    |

## 3. The Engineering Story, Recorded

Two earlier implementation attempts left the helpers UNWIRED: `handle_mouse`
and `toggle_mouse_capture` existed with zero call sites, so the feature did
nothing while the runs reported success. The shipped version wires them and
pins the wiring with a test, which is why M3 is stated as a fix rather than an
implementation detail. An unused-import warning that would have failed
`clippy -D warnings` was also caught and removed before the commit.

## 4. Criteria -> Evidence

| Criterion                                    | Evidence                                                                                                                                                                |
| -------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The wheel scrolls, and stops at both ends    | M2 + M3 tests                                                                                                                                                           |
| Native selection is recoverable              | M4 test pins the toggle state and both messages                                                                                                                         |
| Modals and forms are unaffected              | M5 test asserts the whole state is unchanged                                                                                                                            |
| The change does not move unrelated behaviour | three of the four pinned render frames were unaffected; `npm run check` exit 0                                                                                          |
| The repository gate holds                    | `npm run check` exit 0 at `8214729` -- 316 adapters / 25 conformance / 247 cli / 614 core, 0 failed; differential parity 352/352 with 4 platform skips and 0 deviations |

## 5. Result

The wheel scrolls the transcript, `/mouse` makes the capability reversible, and
the wiring is covered by a test rather than assumed. What remains unverified is
the terminal itself -- the real escape sequences and the actual capture release
-- which no headless test can reach and which the ticket records as the owner's
to confirm.
