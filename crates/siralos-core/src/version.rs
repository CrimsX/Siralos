//! Strict three-part numeric version identity.
//!
//! This type is the domain-neutral representation of the Siralos product
//! version. The CLI parses its own package version through [`Version`] so
//! version identity is validated once at the process boundary and is
//! never stringly handled afterwards.

use core::fmt;
use core::str::FromStr;

/// Maximum accepted value of a `major` or `minor` component.
///
/// Siralos is a private harness whose product versioning will never
/// approach this bound; the parse fails closed on overflow rather than
/// silently truncating a component.
const MAX_SMALL_COMPONENT: u64 = u8::MAX as u64;

/// Maximum accepted value of a `patch` component.
const MAX_PATCH_COMPONENT: u64 = u16::MAX as u64;

/// A strict `major.minor.patch` version with numeric components only.
///
/// # Invariants
///
/// - Components are the parsed decimal values of the input; leading zeros
///   are accepted on parse but not retained (display is canonical).
/// - Component values are bounded: `major` and `minor` fit in `u8`,
///   `patch` fits in `u16`.
/// - [`Ord`] is numeric, so `1.10.0 > 1.9.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    major: u8,
    minor: u8,
    patch: u16,
}

impl Version {
    /// Build a version from validated components.
    pub const fn new(major: u8, minor: u8, patch: u16) -> Self {
        Self { major, minor, patch }
    }

    /// Parse a strict `major.minor.patch` version string.
    ///
    /// Components must be ASCII decimal digits; separators are exactly
    /// one `.` between components and no leading or trailing text.
    ///
    /// # Errors
    ///
    /// Returns [`VersionParseError`] when the input is not exactly three
    /// dot-separated decimal components, a component is empty or
    /// non-numeric, or a component exceeds its bound.
    pub fn parse(input: &str) -> Result<Self, VersionParseError> {
        input.parse()
    }

    /// The `major` component.
    pub const fn major(self) -> u8 {
        self.major
    }

    /// The `minor` component.
    pub const fn minor(self) -> u8 {
        self.minor
    }

    /// The `patch` component.
    pub const fn patch(self) -> u16 {
        self.patch
    }
}

impl FromStr for Version {
    type Err = VersionParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut components = input.split('.');
        let major =
            parse_component(components.next(), 0, MAX_SMALL_COMPONENT)?;
        let minor =
            parse_component(components.next(), 1, MAX_SMALL_COMPONENT)?;
        let patch =
            parse_component(components.next(), 2, MAX_PATCH_COMPONENT)?;
        if components.next().is_some() {
            return Err(VersionParseError::new(format!(
                "expected exactly three dot-separated components in `{input}`"
            )));
        }
        // Lossless: parse_component rejects any value above the target
        // field's bound before these casts.
        Ok(Self::new(major as u8, minor as u8, patch as u16))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Parse one numeric component at `index`.
///
/// Accepts ASCII digits only; a leading `+` or `-` is rejected because
/// [`u64::from_str`] would otherwise accept `+1` and overflow-wrap
/// negative values, and a component is never silently clamped.
fn parse_component(
    text: Option<&str>,
    index: usize,
    max: u64,
) -> Result<u64, VersionParseError> {
    let Some(text) = text else {
        return Err(VersionParseError::new(format!(
            "missing component {index}: expected three dot-separated components"
        )));
    };
    if text.is_empty() {
        return Err(VersionParseError::new(format!(
            "component {index} is empty"
        )));
    }
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(VersionParseError::new(format!(
            "component {index} `{text}` is not a decimal number"
        )));
    }
    let value = text.parse::<u64>().map_err(|_| {
        VersionParseError::new(format!("component {index} `{text}` overflows"))
    })?;
    if value > max {
        return Err(VersionParseError::new(format!(
            "component {index} `{text}` exceeds the maximum of {max}"
        )));
    }
    Ok(value)
}

/// Failure to parse a [`Version`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionParseError {
    detail: String,
}

impl VersionParseError {
    /// A human-readable explanation of the parse failure.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    fn new(detail: impl Into<String>) -> Self {
        Self { detail: detail.into() }
    }
}

impl fmt::Display for VersionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for VersionParseError {}

#[cfg(test)]
mod tests {
    use super::Version;
    use core::str::FromStr;

    use proptest::prelude::*;

