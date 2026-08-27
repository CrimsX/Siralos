//! Version-matched Godot API knowledge (R8).
//!
//! Mirrors `packages/core/src/godot/knowledge.ts`.
//! No filesystem, process, or cache operation — typed models only.

use super::version::GodotVersion;

/// Knowledge schema version (immutable).
pub const KNOWLEDGE_SCHEMA_VERSION: u32 = 1;

/// Version-matched API knowledge profile (schema version 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotKnowledgeProfileV1 {
    /// Schema version, always `1`.
    pub version: u32,
    /// Engine identity.
    pub engine: KnowledgeEngine,
    /// API dump identity + counts.
    pub api: KnowledgeApi,
    /// Index metadata.
    pub index: KnowledgeIndex,
}

/// Engine identity block of a knowledge profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEngine {
    /// Installation id.
    pub installation_id: String,
    /// SHA-256 of the verified executable (64 hex).
    pub executable_sha256: String,
    /// Exact engine version string.
    pub godot_version: String,
    /// Edition string.
    pub edition: String,
}

/// API dump block of a knowledge profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeApi {
    /// SHA-256 of the extension API dump.
    pub dump_sha256: String,
    /// ISO 8601 generation timestamp.
    pub generated_at: String,
    /// Native class count.
    pub class_count: usize,
    /// Built-in class count.
    pub builtin_class_count: usize,
    /// Utility function count.
    pub utility_function_count: usize,
    /// Global enum count.
    pub global_enum_count: usize,
    /// Global constant count.
    pub global_constant_count: usize,
}

/// Index metadata block of a knowledge profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeIndex {
    /// Schema version.
    pub schema_version: u32,
    /// Symbol count in the index.
    pub symbol_count: usize,
}

/// Cache invalidation reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnowledgeCacheReason {
    /// Executable fingerprint changed.
    ExecutableChanged,
    /// Dump hash changed.
    DumpChanged,
    /// Schema version changed.
    SchemaChanged,
}

/// Validation of a stored profile against the expected identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GodotKnowledgeCacheValidation {
    /// Profile is valid.
    Valid,
    /// Profile is invalid for the given reason.
    Invalid(KnowledgeCacheReason),
}

/// Validate a stored profile against the expected identities.
#[must_use]
pub fn validate_godot_knowledge_cache(
    profile: &GodotKnowledgeProfileV1,
    expected_executable_sha256: &str,
    expected_dump_sha256: &str,
    expected_schema_version: u32,
) -> GodotKnowledgeCacheValidation {
    if profile.engine.executable_sha256 != expected_executable_sha256 {
        return GodotKnowledgeCacheValidation::Invalid(
            KnowledgeCacheReason::ExecutableChanged,
        );
    }
    if profile.api.dump_sha256 != expected_dump_sha256 {
        return GodotKnowledgeCacheValidation::Invalid(
            KnowledgeCacheReason::DumpChanged,
        );
    }
    if profile.index.schema_version != expected_schema_version {
        return GodotKnowledgeCacheValidation::Invalid(
            KnowledgeCacheReason::SchemaChanged,
        );
    }
    GodotKnowledgeCacheValidation::Valid
}

/// Official-manual channel for an exact engine version.
///
/// Stable versions map to their exact `major.minor` channel; prerelease
/// and custom builds are `unverified`.
#[must_use]
pub fn classify_godot_manual_channel(version: &GodotVersion) -> String {
    if version.status == super::version::GodotVersionStatus::Stable {
        format!("{}.{}", version.major, version.minor)
    } else {
        "unverified".to_owned()
    }
}

/// Truthful platform support for API knowledge generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotKnowledgeSupport {
    /// `available` or `unavailable`.
    pub state: KnowledgeSupportState,
    /// Reason when `unavailable`, `None` when `available`.
    pub reason: Option<String>,
    /// Platform string (e.g. `linux`, `win32`, `darwin`).
    pub platform: String,
}

/// State of knowledge support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnowledgeSupportState {
    /// Available.
    Available,
    /// Unavailable.
    Unavailable,
}

/// Bounded in-memory knowledge status for CLI diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotKnowledgeStatus {
    /// State.
    pub state: KnowledgeState,
    /// Reason when not ready.
    pub reason: Option<String>,
    /// Platform.
    pub platform: String,
    /// Profile, if any.
    pub profile: Option<GodotKnowledgeProfileV1>,
    /// Cache is always `false` at this stage.
    pub cache_enabled: bool,
    /// Schema version.
    pub schema_version: u32,
    /// Manual channel, if any.
    pub manual_channel: Option<String>,
}

/// Knowledge state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnowledgeState {
    /// Ready.
    Ready,
    /// Unavailable.
    Unavailable,
    /// Unsupported.
    Unsupported,
}

/// A fully loaded knowledge base: profile plus built index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotKnowledgeBase {
    /// Version-matched profile metadata.
    pub profile: GodotKnowledgeProfileV1,
    /// Deterministic bounded symbol index.
    pub index: super::api::GodotApiIndex,
}

