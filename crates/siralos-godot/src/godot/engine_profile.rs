//! Godot engine profile models and classification (R8 Godot Stage-2 parity).
//!
//! Mirrors `packages/core/src/godot/engine-profile.ts`.
//!
//! Conservative edition classification: `mono` in a filename is never proof
//! of .NET on its own; the user hint, filename, `--build-solutions`
//! advertisement, API dump features, and successful probes are combined.

use super::capabilities::GodotCommandCapabilities;
use super::installations::GodotInstallation;
use super::version::{GodotReleaseChannel, GodotVersion};

/// Conservative edition classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotEdition {
    /// Standard editor.
    Standard,
    /// .NET editor.
    Dotnet,
    /// Editor signal present but edition unknown.
    EditorUnknown,
    /// Heuristic runtime-only (no editor signal).
    RuntimeOnly,
    /// Could not be characterized.
    Unknown,
}

/// Confidence of an edition classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotEditionConfidence {
    /// High confidence.
    High,
    /// Medium confidence.
    Medium,
    /// Low confidence.
    Low,
}

/// Siralos's tested support classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SiralosGodotSupport {
    /// Exact verified baseline (4.7.1 stable standard editor).
    Verified,
    /// Compatible but untested.
    CompatibleUntested,
    /// Prerelease, untested.
    PrereleaseUntested,
    /// Custom build, untested.
    CustomBuildUntested,
    /// Unsupported major (Godot 3.x).
    UnsupportedMajor,
    /// Runtime-only binary.
    RuntimeOnly,
    /// Invalid edition.
    Invalid,
}

/// Structured evidence the adapter gathers before core classifies an edition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotEditionEvidence {
    /// Explicit user hint, if any.
    pub explicit_hint: Option<GodotEditionHint>,
    /// Canonical filename lowercased.
    pub filename: String,
    /// Capabilities advertised via `--help`.
    pub capabilities: GodotCommandCapabilities,
    /// Features reported by the API dump header, if available.
    pub api_configuration_features: Vec<String>,
    /// Which probes succeeded.
    pub probes_succeeded: GodotProbesSucceeded,
}

/// Which probes succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GodotProbesSucceeded {
    /// `--version` succeeded.
    pub version: bool,
    /// `--help` succeeded.
    pub help: bool,
    /// `--dump-extension-api` succeeded.
    pub api_dump: bool,
}

/// Edition hint supplied by the user (re-export for evidence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotEditionHint {
    /// Standard.
    Standard,
    /// Dotnet.
    Dotnet,
    /// Unknown (no hint).
    Unknown,
}

/// Result of conservative edition classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotEditionClassification {
    /// Classified edition.
    pub edition: GodotEdition,
    /// Confidence.
    pub confidence: GodotEditionConfidence,
    /// Bounded evidence descriptions.
    pub evidence: Vec<String>,
    /// Bounded conflicts.
    pub conflicts: Vec<String>,
}

/// Immutable engine profile produced for one valid installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotEngineProfile {
    /// Installation id this profile belongs to.
    pub installation_id: String,
    /// Short executable fingerprint (SHA-256 prefix, 8 hex chars).
    pub fingerprint: String,
    /// Exact version.
    pub version: GodotVersion,
    /// Classified edition.
    pub edition: GodotEdition,
    /// Edition confidence.
    pub edition_confidence: GodotEditionConfidence,
    /// Release channel.
    pub release_channel: GodotReleaseChannel,
    /// Advertised capabilities.
    pub capabilities: GodotCommandCapabilities,
    /// Operationally verified capabilities.
    pub verified_capabilities: Vec<String>,
    /// Advertised but degraded capabilities.
    pub degraded_capabilities: Vec<String>,
    /// SHA-256 of the executable bytes (64 hex).
    pub executable_sha256: String,
    /// SHA-256 of the API dump, if available.
    pub api_dump_sha256: Option<String>,
    /// Support classification.
    pub support: SiralosGodotSupport,
    /// Bounded diagnostics.
    pub diagnostics: Vec<String>,
}

/// Input for support classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GodotSupportClassificationInput<'a> {
    /// Version.
    pub version: &'a GodotVersion,
    /// Edition.
    pub edition: GodotEdition,
    /// Edition confidence (unused for the rule itself, kept for parity).
    pub edition_confidence: GodotEditionConfidence,
    /// True only for the exact verified baseline.
    pub is_verified_baseline: bool,
}