    // Canonical `major.minor.patch` strings within the component bounds
    // must parse and round-trip through Display unchanged.
    proptest! {
        #[test]
        fn parses_and_round_trips_canonical_versions(
            major in 0u8..=u8::MAX,
            minor in 0u8..=u8::MAX,
            patch in 0u16..=u16::MAX,
        ) {
            let text = format!("{major}.{minor}.{patch}");
            let version = Version::parse(&text).expect("canonical version parses");
            prop_assert_eq!(version.to_string(), text);
            prop_assert_eq!(version, Version::new(major, minor, patch));
        }
    }

    // Ordering is numeric: for any three in-bounds components,
    // a smaller component value never yields a larger version.
    proptest! {
        #[test]
        fn ordering_is_numeric_and_total(
            a_major in 0u8..=u8::MAX, a_minor in 0u8..=u8::MAX, a_patch in 0u16..=u16::MAX,
            b_major in 0u8..=u8::MAX, b_minor in 0u8..=u8::MAX, b_patch in 0u16..=u16::MAX,
        ) {
            let a = Version::new(a_major, a_minor, a_patch);
            let b = Version::new(b_major, b_minor, b_patch);
            let lexicographic = (a_major, a_minor, a_patch).cmp(&(b_major, b_minor, b_patch));
            prop_assert_eq!(a.cmp(&b), lexicographic);
        }
    }

    // Non-canonical spellings that contain only digits and dots may
    // parse, but their Display is always the canonical form; parsing
    // never panics on any ASCII input.
    proptest! {
        #[test]
        fn never_panics_and_canonicalizes(
            text in proptest::string::string_regex("[0-9.]{0,24}").unwrap(),
        ) {
            let parsed = Version::parse(&text);
            if let Ok(version) = parsed {
                prop_assert_eq!(version.to_string().parse::<Version>().expect("canonical"), version);
            }
        }
    }

    #[test]
    fn parses_canonical_versions() {
        let version = Version::parse("0.0.0").expect("valid version");
        assert_eq!(version, Version::new(0, 0, 0));
        assert_eq!(version.to_string(), "0.0.0");

        let version = Version::parse("1.10.42").expect("valid version");
        assert_eq!(version.major(), 1);
        assert_eq!(version.minor(), 10);
        assert_eq!(version.patch(), 42);
        assert_eq!(version.to_string(), "1.10.42");
    }

    #[test]
    fn accepts_leading_zeros_and_canonicalizes_display() {
        let version =
            Version::parse("01.02.003").expect("leading zeros are valid");
        assert_eq!(version, Version::new(1, 2, 3));
        assert_eq!(version.to_string(), "1.2.3");
    }

    #[test]
    fn orders_numerically() {
        assert!(
            Version::parse("1.10.0").expect("valid")
                > Version::parse("1.9.0").expect("valid")
        );
        assert!(
            Version::parse("2.0.0").expect("valid")
                > Version::parse("1.99.99").expect("valid")
        );
        assert_eq!(
            Version::parse("0.0.1").expect("valid"),
            Version::parse("0.0.1").expect("valid")
        );
    }

    #[test]
    fn rejects_wrong_component_counts() {
        for input in ["", "1", "1.2", "1.2.3.4", ".1.2", "1..2", "1.2."] {
            assert!(
                Version::parse(input).is_err(),
                "`{input}` must be rejected"
            );
        }
    }

    #[test]
    fn rejects_non_numeric_and_signed_components() {
        for input in ["a.b.c", "1.a.3", "+1.2.3", "-1.2.3", "1.2.3 ", " 1.2.3"]
        {
            assert!(
                Version::parse(input).is_err(),
                "`{input}` must be rejected"
            );
        }
    }

    #[test]
    fn rejects_components_beyond_their_bounds() {
        assert!(Version::parse("256.0.0").is_err(), "major overflow");
        assert!(Version::parse("0.256.0").is_err(), "minor overflow");
        assert!(Version::parse("0.0.65536").is_err(), "patch overflow");
        assert!(
            Version::parse("0.0.65535").is_ok(),
            "patch bound is inclusive"
        );
    }

    #[test]
    fn parse_error_reports_a_useful_detail() {
        let error = Version::parse("1.2").expect_err("must fail");
        assert!(!error.detail().is_empty());
        assert_eq!(error.to_string(), error.detail());
    }

    #[test]
    fn parses_through_from_str() {
        assert_eq!(
            Version::from_str("3.2.1").expect("valid version"),
            Version::new(3, 2, 1)
        );
    }
}
