//! Generic UTF-8 byte truncation and UTF-16 prefix helpers (Stage 3R R5).
//!
//! `truncate_utf8_bytes` never splits a code point and is byte-identical
//! to the reference `truncateUtf8Bytes`. The UTF-16 helpers mirror the
//! reference summary formatter's UTF-16-unit slicing with TextEncoder
//! encoding (a prefix ending inside an astral character contributes
//! U+FFFD = 3 bytes, exactly like a lone surrogate).

/// UTF-16 code unit length of a string (BMP characters count 1, astral
/// characters count 2), matching JavaScript `.length` for valid text.
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// UTF-8 byte length of the UTF-16-unit prefix of `text`, encoding a
/// prefix that ends inside an astral character with U+FFFD (3 bytes),
/// exactly like TextEncoder encodes a lone surrogate.
pub fn utf16_prefix_byte_len(text: &str, units: usize) -> usize {
    let mut used = 0;
    let mut bytes = 0;
    for character in text.chars() {
        let count = character.len_utf16();
        if used + count > units {
            if used + 1 == units {
                bytes += 3;
            }
            break;
        }
        used += count;
        bytes += character.len_utf8();
    }
    bytes
}

/// The UTF-16-unit prefix of `text` as a Rust string; a prefix ending
/// inside an astral character is represented with U+FFFD (matching
/// TextEncoder's lone-surrogate encoding).
pub fn utf16_prefix_lossy(text: &str, units: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for character in text.chars() {
        let count = character.len_utf16();
        if used + count > units {
            if used + 1 == units {
                out.push('\u{fffd}');
            }
            break;
        }
        used += count;
        out.push(character);
    }
    out
}

/// Truncate UTF-8 text to an exact byte bound without splitting a code
/// point.
pub fn truncate_utf8_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut result = String::new();
    let mut bytes = 0;
    for character in text.chars() {
        let size = character.len_utf8();
        if bytes + size > max_bytes {
            break;
        }
        result.push(character);
        bytes += size;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_never_splits_a_code_point() {
        assert_eq!(truncate_utf8_bytes("héllo", 4), "hél");
        assert_eq!(truncate_utf8_bytes("😀😀😀", 5), "😀");
        assert_eq!(truncate_utf8_bytes("short", 1024), "short");
        assert_eq!(truncate_utf8_bytes("any", 0), "");
    }

    #[test]
    fn utf16_prefix_matches_textencoder_semantics() {
        // "a😀b" has 4 UTF-16 units (a, high, low, b).
        assert_eq!(utf16_len("a😀b"), 4);
        // 3 units covers the complete astral char (units 1-2).
        assert_eq!(utf16_prefix_byte_len("a😀b", 3), 5);
        assert_eq!(utf16_prefix_lossy("a😀b", 3), "a😀");
        // 2 units ends between the surrogate halves: the prefix encodes
        // as 'a' + lone high surrogate = 'a' + U+FFFD (4 bytes).
        assert_eq!(utf16_prefix_byte_len("a😀b", 2), 4);
        assert_eq!(utf16_prefix_lossy("a😀b", 2), "a\u{fffd}");
        // 1 unit is just 'a'.
        assert_eq!(utf16_prefix_lossy("a😀b", 1), "a");
        // Full length.
        assert_eq!(utf16_prefix_lossy("a😀b", 4), "a😀b");
        // Non-astral text is exact (é is 2 UTF-8 bytes).
        assert_eq!(utf16_prefix_lossy("héllo", 3), "hél");
        assert_eq!(utf16_prefix_byte_len("héllo", 3), 4);
    }
}
