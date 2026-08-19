//! Terminal sanitizer — single output boundary for the interactive session.
//!
//! Ports the TypeScript `TerminalSanitizer` from
//! `apps/cli/src/output/sanitize.ts`. Every byte that reaches the
//! terminal passes through this sanitizer: C0/C1 controls are neutralized
//! via caret or \u{FFFD}, ANSI CSI and OSC (including OSC 8 links, title
//! changes, clipboard writes) are swallowed, and CR/BS rewriting is
//! neutralized. Ordinary Unicode and readable newlines survive.
//!
//! `Host-computed` presentation strings from `output::format_*` (counts,
//! fingerprints) are **not** sanitized — they are typed Host values, not
//! provider output. Only `TextDelta`, `ResponseFailed`, and
//! `ToolFailed` messages (untrusted provider/tool data) are sanitized in
//! `interactive::drain_events`.

/// One final terminal-rendering boundary.
///
/// Untrusted provider/tool messages are pushed through this sanitizer before
/// reaching `Write`. The sanitizer is stateful across pushes so a CSI/OSC
/// sequence split across successive `TextDelta` events is still swallowed;
/// `flush` drops any dangling escape so truncation never leaves the terminal
/// inside an active escape sequence.
///
/// Rust strings are valid UTF-8, so the TypeScript high/low surrogate
/// pairing across stream chunks is not required — Rust `char` is a Unicode
/// scalar value, not a UTF-16 code unit. The documented
/// `pendingHighSurrogate` state from the TypeScript oracle has no Rust
/// counterpart; unpaired surrogates cannot exist in `&str`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

/// Stateful terminal sanitizer (port of `TerminalSanitizer`).
#[derive(Debug, Clone)]
pub struct TerminalSanitizer {
    mode: Mode,
}

impl TerminalSanitizer {
    /// Create a fresh sanitizer in the initial state.
    pub fn new() -> Self {
        Self { mode: Mode::Normal }
    }

    /// Sanitize `text` and return the safe output.
    ///
    /// Internal mode is retained so a sequence split across two `push` calls
    /// is handled correctly. Call `flush` after the final push.
    pub fn push(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            let code = ch as u32;
            match self.mode {
                Mode::Normal => {
                    if ch == '\u{1b}' {
                        self.mode = Mode::Escape;
                    } else if ch == '\n' || ch == '\t' {
                        out.push(ch);
                    } else if code <= 0x1f {
                        out.push('^');
                        let caret = char::from_u32(code + 0x40)
                            .expect("caret base is ASCII");
                        out.push(caret);
                    } else if code == 0x7f {
                        out.push_str("^?");
                    } else if (0x80..=0x9f).contains(&code) {
                        out.push('\u{fffd}');
                    } else {
                        out.push(ch);
                    }
                }
                Mode::Escape => {
                    if ch == '[' {
                        self.mode = Mode::Csi;
                    } else if ch == ']' {
                        self.mode = Mode::Osc;
                    } else {
                        self.mode = Mode::Normal;
                    }
                }
                Mode::Csi => {
                    if ('\u{40}'..='\u{7e}').contains(&ch) {
                        self.mode = Mode::Normal;
                    }
                }
                Mode::Osc => {
                    if ch == '\u{07}' {
                        self.mode = Mode::Normal;
                    } else if ch == '\u{1b}' {
                        self.mode = Mode::OscEscape;
                    }
                }
                Mode::OscEscape => {
                    if ch == '\\' {
                        self.mode = Mode::Normal;
                    } else {
                        self.mode = Mode::Osc;
                    }
                }
            }
        }
        out
    }

    /// Drop any dangling escape state at end-of-stream.
    ///
    /// The TypeScript oracle also drains a pending high surrogate here; Rust
    /// strings cannot have one, so this is a no-op besides resetting mode.
    pub fn flush(&mut self) -> String {
        self.mode = Mode::Normal;
        String::new()
    }
}

