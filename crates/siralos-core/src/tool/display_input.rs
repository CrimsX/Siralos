//! `tool_started.displayInput` formatting with JavaScript UTF-16
//! semantics.
//!
//! The reference algorithm is:
//!
//! ```text
//! text = JSON.stringify(input)
//! if serialization is undefined: "<unprintable>"
//! else if text.length <= 200: text
//! else: text.slice(0, 200) + "..."
//! ```
//!
//! `String.length` counts UTF-16 code units and `slice(0, 200)` cuts at
//! UTF-16 code-unit boundaries. Rust `str` cannot represent a cut that
//! leaves a lone surrogate, so the formatted result is stored as
//! UTF-16 code units. Differential canonical records serialize those
//! units directly, which preserves the exact JavaScript string value
//! (including a lone surrogate) without inventing a second parser.
//!
//! The `<unprintable>` fallback is structurally unreachable for the
//! accepted R7.2 input path on both implementations: provider input is
//! a `serde_json::Value` and is always serializable. It is therefore
//! deliberately not represented as a state; the formatter would
//! overflow first (see [`truncated_utf16_units`]).

/// Maximum retained UTF-16 code units before the `...` suffix.
pub const MAX_DISPLAY_INPUT_LENGTH: usize = 200;

/// The reference fallback for a JSON serialization that returns
/// `undefined`. Structurally unreachable for the accepted typed input.
pub const UNPRINTABLE_DISPLAY_INPUT: &str = "<unprintable>";

/// A displayInput value as exact UTF-16 code units.
///
/// This is the portable representation of a JavaScript string value for
/// the differential boundary. For strings made only of valid Unicode
/// scalar values [`DisplayInput::to_string`] reconstructs ordinary
/// Rust text; a boundary that splits a surrogate pair stays observable
/// as the exact lone-surrogate code unit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DisplayInput {
    units: Vec<u16>,
}

impl DisplayInput {
    /// All UTF-16 code units of a Rust string (no truncation).
    pub fn from_rust_str(value: &str) -> Self {
        Self { units: value.encode_utf16().collect() }
    }

    /// The exact UTF-16 code units of the reference string value.
    pub fn units(&self) -> &[u16] {
        &self.units
    }

    /// Reconstruct ordinary Rust text when every code unit forms valid
    /// Unicode scalar values.
    ///
    /// Returns `None` for a value containing a lone surrogate (the
    /// split-pair boundary case). The code units remain authoritative
    /// in that case.
    pub fn to_string(&self) -> Option<String> {
        char::decode_utf16(self.units.iter().copied())
            .collect::<Result<String, _>>()
            .ok()
    }

    /// Whether this value contains a lone surrogate code unit.
    pub fn has_lone_surrogate(&self) -> bool {
        let mut units = self.units.iter().copied();
        while let Some(unit) = units.next() {
            if (0xD800..=0xDBFF).contains(&unit) {
                let Some(next) = units.next() else {
                    return true;
                };
                if !(0xDC00..=0xDFFF).contains(&next) {
                    return true;
                }
            } else if (0xDC00..=0xDFFF).contains(&unit) {
                return true;
            }
        }
        false
    }
}

/// Format one serialized JSON text exactly like the reference
/// `toDisplayInput`.
///
/// `serialized_json` is the already-serialized `JSON.stringify(input)`
/// text (source-ordered when the caller attached it). The returned
/// value retains the first 200 UTF-16 code units plus `...` when
/// longer; otherwise it retains the whole text.
pub fn to_display_input(serialized_json: &str) -> DisplayInput {
    DisplayInput { units: truncated_utf16_units(serialized_json) }
}

/// First `limit` UTF-16 code units, plus a `...` suffix when truncation
/// occurred. The suffix is appended exactly like the reference's
/// template literal, so the retained code units are `limit + 3` for
/// every truncated input.
fn truncated_utf16_units(value: &str) -> Vec<u16> {
    let mut units = Vec::new();
    for unit in value.encode_utf16() {
        if units.len() >= MAX_DISPLAY_INPUT_LENGTH {
            units.extend_from_slice(&[b'.' as u16; 3]);
            return units;
        }
        units.push(unit);
    }
    units
}

#[cfg(test)]
mod tests {
    use super::{
        DisplayInput, MAX_DISPLAY_INPUT_LENGTH, UNPRINTABLE_DISPLAY_INPUT,
        to_display_input,
    };

    fn string_units(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }

    #[test]
    fn keeps_short_input_unchanged() {
        let text = "{\"z\":1,\"a\":2}";
        let display = to_display_input(text);
        assert_eq!(display.units(), string_units(text));
        assert_eq!(display.to_string().as_deref(), Some(text));
    }

    #[test]
    fn truncates_at_exactly_200_utf16_code_units() {
        let text = "a".repeat(250);
        let display = to_display_input(&text);
        assert_eq!(display.units().len(), MAX_DISPLAY_INPUT_LENGTH + 3);
        assert_eq!(
            &display.units()[..MAX_DISPLAY_INPUT_LENGTH],
            &string_units(&"a".repeat(200))[..]
        );
        assert_eq!(
            &display.units()[MAX_DISPLAY_INPUT_LENGTH..],
            &[b'.' as u16; 3]
        );
    }

    #[test]
    fn boundary_of_200_has_no_suffix_and_201_does() {
        let at_limit = "b".repeat(200);
        assert_eq!(to_display_input(&at_limit).units().len(), 200);
        let over_limit = "b".repeat(201);
        assert_eq!(to_display_input(&over_limit).units().len(), 203);
    }

    #[test]
    fn supplementary_unicode_counts_two_units_per_scalar() {
        let text = "😀".repeat(50);
        let units = to_display_input(&text);
        assert_eq!(units.units().len(), 100);
        assert!(!units.has_lone_surrogate());
        assert_eq!(units.to_string().as_deref(), Some(text.as_str()));
    }

    #[test]
    fn a_split_surrogate_pair_keeps_the_lone_high_surrogate() {
        // 202 total units with a high surrogate landing exactly at the
        // 200-unit cut: 7 ASCII prefix units + 96 complete emoji pairs
        // (192 units) + the 97th emoji (2 units) + one ASCII suffix.
        let mut text = String::from("prefix-");
        for _ in 0..97 {
            text.push('😀');
        }
        text.push('x');
        assert_eq!(text.encode_utf16().count(), 202);
        let units = to_display_input(&text);
        assert_eq!(units.units().len(), 203);
        assert_eq!(units.units()[199], 0xD83D);
        assert!(units.has_lone_surrogate());
        assert!(units.to_string().is_none());
    }

    #[test]
    fn unprintable_fallback_is_a_documented_constant() {
        // Structurally unreachable through the typed input path; the
        // constant preserves the reference wording for completeness.
        assert_eq!(UNPRINTABLE_DISPLAY_INPUT, "<unprintable>");
        let value = DisplayInput::from_rust_str(UNPRINTABLE_DISPLAY_INPUT);
        assert_eq!(value.to_string().as_deref(), Some("<unprintable>"));
    }
}
