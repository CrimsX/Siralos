//! Fixed presentation limits, redraw timing, and the static host copy
//! (banner, greeting, mouse-capture messages) the TUI shows.

use super::Duration;

/// Zero-timeout drain poll — only already-queued events, NEVER waits (P1).
pub const TUI_DRAIN_POLL: Duration = Duration::ZERO;

/// Bounded idle poll for status/pane redraw when idle (P1 — raised 15ms → 50ms).
pub const TUI_IDLE_POLL: Duration = Duration::from_millis(50);

/// Context budget for the status usage readout (P5).
pub const CONTEXT_BUDGET_TOKENS: usize = 4096;

/// Maximum number of transcript lines retained (bounded ring).
pub const MAX_TRANSCRIPT_LINES: usize = 1000;
/// Maximum bytes retained in one transcript line (the ring is also bounded).
pub const MAX_TRANSCRIPT_LINE_BYTES: usize = 64 * 1024;

/// Maximum number of lines shown in the approval modal (bounded).
pub const MAX_APPROVAL_LINES: usize = 30;

/// Marker appended when the approval request is truncated to the bound.
pub const APPROVAL_TRUNCATION_MARKER: &str = "... (truncated)";

/// Transcript rows per mouse-wheel notch (option b): a small step reusing
/// the existing `scroll_offset` clamp/max logic — not a second mechanism.
/// PageUp/PageDown keep their 10-row step; the wheel is the fine control.
pub const MOUSE_WHEEL_STEP: u16 = 3;

/// Minimum gap between sink-requested redraws while a stream is arriving.
///
/// The reveal releases `rate * interval` characters per frame, so this IS
/// the text's step size: at 33 ms a 240 char/s reveal moved ~8 characters a
/// frame and read as "display, stop, display" on a slow provider. 16 ms
/// (60 fps) halves the step, and the last delta of a turn is always painted
/// because the loop's own draw runs after the drain.
pub const REDRAW_INTERVAL: Duration = Duration::from_millis(16);

/// How much streamed thinking the TUI keeps (the tail), so a long reasoning
/// trace cannot grow the state without bound.
pub const REASONING_BYTES: usize = 8192;

/// How many thinking rows the expanded block shows (the tail).
pub const REASONING_ROWS: usize = 8;

/// How much of the newest thinking line the COLLAPSED row previews.
pub const THINKING_TAIL_CHARS: usize = 60;
/// Maximum bytes accepted in the main prompt or provider-add form input.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
/// Maximum buffered answer bytes retained by the TUI sink.
pub const MAX_STREAM_BYTES: usize = 1024 * 1024;
/// Maximum unrevealed reasoning bytes retained before a visible bound.
pub const MAX_REASONING_BUFFER_BYTES: usize = 64 * 1024;

/// How long a painter waits before it paints the next frame.
///
/// While the reveal owes the reader text it waits NOTHING: the reveal renders a
/// character at a time (owner ruling), so matching the speed the model produces
/// means painting one frame per character as fast as frames can be painted.
/// The frame cost is then the only limiter -- measured at 2.2 ms in the
/// unoptimized build the dev flow runs (`cargo run`), which is a ceiling of
/// roughly 450 characters a second, several times higher than a reasoning
/// stream. Once nothing is owed, `idle` applies again, so a quiet UI does not
/// spin.
#[must_use]
pub const fn paint_interval(owed: bool, idle: Duration) -> Duration {
    if owed { Duration::ZERO } else { idle }
}

/// Toggle result line when mouse capture turns on: states the result and
/// the copy trade (capture steals click-drag selection) with the way back.
pub const MOUSE_CAPTURE_ON_MESSAGE: &str = "mouse capture on - the wheel scrolls the transcript directly; /mouse again hands the mouse back to the terminal";

/// Toggle result line when mouse capture turns off: states the result and
/// the way back to wheel scrolling.
pub const MOUSE_CAPTURE_OFF_MESSAGE: &str = "mouse capture off - the terminal selects text and pastes; the wheel scrolls via arrow keys; /mouse captures the mouse";

/// Honest stdio answer for `/mouse`: the stdio frontend has no TTY mouse,
/// so it reports that instead of pretending to toggle anything.
pub const MOUSE_STDIO_MESSAGE: &str = "mouse capture unavailable - the stdio frontend has no TTY mouse; use the TUI for wheel scrolling";

/// Map a capture state to its toggle result line (pure, headlessly tested).
#[must_use]
pub fn mouse_capture_message(enabled: bool) -> &'static str {
    if enabled { MOUSE_CAPTURE_ON_MESSAGE } else { MOUSE_CAPTURE_OFF_MESSAGE }
}

/// ASCII banner for Siralos — hand-drawn static block, bounded width <= 80 cols (H2).
/// TUI-only: pushed into the transcript at session start via `push_line` (stdio unchanged).
pub const SIRALOS_BANNER: &[&str] = &[
    "  ___ ___ ___  _   _    ___  ___",
    " / __|_ _| _ \\/ \\ | |  / _ \\/ __|",
    " \\__ \\| ||   / _ \\| |_| (_) \\__ \\",
    " |___/___|_|_/_/ \\_\\___\\___/|___/",
];

/// Greeting line shown below the banner at TUI session start (H2).
pub const SIRALOS_GREETING: &str =
    "Welcome to Siralos. Type / to browse commands, or just start typing.";
