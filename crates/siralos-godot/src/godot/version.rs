//! Exact Godot version model.
//!
//! The raw version text is bounded and sanitized for control
//! characters by the probe adapter; the parsed model and its
//! classification are core-owned and provider-neutral. Unknown suffixes
//! are preserved rather than failing; prerelease statuses are never
//! normalized into stable.

/// Status of a Godot version string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GodotVersionStatus {
    /// Stable release.
    Stable,
    /// Release candidate (`rc`).
    Rc,
    /// Beta.
    Beta,
    /// Alpha.
    Alpha,
    /// Development build (`dev`).
    Dev,
    /// Custom build.
    Custom,
    /// Unknown status suffix.
    Unknown,
}

/// Exact Godot version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotVersion {
    /// Complete bounded raw version text, sanitized for control characters.
    pub raw: String,
    /// Major component.
    pub major: u64,
    /// Minor component.
    pub minor: u64,
    /// Patch component, if present.
    pub patch: Option<u64>,
    /// Prerelease or custom status.
    pub status: GodotVersionStatus,
    /// Prerelease sequence number, e.g. `1` for `rc1`.
    pub status_number: Option<u64>,
    /// Build token such as `official` or `custom_build`.
    pub build: Option<String>,
    /// Git commit hash token when present.
    pub commit: Option<String>,
}

/// Release channel derived from [`GodotVersionStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotReleaseChannel {
    /// Stable channel.
    Stable,
    /// Release-candidate channel.
    ReleaseCandidate,
    /// Beta channel.
    Beta,
    /// Alpha channel.
    Alpha,
    /// Development channel.
    Development,
    /// Custom channel.
    Custom,
    /// Unknown channel.
    Unknown,
}

/// Classify a [`GodotVersion`] into a [`GodotReleaseChannel`].
pub fn classify_godot_release_channel(
    version: &GodotVersion,
) -> GodotReleaseChannel {
    match version.status {
        GodotVersionStatus::Stable => GodotReleaseChannel::Stable,
        GodotVersionStatus::Rc => GodotReleaseChannel::ReleaseCandidate,
        GodotVersionStatus::Beta => GodotReleaseChannel::Beta,
        GodotVersionStatus::Alpha => GodotReleaseChannel::Alpha,
        GodotVersionStatus::Dev => GodotReleaseChannel::Development,
        GodotVersionStatus::Custom => GodotReleaseChannel::Custom,
        GodotVersionStatus::Unknown => GodotReleaseChannel::Unknown,
    }
}

/// Version declared by a project (from `config/features`).
///
/// Static and non-authoritative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDeclaredVersion {
    /// Major component.
    pub major: u64,
    /// Minor component.
    pub minor: u64,
    /// Patch component, if present.
    pub patch: Option<u64>,
    /// Raw declared feature token, e.g. `4.7`.
    pub raw: String,
}

/// Conservative static parse of a declared `major.minor[.patch]` feature token.
///
/// Returns `None` when the token is not exactly `major.minor` or
/// `major.minor.patch` with decimal components.
pub fn parse_declared_version(raw: &str) -> Option<GodotDeclaredVersion> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split('.');
    let major_raw = parts.next()?;
    let minor_raw = parts.next()?;
    let patch_raw = parts.next();
    if parts.next().is_some() {
        return None;
    }
    let major: u64 = major_raw.parse().ok()?;
    let minor: u64 = minor_raw.parse().ok()?;
    let patch = match patch_raw {
        None => None,
        Some(text) => Some(text.parse::<u64>().ok()?),
    };
    // Validate safe integer (u64 parse already guarantees it) and
    // non-empty decimal form (parse would fail on empty).
    if major_raw.is_empty() || minor_raw.is_empty() {
        return None;
    }
    if !major_raw.bytes().all(|b| b.is_ascii_digit())
        || !minor_raw.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    if let Some(text) = patch_raw
        && !text.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some(GodotDeclaredVersion { major, minor, patch, raw: trimmed.to_owned() })
}

#[cfg(test)]
mod tests {
    use super::{
        GodotReleaseChannel, GodotVersion, GodotVersionStatus,
        classify_godot_release_channel, parse_declared_version,
    };

    fn version_with_status(status: GodotVersionStatus) -> GodotVersion {
        GodotVersion {
            raw: "4.0".to_owned(),
            major: 4,
            minor: 0,
            patch: None,
            status,
            status_number: None,
            build: None,
            commit: None,
        }
    }

    #[test]
    fn release_channel_classification() {
        assert_eq!(
            classify_godot_release_channel(&version_with_status(
                GodotVersionStatus::Stable
            )),
            GodotReleaseChannel::Stable
        );
        assert_eq!(
            classify_godot_release_channel(&version_with_status(
                GodotVersionStatus::Rc
            )),
            GodotReleaseChannel::ReleaseCandidate
        );
        assert_eq!(
            classify_godot_release_channel(&version_with_status(
                GodotVersionStatus::Custom
            )),
            GodotReleaseChannel::Custom
        );
    }

    #[test]
    fn declared_version_parsing() {
        let parsed = parse_declared_version("4.7").expect("4.7");
        assert_eq!(parsed.major, 4);
        assert_eq!(parsed.minor, 7);
        assert_eq!(parsed.patch, None);
        assert_eq!(parse_declared_version("4.7.1").unwrap().patch, Some(1));
        assert!(parse_declared_version("bad").is_none());
        assert!(parse_declared_version("4").is_none());
        assert!(parse_declared_version("4.7.1.2").is_none());
        assert_eq!(parse_declared_version("  4.2  ").unwrap().raw, "4.2");
    }
}
