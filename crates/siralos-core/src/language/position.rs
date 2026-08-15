//! Generic source positions and ranges (Stage 3R R5).
//!
//! Siralos external positions are one-based: line and column both start
//! at 1. LSP line/character positions are 0-based and are converted to
//! this convention explicitly at the adapter boundary (never silently
//! mixed). Malformed external positions become typed invalid results,
//! never panics and never fabricated values.

/// A one-based source position (line and column start at 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LanguagePosition {
    /// One-based line.
    pub line: u64,
    /// One-based column.
    pub column: u64,
}

/// A source range under the reference ordering (start before end).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageRange {
    /// Range start (1-based).
    pub start: LanguagePosition,
    /// Range end (1-based).
    pub end: LanguagePosition,
}

/// A workspace-relative source location: path plus range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// Workspace-relative path with `/` separators.
    pub path: String,
    /// Source range within the file.
    pub range: LanguageRange,
}

/// Why an external position/range is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionError {
    /// A position is missing, not an integer, or negative.
    InvalidPosition,
    /// The range start precedes the range end (invalid ordering).
    UnorderedRange,
}

/// A raw external position before validation (0-based convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawPosition {
    /// Raw line value; None when not a non-negative integer.
    pub line: Option<i64>,
    /// Raw column value; None when not a non-negative integer.
    pub column: Option<i64>,
}

/// A raw external range before validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawRange {
    /// Raw range start.
    pub start: RawPosition,
    /// Raw range end.
    pub end: RawPosition,
}

/// Convert one raw 0-based position to the 1-based convention, or None
/// when malformed (missing, non-integer, or negative values).
pub fn to_one_based_position(raw: RawPosition) -> Option<LanguagePosition> {
    let line = raw.line?;
    let column = raw.column?;
    if line < 0 || column < 0 {
        return None;
    }
    Some(LanguagePosition {
        line: (line as u64) + 1,
        column: (column as u64) + 1,
    })
}

/// Convert one raw 0-based range to the 1-based convention, or None when
/// either position is malformed. The reference ordering (start before
/// end) is validated separately via [validate_range].
pub fn to_one_based_range(raw: RawRange) -> Option<LanguageRange> {
    Some(LanguageRange {
        start: to_one_based_position(raw.start)?,
        end: to_one_based_position(raw.end)?,
    })
}

/// Validate a range under the reference ordering (start <= end).
pub fn validate_range(
    range: LanguageRange,
) -> Result<LanguageRange, PositionError> {
    if range.start.line == 0 || range.start.column == 0 {
        return Err(PositionError::InvalidPosition);
    }
    if range.end.line == 0 || range.end.column == 0 {
        return Err(PositionError::InvalidPosition);
    }
    if range.start.line > range.end.line
        || (range.start.line == range.end.line
            && range.start.column > range.end.column)
    {
        return Err(PositionError::UnorderedRange);
    }
    Ok(range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_based_conversion_is_exact() {
        let range = to_one_based_range(RawRange {
            start: RawPosition { line: Some(32), column: Some(15) },
            end: RawPosition { line: Some(32), column: Some(26) },
        })
        .expect("valid");
        assert_eq!(range.start, LanguagePosition { line: 33, column: 16 });
        assert_eq!(range.end, LanguagePosition { line: 33, column: 27 });
    }

    #[test]
    fn malformed_positions_are_rejected_not_panicked() {
        assert_eq!(
            to_one_based_position(RawPosition { line: None, column: Some(1) }),
            None
        );
        assert_eq!(
            to_one_based_position(RawPosition {
                line: Some(-1),
                column: Some(1)
            }),
            None
        );
        // Column 0 is a valid 0-based column (converts to 1); a
        // negative coordinate is malformed.
        assert_eq!(
            to_one_based_range(RawRange {
                start: RawPosition { line: Some(1), column: Some(-1) },
                end: RawPosition { line: Some(1), column: Some(1) },
            }),
            None
        );
    }

    #[test]
    fn range_ordering_is_enforced() {
        let reversed = LanguageRange {
            start: LanguagePosition { line: 5, column: 1 },
            end: LanguagePosition { line: 4, column: 1 },
        };
        assert_eq!(
            validate_range(reversed),
            Err(PositionError::UnorderedRange)
        );
        let same_line_reversed = LanguageRange {
            start: LanguagePosition { line: 4, column: 9 },
            end: LanguagePosition { line: 4, column: 2 },
        };
        assert_eq!(
            validate_range(same_line_reversed),
            Err(PositionError::UnorderedRange)
        );
        let ordered = LanguageRange {
            start: LanguagePosition { line: 4, column: 2 },
            end: LanguagePosition { line: 4, column: 9 },
        };
        assert!(validate_range(ordered).is_ok());
    }
}