/// Classify support per the frozen rules (see engine-profile.ts).
#[must_use]
pub fn classify_godot_support(
    input: GodotSupportClassificationInput<'_>,
) -> SiralosGodotSupport {
    let version = input.version;
    let edition = input.edition;
    if edition == GodotEdition::RuntimeOnly {
        return SiralosGodotSupport::RuntimeOnly;
    }
    if version.major < 4 {
        return SiralosGodotSupport::UnsupportedMajor;
    }
    if edition == GodotEdition::Dotnet {
        return SiralosGodotSupport::CompatibleUntested;
    }
    if version.status == super::version::GodotVersionStatus::Custom {
        return SiralosGodotSupport::CustomBuildUntested;
    }
    if matches!(
        version.status,
        super::version::GodotVersionStatus::Dev
            | super::version::GodotVersionStatus::Rc
            | super::version::GodotVersionStatus::Beta
            | super::version::GodotVersionStatus::Alpha
    ) {
        return SiralosGodotSupport::PrereleaseUntested;
    }
    if version.status == super::version::GodotVersionStatus::Unknown {
        return SiralosGodotSupport::PrereleaseUntested;
    }
    if input.is_verified_baseline
        && version.status == super::version::GodotVersionStatus::Stable
    {
        return SiralosGodotSupport::Verified;
    }
    SiralosGodotSupport::CompatibleUntested
}

/// Whether the profile is a selection candidate (editor workflow).
#[must_use]
pub fn is_editor_selection_candidate(profile: &GodotEngineProfile) -> bool {
    matches!(
        profile.edition,
        GodotEdition::Standard
            | GodotEdition::Dotnet
            | GodotEdition::EditorUnknown
    )
}

/// Conservative edition classification (mirrors engine-profile.ts).
#[must_use]
pub fn classify_godot_edition(
    evidence: &GodotEditionEvidence,
) -> GodotEditionClassification {
    let mut conflicts: Vec<String> = Vec::new();
    if !evidence.probes_succeeded.version {
        return GodotEditionClassification {
            edition: GodotEdition::Unknown,
            confidence: GodotEditionConfidence::Low,
            evidence: vec!["the version probe did not succeed".to_owned()],
            conflicts,
        };
    }
    let mut dotnet_signals: Vec<String> = Vec::new();
    let mut standard_signals: Vec<String> = Vec::new();
    let filename_lower = evidence.filename.to_ascii_lowercase();
    if evidence.explicit_hint == Some(GodotEditionHint::Dotnet) {
        dotnet_signals.push("explicit user edition hint: dotnet".to_owned());
    }
    if evidence.explicit_hint == Some(GodotEditionHint::Standard) {
        standard_signals
            .push("explicit user edition hint: standard".to_owned());
    }
    if filename_lower.contains("mono") || filename_lower.contains("dotnet") {
        dotnet_signals
            .push("canonical filename contains a .NET marker".to_owned());
    }
    if evidence.capabilities.build_solutions {
        dotnet_signals.push("advertises --build-solutions".to_owned());
    }
    if evidence.api_configuration_features.iter().any(|f| {
        matches!(f.as_str(), "dotnet" | "mono" | "csharp" | "managed")
    }) {
        dotnet_signals
            .push("API dump configuration reports .NET features".to_owned());
    }
    if evidence.explicit_hint == Some(GodotEditionHint::Standard)
        && !dotnet_signals.is_empty()
    {
        conflicts.push(
            "the user hint says standard while other evidence suggests .NET"
                .to_owned(),
        );
    }
    if evidence.explicit_hint == Some(GodotEditionHint::Dotnet)
        && !filename_lower.contains("mono")
        && !filename_lower.contains("dotnet")
    {
        conflicts.push(
            "the user hint says dotnet while the filename shows no .NET marker".to_owned(),
        );
    }
    let editor_signals = evidence.capabilities.editor
        || evidence.capabilities.project_manager
        || evidence.capabilities.extension_api_dump;
    if !evidence.probes_succeeded.help {
        return GodotEditionClassification {
            edition: GodotEdition::Unknown,
            confidence: GodotEditionConfidence::Low,
            evidence: vec![
                "the help probe did not succeed, so the binary cannot be characterized"
                    .to_owned(),
            ],
            conflicts,
        };
    }
    if !dotnet_signals.is_empty() {
        let confident = standard_signals.is_empty()
            && (dotnet_signals.len() >= 2
                || evidence.explicit_hint == Some(GodotEditionHint::Dotnet));
        return GodotEditionClassification {
            edition: GodotEdition::Dotnet,
            confidence: if confident {
                GodotEditionConfidence::High
            } else {
                GodotEditionConfidence::Medium
            },
            evidence: dotnet_signals,
            conflicts,
        };
    }
    if !editor_signals {
        if !standard_signals.is_empty() {
            conflicts.push(
                "the user hint says standard, but the help output advertises no editor signal"
                    .to_owned(),
            );
            return GodotEditionClassification {
                edition: GodotEdition::Standard,
                confidence: GodotEditionConfidence::Medium,
                evidence: standard_signals,
                conflicts,
            };
        }
        return GodotEditionClassification {
            edition: GodotEdition::RuntimeOnly,
            confidence: GodotEditionConfidence::Medium,
            evidence: vec![
                "the help probe succeeded but no editor signal is advertised (heuristic runtime-only inference)"
                    .to_owned(),
            ],
            conflicts,
        };
    }
    if !standard_signals.is_empty() {
        return GodotEditionClassification {
            edition: GodotEdition::Standard,
            confidence: if evidence.probes_succeeded.api_dump {
                GodotEditionConfidence::High
            } else {
                GodotEditionConfidence::Medium
            },
            evidence: standard_signals,
            conflicts,
        };
    }
    GodotEditionClassification {
        edition: GodotEdition::EditorUnknown,
        confidence: GodotEditionConfidence::Medium,
        evidence: vec![
            "editor signals are advertised, but no positive standard or .NET evidence exists"
                .to_owned(),
        ],
        conflicts,
    }
}