/// Outcome of a knowledge refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotKnowledgeRefreshResult {
    /// A new profile replaced any previous one.
    Ready {
        /// The newly loaded profile.
        profile: Box<GodotKnowledgeProfileV1>,
        /// The previously loaded profile, if any.
        previous_profile: Option<Box<GodotKnowledgeProfileV1>>,
    },
    /// Typed non-ready outcome with its bounded reason.
    NotReady {
        /// `unavailable`, `unsupported`, `failed`, or `cancelled`.
        status: KnowledgeRefreshStatus,
        /// Bounded truthful reason.
        message: String,
    },
}

/// Non-ready refresh statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnowledgeRefreshStatus {
    /// Generation cannot execute under the current boundary.
    Unavailable,
    /// The selected engine cannot generate documentation.
    Unsupported,
    /// Generation ran and failed.
    Failed,
    /// The operation was cancelled.
    Cancelled,
}

/// Outcome of a knowledge search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotKnowledgeQueryResult {
    /// Search served from the loaded base.
    Ready {
        /// Exact engine version string.
        engine_version: String,
        /// Ranked bounded results.
        results: Vec<super::api::GodotApiSearchResult>,
        /// True when results beyond the limit were dropped.
        truncated: bool,
    },
    /// Typed non-ready outcome with its bounded reason.
    NotReady {
        /// `unavailable`, `unsupported`, `invalid_input`, `failed`, or
        /// `cancelled`.
        status: KnowledgeQueryStatus,
        /// Bounded truthful reason.
        message: String,
    },
}

/// Non-ready query statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnowledgeQueryStatus {
    /// No base is loaded and generation is unavailable.
    Unavailable,
    /// The request was malformed.
    InvalidInput,
    /// The operation was cancelled.
    Cancelled,
}

/// Outcome of a knowledge lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotKnowledgeLookupOutcome {
    /// Lookup served from the loaded base.
    Ready {
        /// Exact engine version string.
        engine_version: String,
        /// Full structured symbol result.
        result: Box<super::api::GodotApiLookupResult>,
    },
    /// Typed non-ready outcome with its bounded reason.
    NotReady {
        /// `not_found`, `unavailable`, `unsupported`, `invalid_input`,
        /// `failed`, or `cancelled`.
        status: KnowledgeLookupStatus,
        /// Bounded truthful reason.
        message: String,
    },
}

/// Non-ready lookup statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnowledgeLookupStatus {
    /// The symbol identity is not in the loaded index.
    NotFound,
    /// No base is loaded and generation is unavailable.
    Unavailable,
    /// The request was malformed.
    InvalidInput,
    /// The operation was cancelled.
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::{
        GodotKnowledgeProfileV1, KnowledgeApi, KnowledgeEngine,
        KnowledgeIndex, classify_godot_manual_channel,
        validate_godot_knowledge_cache,
    };
    use crate::godot::{GodotVersion, GodotVersionStatus};

    fn profile() -> GodotKnowledgeProfileV1 {
        GodotKnowledgeProfileV1 {
            version: 1,
            engine: KnowledgeEngine {
                installation_id: "test".to_owned(),
                executable_sha256: "a".repeat(64),
                godot_version: "4.7".to_owned(),
                edition: "standard".to_owned(),
            },
            api: KnowledgeApi {
                dump_sha256: "b".repeat(64),
                generated_at: "2026-01-01T00:00:00Z".to_owned(),
                class_count: 10,
                builtin_class_count: 2,
                utility_function_count: 5,
                global_enum_count: 1,
                global_constant_count: 1,
            },
            index: KnowledgeIndex { schema_version: 1, symbol_count: 100 },
        }
    }

    #[test]
    fn cache_validation() {
        let p = profile();
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        assert!(matches!(
            validate_godot_knowledge_cache(&p, &a, &b, 1),
            super::GodotKnowledgeCacheValidation::Valid
        ));
        assert!(matches!(
            validate_godot_knowledge_cache(&p, &"c".repeat(64), &b, 1),
            super::GodotKnowledgeCacheValidation::Invalid(_)
        ));
    }

    #[test]
    fn manual_channel_stable_vs_unverified() {
        let stable = GodotVersion {
            raw: "4.7".to_owned(),
            major: 4,
            minor: 7,
            patch: None,
            status: GodotVersionStatus::Stable,
            status_number: None,
            build: None,
            commit: None,
        };
        assert_eq!(classify_godot_manual_channel(&stable), "4.7");
        let dev = GodotVersion {
            raw: "4.8-dev1".to_owned(),
            major: 4,
            minor: 8,
            patch: None,
            status: GodotVersionStatus::Dev,
            status_number: Some(1),
            build: None,
            commit: None,
        };
        assert_eq!(classify_godot_manual_channel(&dev), "unverified");
    }
}
