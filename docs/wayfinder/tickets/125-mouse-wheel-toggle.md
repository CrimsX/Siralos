---
title: "Mouse-Wheel Scrolling and the /mouse Toggle"
label: "wayfinder:ticket"
status: closed
date: "2026-09-11"
supersedes: []
---

# Mouse-Wheel Scrolling and the /mouse Toggle

The CLI crate had no mouse handling at all -- no `EnableMouseCapture`, no
`MouseEvent` -- so the terminal never delivered wheel events and transcript
history could only be scrolled with `PageUp`/`PageDown`. The owner reported
exactly that ("no way to scroll up history to view (mousewheel)") and, after
the tradeoff was put to them, chose a wheel **with** a toggle that hands the
mouse back to the terminal (option b): enabling capture stops the terminal's
native click-drag selection, so the capability has to be reversible.

Delivered: capture is enabled after raw mode plus the alternate screen and
released before leaving it, with the state flag and the terminal escape kept in
lockstep and every entry-unwind path restoring what it had already changed;
wheel events scroll by `MOUSE_WHEEL_STEP` through the existing
`scroll_offset`/`max_scroll` clamping rather than a second scroll mechanism; a
live-loop seam (`handle_tui_mouse`) routes them, following the precedent the
modal crash fix set with `handle_pending_approval_key`, so the WIRING is
covered by a test and not only the helper; `/mouse` (shared catalog, both
frontends) toggles capture, with stdio reporting truthfully instead of
pretending; and wheel events are ignored while a modal, picker or the add-form
is open -- the same discipline that ignores non-modal keys, pinned by a test
asserting the whole `TuiState` is unchanged.

> **Retroactive record.** This change was implemented, verified, and committed
> (commit `8214729` "feat(tui): mouse-wheel scrolling with a /mouse capture
> toggle") BEFORE this ticket and its entry review were written. There was no
> prior ticket and no prior entry review; the provenance is an owner feature
> request plus an in-chat design approval of option (b) on 2026-09-11 --
> nothing more is claimed here, and nothing is backdated. The entry review
> ([155](../decisions/155-mouse-wheel-toggle-entry-review.md)) is itself
> retroactive and names the inversion; the implementation record is
> [156](../decisions/156-mouse-wheel-toggle.md).

## What could not be verified here

The real terminal escape sequences and the actual capture release on a live
terminal are not covered by any test. The seam, the clamp, the toggle state and
the modal discipline are unit-proven; the terminal behaviour is the owner's to
confirm at a terminal.
