//! JavaScript default string comparison (UTF-16 code-unit order).
//!
//! One rule, owned once. It was written twice — `language::diagnostic`'s
//! `utf16_cmp` and `projection::segments`' `js_string_cmp` — as two copies of
//! the same walk, which is the shape that drifts: the surrogate rule below is
//! easy to get wrong and is silent when it is wrong.
//!
//! The module is a leaf: it takes no import from this crate — only
//! `std::cmp::Ordering` — so it adds no edge between `language` and
//! `projection`, which the architecture keeps independent.

use std::cmp::Ordering;

/// Compare two strings as JavaScript's default relational operators do, by
/// UTF-16 code unit.
///
/// The code units are walked lazily. Materializing a `Vec<u16>` per side made
/// every comparison allocate twice, which dominated the diagnostic sort at its
/// run-wide bound (`docs/development/performance-baseline.md`).
///
/// This is not byte order for astral text: a supplementary scalar's lead
/// surrogate sorts below a BMP scalar above U+E000. A proper prefix sorts
/// first, which is what `Iterator::cmp` returns.
pub(crate) fn cmp(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

#[cfg(test)]
mod tests {
    use super::cmp;
    use std::cmp::Ordering;

    #[test]
    fn utf16_ordering_matches_javascript_string_order() {
        // Astral characters sort BEFORE BMP characters in UTF-16 order
        // (high surrogate 0xD800 < 0xFFFF), but AFTER in byte order.
        let astral = "a\u{1f600}";
        let bmp_high = "a\u{ffff}";
        assert_eq!(cmp(astral, bmp_high), Ordering::Less);
        assert_eq!(cmp("abc", "abd"), Ordering::Less);
        assert_eq!(cmp("abc", "abc"), Ordering::Equal);
        assert_eq!(cmp("abc", "abcd"), Ordering::Less);
    }

    #[test]
    fn js_cmp_matches_rust_for_bmp() {
        assert_eq!(cmp("a", "b"), Ordering::Less);
        assert_eq!(cmp("b", "a"), Ordering::Greater);
        assert_eq!(cmp("a", "a"), Ordering::Equal);
    }

    #[test]
    fn js_cmp_for_supplementary_scalar() {
        // U+10400 (DESERET CAPITAL LETTER LONG I) is outside BMP; its UTF-16
        // encoding is a surrogate pair 0xD801 0xDC00. Ensure the comparison
        // handles supplementary scalars (no panic, deterministic).
        let a = "\u{10400}";
        let b = "\u{10401}";
        assert_eq!(cmp(a, b), Ordering::Less);
    }
}
