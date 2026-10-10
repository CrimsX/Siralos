//! Terminal ownership: the sanitized [`TuiSink`] transcript sink,
//! [`TerminalGuard`], and the TTY/launch heuristics.

use super::{
    MAX_STREAM_BYTES, REDRAW_INTERVAL, Rc, RefCell, TuiState, Write, io,
    paint_interval,
};

/// Pure predicate for the launch decision (decision 105, testable without a real TTY).
///
/// Returns true when the TUI should be used: stdout is a TTY and `--stdio`
/// was not requested. `wants_stdio` is true when the user passed `--stdio`.
///
/// Truth table:
/// - `!wants_stdio && is_tty` => TUI
/// - otherwise => stdio (scripts / CI silent, no diagnostic)
pub fn should_launch_tui(wants_stdio: bool, is_tty: bool) -> bool {
    !wants_stdio && is_tty
}

/// Deprecated alias for the T1 `--tui` opt-in (kept for compiling existing
/// tests; new code should use [`should_launch_tui`]).
pub fn should_use_tui(wants_tui: bool, is_tty: bool) -> bool {
    should_launch_tui(!wants_tui, is_tty)
}

/// Returns whether stdout is a TTY on this platform.
pub fn stdout_is_tty() -> bool {
    // Use crossterm's TTY detection via is_tty crate indirectly: crossterm
    // itself does not expose is_tty, so we use std's is_terminal on Unix and
    // Windows via the `IsTerminal` trait (stable since 1.70).
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// Sink that appends sanitized lines verbatim to a shared [`TuiState`].
///
/// The session's existing sanitizer boundary produces sanitized lines upstream;
/// this sink adds nothing unsanitized — it splits on newlines and pushes each
/// segment verbatim via [`TuiState::push_line`].
pub struct TuiSink {
    state: Rc<RefCell<TuiState>>,
    /// The live loop's redraw hook (S2 chunk 4): a streamed delta lands in
    /// the transcript and the sink asks for a frame, which is what makes
    /// streaming VISIBLE instead of arriving in one frame at the end.
    /// `None` in headless tests.
    redraw: Option<Rc<dyn Fn()>>,
    /// Last time the hook ran, for throttling a fast stream.
    last_redraw: std::cell::Cell<Option<std::time::Instant>>,
    /// Defense-in-depth terminal sanitizer for callers that write directly
    /// through the sink rather than through the session's relay.
    sanitizer: crate::sanitize::TerminalSanitizer,
    pending_utf8: Vec<u8>,
}

impl TuiSink {
    /// Create a sink sharing `state`.
    pub fn new(state: Rc<RefCell<TuiState>>) -> Self {
        Self {
            state,
            redraw: None,
            last_redraw: std::cell::Cell::new(None),
            sanitizer: crate::sanitize::TerminalSanitizer::new(),
            pending_utf8: Vec::new(),
        }
    }

    /// Install the live loop's redraw hook.
    pub fn set_redraw(&mut self, redraw: Rc<dyn Fn()>) {
        self.redraw = Some(redraw);
    }

    /// Ask for a frame, at most once per [`REDRAW_INTERVAL`].
    fn redraw_due(&self) {
        let Some(hook) = self.redraw.as_ref() else {
            return;
        };
        let now = std::time::Instant::now();
        // While the reader is owed text the frame cadence IS the character
        // cadence and nothing throttles it (see `paint_interval`); otherwise
        // the ordinary interval.
        let interval = paint_interval(
            self.state.borrow().reveal_pending(),
            REDRAW_INTERVAL,
        );
        let due = match self.last_redraw.get() {
            None => true,
            Some(last) => now.duration_since(last) >= interval,
        };
        if due {
            self.last_redraw.set(Some(now));
            hook();
        }
    }
}

impl Write for TuiSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // Bound the caller buffer BEFORE UTF-8 decoding. Provider output is
        // untrusted and a direct sink caller must not make the terminal
        // allocate an unbounded replacement string.
        let take = bytes.len().min(MAX_STREAM_BYTES);
        let mut combined = std::mem::take(&mut self.pending_utf8);
        combined.extend_from_slice(&bytes[..take]);
        let decoded = match std::str::from_utf8(&combined) {
            Ok(text) => {
                self.pending_utf8.clear();
                text.to_owned()
            }
            Err(error) if error.error_len().is_none() => {
                let valid = error.valid_up_to();
                self.pending_utf8 = combined[valid..].to_vec();
                String::from_utf8_lossy(&combined[..valid]).into_owned()
            }
            Err(_) => {
                self.pending_utf8.clear();
                String::from_utf8_lossy(&combined).into_owned()
            }
        };
        let text = self.sanitizer.push(&decoded);
        {
            let mut state = self.state.borrow_mut();
            // S3c: hand the text to the REVEAL rather than the transcript, so a
            // line the provider sends whole still appears a character at a
            // time, and an unfinished line is visible as it grows. The
            // CHARACTER is released by the paint (one per frame), never here:
            // this only buffers and asks for the frame that will release it.
            if take < bytes.len() {
                state.status =
                    "response truncated at the TUI stream bound".to_owned();
            }
            let remaining =
                MAX_STREAM_BYTES.saturating_sub(state.stream_buffer.len());
            if remaining == 0 && !text.is_empty() {
                state.status =
                    "response truncated at the TUI stream bound".to_owned();
                drop(state);
                self.redraw_due();
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "TUI stream byte bound reached",
                ));
            }
            if text.len() <= remaining {
                state.stream_buffer.push_str(&text);
            } else {
                let accepted = siralos_core::language::truncate_utf8_bytes(
                    &text, remaining,
                );
                state.stream_buffer.push_str(&accepted);
                state.status =
                    "response truncated at the TUI stream bound".to_owned();
                drop(state);
                self.redraw_due();
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "TUI stream byte bound reached",
                ));
            }
        }
        self.redraw_due();
        Ok(take)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Do not emit a dangling escape fragment, but do reset the
        // stateful parser so the next turn cannot inherit an unfinished
        // escape/OSC sequence. Likewise, discard an incomplete UTF-8 scalar
        // at the turn boundary rather than joining it to the next turn.
        self.pending_utf8.clear();
        let _ = self.sanitizer.flush();
        Ok(())
    }
}

