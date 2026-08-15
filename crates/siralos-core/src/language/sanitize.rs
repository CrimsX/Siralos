//! Generic control-character sanitization for untrusted language-tool
//! output (Stage 3R R5).
//!
//! Language-server/compiler/parser output is untrusted data. Terminal
//! escape sequences (CSI) are stripped and remaining control characters
//! are replaced with U+FFFD before any model- or terminal-facing text is
//! produced. Tabs, newlines, and carriage returns are preserved. The
//! semantics are byte-identical to the reference
//! `sanitizeControlCharacters` (C0/DEL/C1 classes and the CSI grammar
//! ESC [ [0-9;?]* [ -/]* [@-~]).
//!
//! Sanitized text is data, never trusted markup: it is never executed or
//! rendered as UI.

/// True when the code point is a C0 control character (excluding tab,
/// LF, CR), DEL, or a C1 control character. The check is code-point
/// based: C1 controls (U+0080-U+009F) are multi-byte sequences in UTF-8
/// and must never be matched at the byte level.
fn is_control_code_point(character: char) -> bool {
    matches!(
        character as u32,
        0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f..=0x9f
    )
}

/// Replace terminal escape sequences and control characters with safe
/// text: CSI sequences (ESC [ params intermediates final) are removed,
/// and every other C0 control character, DEL, and C1 control character
/// becomes U+FFFD. The CSI grammar is ASCII-only, so the escape scan
/// operates on bytes without ever consuming UTF-8 continuation bytes.
pub fn sanitize_control_characters(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        // Try a complete CSI sequence first (ESC '[' params intermed
        // final); only a fully matched sequence is stripped.
        if bytes[index] == 0x1b
            && index + 1 < bytes.len()
            && bytes[index + 1] == b'['
        {
            let mut end = index + 2;
            while end < bytes.len()
                && matches!(bytes[end], b'0'..=b'9' | b';' | b'?')
            {
                end += 1;
            }
            while end < bytes.len() && (0x20..=0x2f).contains(&bytes[end]) {
                end += 1;
            }
            if end < bytes.len() && (0x40..=0x7e).contains(&bytes[end]) {
                index = end + 1;
                continue;
            }
        }
        let character = text[index..]
            .chars()
            .next()
            .expect("sanitize input is always valid UTF-8");
        if is_control_code_point(character) {
            out.push('\u{fffd}');
        } else {
            out.push(character);
        }
        index += character.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_sequences() {
        assert_eq!(
            sanitize_control_characters("a\u{1b}[31mb\u{1b}[0mc"),
            "abc"
        );
        assert_eq!(sanitize_control_characters("\u{1b}[2Jx"), "x");
        assert_eq!(sanitize_control_characters("\u{1b}[1;2mx"), "x");
    }

    #[test]
    fn replaces_remaining_controls() {
        assert_eq!(sanitize_control_characters("a\u{0}b"), "a\u{fffd}b");
        assert_eq!(sanitize_control_characters("\u{7f}"), "\u{fffd}");
        assert_eq!(sanitize_control_characters("\u{1b}x"), "\u{fffd}x");
    }

    #[test]
    fn preserves_tab_lf_cr_and_unicode() {
        assert_eq!(sanitize_control_characters("a\tb\nc\rd"), "a\tb\nc\rd");
        assert_eq!(
            sanitize_control_characters("caf\u{e9} \u{1f600}"),
            "caf\u{e9} \u{1f600}"
        );
    }

    #[test]
    fn replaces_c1_controls_as_code_points() {
        // U+009B is a two-byte UTF-8 sequence; it must be replaced at the
        // code-point level, never matched at the byte level.
        assert_eq!(
            sanitize_control_characters(
                "controls \u{0}\u{1}\u{7f}\u{9b} kept"
            ),
            "controls \u{fffd}\u{fffd}\u{fffd}\u{fffd} kept"
        );
    }

    #[test]
    fn incomplete_csi_is_not_stripped() {
        // ESC '[' without a final byte: ESC is a control character.
        assert_eq!(sanitize_control_characters("\u{1b}[1;2"), "\u{fffd}[1;2");
    }
}
