---
title: "TUI Default Entry"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "103"
supersedes: []
---

# 105 — TUI Default Entry

Ticket [103](../tickets/103-siralos-tui.md) · entry review [103](103-siralos-tui-entry-review.md) · Map.

> **User-directed 2026-08-31 (session HITL).** The TUI becomes the default frontend when stdout is a TTY — the user's amendment to the decision 103 entry semantics; --stdio forces the stdio frontend, plain non-TTY uses stdio silently, and the one-commit --tui flag is removed.

## 2. Amendment (A1–A3) — as implemented

| ID  | Amendment                                                                                                                                                                                                                                                                                                                                                                                                                              | Evidence / note                                                                                                                                      |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| A1  | DEFAULT: when stdout is a TTY and no flag overrides, the session launches the TUI by default. The pure predicate is `should_launch_tui(wants_stdio, is_tty) -> !wants_stdio && is_tty` in `tui.rs`; `main.rs` calls it with `wants_stdio = false` on the no-flag path and enters `run_interactive_tui_stdio` on true.                                                                                                                  | `tui.rs` `should_launch_tui`; `main.rs` `Command::Interactive` branch; `tui_default_entry_truth_table`                                               |
| A2  | ESCAPE HATCH: a new `--stdio` flag forces the stdio frontend regardless of TTY (`parse_args` → `Command::Stdio` → `run_interactive_stdio`). The `--tui` flag is REMOVED (it lived for one commit; unreleased — `parse_args` now rejects it as unknown). Plain non-TTY with no flags uses stdio SILENTLY: the `main.rs` non-TTY branch prints no diagnostic (the diagnostic existed only for the removed `--tui`-without-TTY mismatch). | `lib.rs` `Command::Stdio` + `stdio_flag_forces_the_stdio_frontend`; `removed_tui_flag_is_rejected`; `non_tty_silent_stdio`; `--help` lists `--stdio` |
| A3  | EVERYTHING ELSE UNCHANGED: the stdio path is byte-unchanged for scripts/CI (`run_interactive_session*` untouched — all pre-existing `interactive::tests` pass untouched); the TUI shell is unchanged in T1 scope (`TuiState`, `draw`, `TuiSink`, `handle_key`, `TerminalGuard` untouched).                                                                                                                                             | Full `interactive::tests` suite green untouched; `tui.rs` T1 `TestBackend` tests green untouched                                                     |

## 3. Criteria → evidence

| Criterion                                | Evidence                                                                                                                                                                                                                              | Status |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Default TTY launches the TUI             | `tui_default_entry_truth_table`: `should_launch_tui(false, true)` is true; the other three cells false — pure predicate tested headlessly without a real TTY                                                                          | pass   |
| `--stdio` forces stdio even on TTY       | `stdio_escape_hatch_forces_stdio` (predicate level) + `stdio_flag_forces_the_stdio_frontend` (`parse_args(["--stdio"]) == Command::Stdio`); `main.rs` routes `Command::Stdio` straight to `run_interactive_stdio` without a TTY check | pass   |
| `--tui` removed                          | `removed_tui_flag_is_rejected`: `parse_args(["--tui"])` is an unknown-argument error; no `--tui` string remains in `main.rs`/`lib.rs`                                                                                                 | pass   |
| Plain non-TTY uses stdio silently        | `non_tty_silent_stdio` + `main.rs` non-TTY branch calls `run_interactive_stdio` with no diagnostic print                                                                                                                              | pass   |
| Stdio path byte-unchanged for scripts/CI | `run_interactive_session` / `run_interactive_session_with_options` untouched; every pre-existing `interactive::tests` case passes unmodified                                                                                          | pass   |
| TUI shell unchanged in T1 scope          | `draw`, `TuiState`, `TuiSink`, `handle_key`, `TerminalGuard` untouched; all T1 `TestBackend` render tests pass unmodified                                                                                                             | pass   |
| New flag visible on the binary           | `siralos --help` usage lists `--stdio` with the default-on-TTY / silent-stdio-otherwise semantics                                                                                                                                     | pass   |

## 4. Result

The TUI is the default interactive frontend on TTYs; scripts and CI get stdio silently and unchanged; the escape hatch is --stdio.