/// RAII guard that restores terminal state (raw mode off, alternate screen
/// exit, MOUSE CAPTURE OFF) on drop — panic-safe.
///
/// PAIRING GUARANTEE (required behaviour 1): `enter` enables
/// `EnableMouseCapture` is deliberately NOT sent at entry -- capture is OFF
/// by default so native select, copy and paste work (owner ruling) -- and `drop`
/// disables it (`DisableMouseCapture`) BEFORE leaving the alternate screen
/// and raw mode — the exact reverse order. The guard is held as `_guard`
/// for the whole TUI session in
/// `interactive::run_interactive_tui_with_options`, so EVERY exit path —
/// normal `/exit`, Ctrl+C break, `?` early return, and panics — runs
/// `drop`. The only gap is a hard abort (`process::abort`, `SIGKILL`):
/// no userspace guard can run there, same as raw mode itself.
///
/// The guard additionally exposes `set_mouse_capture` so the live loop can
/// re-pair the terminal escape immediately on every `/mouse` toggle: the
/// state flip (`toggle_mouse_capture`) and the escape stay in lockstep in
/// the same match arm — never one without the other.
pub struct TerminalGuard {
    restored: bool,
    mouse_captured: bool,
}

impl TerminalGuard {
    /// Enter alternate screen and raw mode (in that order). The mouse is NOT
    /// captured here: the terminal keeps it, so click-drag selects text and
    /// its own paste works (owner ruling 2026-09-12). `/mouse` captures it
    /// live through [`Self::set_mouse_capture`]. Returns the guard; dropping
    /// it restores state.
    pub fn enter() -> io::Result<Self> {
        use crossterm::execute;
        use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(e) = execute!(stdout, EnterAlternateScreen) {
            let _ = crossterm::terminal::disable_raw_mode();
            return Err(e);
        }
        Ok(Self { restored: false, mouse_captured: false })
    }

    /// Re-pair the terminal escape with the toggled state: enabling sends
    /// `EnableMouseCapture`, disabling sends `DisableMouseCapture`. Called
    /// in the same `/mouse` arm as the `TuiState` flip — the two never
    /// diverge. A failed escape is reported to the loop as `Err` (the state
    /// flip is rolled back by the caller so state and terminal stay paired).
    pub fn set_mouse_capture(&mut self, enabled: bool) -> io::Result<()> {
        use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
        if enabled == self.mouse_captured {
            return Ok(());
        }
        if enabled {
            crossterm::execute!(io::stdout(), EnableMouseCapture)?;
        } else {
            crossterm::execute!(io::stdout(), DisableMouseCapture)?;
        }
        self.mouse_captured = enabled;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        use crossterm::event::DisableMouseCapture;
        if self.restored {
            return;
        }
        // Reverse of `enter`: mouse capture off FIRST, so no exit path —
        // not even a panic unwind — leaves the terminal captured.
        if self.mouse_captured {
            let _ = crossterm::execute!(io::stdout(), DisableMouseCapture);
            self.mouse_captured = false;
        }
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            io::stdout(),
            crossterm::terminal::LeaveAlternateScreen
        );
        self.restored = true;
    }
}
