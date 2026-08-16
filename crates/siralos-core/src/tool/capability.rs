//! Domain-neutral validated Tool-loop capability identifier.
//!
//! The R6 `siralos-core::domain::capability` vocabulary remains the
//! Domain host-boundary vocabulary. The Tool Loop uses its own opaque
//! identifier boundary so optional-domain tool capabilities can enter
//! through the same generic path without teaching Core any domain
//! semantics. A future identifier such as `future.domain` (including a
//! real optional-domain identifier) is opaque data to this type; it is
//! never a semantic enum variant.

use std::fmt;

/// Maximum length of one Tool-loop capability identifier in ASCII bytes.
///
/// §13 fixes the grammar as bounded but leaves the precise number to be
/// derived from the reference. The reference capability identifiers are
/// short (`workspace.read` is the longest ordinary identifier), tool
/// names are bounded at 256 UTF-8 bytes, and the R6 capability boundary
/// already uses 64 bytes. 64 is the smallest existing repository bound
/// that accepts every reference identifier with headroom.
pub const MAX_CAPABILITY_ID_BYTES: usize = 64;

/// Why a Tool-loop capability identifier was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityIdError {
    /// The identifier is empty.
    Empty,
    /// The identifier exceeds [`MAX_CAPABILITY_ID_BYTES`].
    TooLong,
    /// The identifier is not lowercase ASCII letters/digits joined by
    /// `.`, `_`, or `-` separators without leading, trailing, or
    /// repeated separators.
    InvalidSyntax,
}

impl fmt::Display for CapabilityIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("capability id is empty"),
            Self::TooLong => formatter.write_str(
                "capability id exceeds the 64-byte bound",
            ),
            Self::InvalidSyntax => formatter.write_str(
                "capability id must be lowercase ASCII letters/digits joined by '.', '_', or '-' separators",
            ),
        }
    }
}

impl std::error::Error for CapabilityIdError {}

fn valid_identifier(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_CAPABILITY_ID_BYTES {
        return false;
    }
    let mut previous_separator = false;
    for (index, byte) in value.bytes().enumerate() {
        let separator = matches!(byte, b'.' | b'_' | b'-');
        let alphanumeric = byte.is_ascii_lowercase() || byte.is_ascii_digit();
        if !alphanumeric && !separator {
            return false;
        }
        if separator && (index == 0 || previous_separator) {
            return false;
        }
        previous_separator = separator;
    }
    !previous_separator
}

/// A small validated opaque Tool-loop capability identifier.
///
/// Core may compare, order, hash, and evaluate policy over identifiers.
/// Core never interprets what an identifier means: `future.domain` is
/// exactly as generic as `workspace.read`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityId(String);

impl CapabilityId {
    /// Parse and validate one canonical capability identifier.
    pub fn parse(value: &str) -> Result<Self, CapabilityIdError> {
        if value.is_empty() {
            return Err(CapabilityIdError::Empty);
        }
        if value.len() > MAX_CAPABILITY_ID_BYTES {
            return Err(CapabilityIdError::TooLong);
        }
        if !valid_identifier(value) {
            return Err(CapabilityIdError::InvalidSyntax);
        }
        Ok(Self(value.to_owned()))
    }

    /// The canonical identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{CapabilityId, CapabilityIdError, MAX_CAPABILITY_ID_BYTES};

    #[test]
    fn accepts_generic_lowercase_identifiers() {
        for value in [
            "a",
            "z9",
            "workspace.read",
            "workspace_read",
            "workspace-read",
            "future.domain",
            "a.b_c-d",
        ] {
            let id = CapabilityId::parse(value).expect(value);
            assert_eq!(id.as_str(), value);
        }
    }

    #[test]
    fn rejects_empty_overlong_and_invalid_syntax() {
        assert_eq!(
            CapabilityId::parse("").unwrap_err(),
            CapabilityIdError::Empty
        );
        assert_eq!(
            CapabilityId::parse(&"a".repeat(MAX_CAPABILITY_ID_BYTES + 1))
                .unwrap_err(),
            CapabilityIdError::TooLong
        );
        for value in [
            "Workspace.Read",
            "workspace read",
            "workspace.read!",
            ".workspace",
            "workspace.",
            "workspace..read",
            "workspace.-read",
            "_workspace",
            "workspace_",
            "é",
        ] {
            assert_eq!(
                CapabilityId::parse(value).unwrap_err(),
                CapabilityIdError::InvalidSyntax,
                "{value}"
            );
        }
    }

    #[test]
    fn ordering_is_deterministic_and_total() {
        let a = CapabilityId::parse("a.b").unwrap();
        let b = CapabilityId::parse("a.c").unwrap();
        let c = CapabilityId::parse("a.b").unwrap();
        assert!(a < b);
        assert_eq!(a, c);
    }
}