impl Default for TerminalSanitizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Sanitize `text` as a single call (convenience over `push` + `flush`).
pub fn sanitize_for_display(text: &str) -> String {
    let mut s = TerminalSanitizer::new();
    let mut out = s.push(text);
    out.push_str(&s.flush());
    out
}

/// Sanitize a path-like single-line field (ported from `sanitizePathForDisplay`).
pub fn sanitize_path_for_display(path: Option<&str>) -> String {
    let Some(path) = path else {
        return "(none)".to_owned();
    };
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        let code = ch as u32;
        if ch == '\\' {
            out.push_str("\\\\");
        } else if ch == '\n' {
            out.push_str("\\n");
        } else if ch == '\r' {
            out.push_str("\\r");
        } else if ch == '\t' {
            out.push_str("\\t");
        } else if code < 0x20 {
            out.push('^');
            let caret =
                char::from_u32(code + 0x40).expect("caret base is ASCII");
            out.push(caret);
        } else if code == 0x7f {
            out.push_str("^?");
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{TerminalSanitizer, sanitize_for_display};
    #[test]
    fn strips_csi_sequences() {
        assert_eq!(sanitize_for_display("a\u{1b}[31mb\u{1b}[0mc"), "abc");
        assert_eq!(sanitize_for_display("\u{1b}[2Jx"), "x");
        assert_eq!(sanitize_for_display("\u{1b}[1;2mx"), "x");
    }
    #[test]
    fn strips_osc_sequences() {
        assert_eq!(
            sanitize_for_display("a\u{1b}]8;;https://example.com\u{07}b"),
            "ab"
        );
        assert_eq!(sanitize_for_display("a\u{1b}]0;title\u{1b}\\b"), "ab");
    }
    #[test]
    fn caret_notation_for_c0_and_del() {
        assert_eq!(sanitize_for_display("\u{00}"), "^@");
        assert_eq!(sanitize_for_display("\u{01}"), "^A");
        assert_eq!(sanitize_for_display("\u{08}"), "^H");
        assert_eq!(sanitize_for_display("\u{0d}"), "^M");
        assert_eq!(sanitize_for_display("\u{7f}"), "^?");
    }
    #[test]
    fn preserves_tab_and_lf() {
        assert_eq!(sanitize_for_display("a\tb\nc"), "a\tb\nc");
    }
    #[test]
    fn replaces_c1_controls() {
        assert_eq!(sanitize_for_display("\u{80}"), "\u{fffd}");
        assert_eq!(sanitize_for_display("\u{9f}"), "\u{fffd}");
        assert_eq!(sanitize_for_display("a\u{85}b"), "a\u{fffd}b");
    }
    #[test]
    fn lone_escape_is_dropped() {
        assert_eq!(sanitize_for_display("\u{1b}"), "");
        assert_eq!(sanitize_for_display("\u{1b}["), "");
        assert_eq!(sanitize_for_display("a\u{1b}"), "a");
    }
    #[test]
    fn csi_split_across_pushes_is_still_stripped() {
        let mut s = TerminalSanitizer::new();
        let a = s.push("\u{1b}[");
        let b = s.push("31mhello");
        let c = s.flush();
        assert_eq!(format!("{a}{b}{c}"), "hello");
    }
    #[test]
    fn osc_split_across_pushes_is_still_stripped() {
        let mut s = TerminalSanitizer::new();
        let a = s.push("\u{1b}]8;;https://e");
        let b = s.push("xample.com\u{07}ok");
        let c = s.flush();
        assert_eq!(format!("{a}{b}{c}"), "ok");
    }
    #[test]
    fn preserves_unicode_and_emoji() {
        assert_eq!(
            sanitize_for_display("caf\u{e9} \u{1f600}\n"),
            "caf\u{e9} \u{1f600}\n"
        );
    }
    #[test]
    fn host_vocab_passes_unchanged() {
        let vocab = "Context projection (mode generic)\n  Stable: 3 B (fingerprint abcdef12)\n";
        assert_eq!(sanitize_for_display(vocab), vocab);
    }
}