/// Describe provenance of an installation.
#[must_use]
pub fn describe_installation_provenance(
    installation: &GodotInstallation,
) -> String {
    format!("{} ({})", installation.source_label, installation.id)
}

#[cfg(test)]
mod tests {
    use super::{
        GodotEdition, GodotEditionConfidence, GodotEditionEvidence,
        GodotProbesSucceeded, GodotSupportClassificationInput,
        SiralosGodotSupport, classify_godot_edition, classify_godot_support,
        is_editor_selection_candidate,
    };
    use crate::godot::capabilities::empty_godot_command_capabilities;
    use crate::godot::version::{GodotVersion, GodotVersionStatus};

    fn version(status: GodotVersionStatus, major: u64) -> GodotVersion {
        GodotVersion {
            raw: format!("{major}.0"),
            major,
            minor: 0,
            patch: None,
            status,
            status_number: None,
            build: None,
            commit: None,
        }
    }

    #[test]
    fn support_runtime_only_before_major() {
        let v = version(GodotVersionStatus::Stable, 4);
        let input = GodotSupportClassificationInput {
            version: &v,
            edition: GodotEdition::RuntimeOnly,
            edition_confidence: GodotEditionConfidence::Medium,
            is_verified_baseline: false,
        };
        assert_eq!(
            classify_godot_support(input),
            SiralosGodotSupport::RuntimeOnly
        );
        let v3 = version(GodotVersionStatus::Stable, 3);
        let input3 = GodotSupportClassificationInput {
            version: &v3,
            edition: GodotEdition::Standard,
            edition_confidence: GodotEditionConfidence::High,
            is_verified_baseline: false,
        };
        assert_eq!(
            classify_godot_support(input3),
            SiralosGodotSupport::UnsupportedMajor
        );
    }

    #[test]
    fn edition_unknown_when_version_probe_failed() {
        let ev = GodotEditionEvidence {
            explicit_hint: None,
            filename: "godot".to_owned(),
            capabilities: empty_godot_command_capabilities(),
            api_configuration_features: vec![],
            probes_succeeded: GodotProbesSucceeded {
                version: false,
                help: true,
                api_dump: false,
            },
        };
        let c = classify_godot_edition(&ev);
        assert_eq!(c.edition, GodotEdition::Unknown);
    }

    #[test]
    fn editor_candidate_editions() {
        let mut profile = tests_support::make_profile(GodotEdition::Standard);
        assert!(is_editor_selection_candidate(&profile));
        profile.edition = GodotEdition::RuntimeOnly;
        assert!(!is_editor_selection_candidate(&profile));
    }

    pub(crate) mod tests_support {
        use super::super::*;
        use crate::godot::capabilities::empty_godot_command_capabilities;
        use crate::godot::version::{GodotVersion, GodotVersionStatus};

        pub fn make_profile(edition: GodotEdition) -> GodotEngineProfile {
            GodotEngineProfile {
                installation_id: "test".to_owned(),
                fingerprint: "abc12345".to_owned(),
                version: GodotVersion {
                    raw: "4.7.1".to_owned(),
                    major: 4,
                    minor: 7,
                    patch: Some(1),
                    status: GodotVersionStatus::Stable,
                    status_number: None,
                    build: Some("official".to_owned()),
                    commit: None,
                },
                edition,
                edition_confidence: GodotEditionConfidence::High,
                release_channel:
                    crate::godot::version::GodotReleaseChannel::Stable,
                capabilities: empty_godot_command_capabilities(),
                verified_capabilities: vec![],
                degraded_capabilities: vec![],
                executable_sha256: "a".repeat(64),
                api_dump_sha256: None,
                support: SiralosGodotSupport::CompatibleUntested,
                diagnostics: vec![],
            }
        }
    }
}
