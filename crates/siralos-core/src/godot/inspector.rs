//! Provider-neutral Godot inspection ports and result models (R8).
//!
//! Mirrors `packages/core/src/godot/inspector.ts`. Absolute executable
//! paths never enter provider-visible results; fingerprints are used instead.

use super::capabilities::GodotCommandCapabilities;
use super::compatibility::GodotCompatibilityAssessment;
use super::diagnostics::SafeDiagnostic;
use super::engine_profile::{
    GodotEdition, GodotEditionConfidence, SiralosGodotSupport,
};
use super::installations::GodotInstallationSource;
use super::project::GodotProjectProfile;
use super::version::{GodotReleaseChannel, GodotVersion};

/// Provider-safe summary of one installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotInstallationOverview {
    /// Installation id.
    pub installation_id: String,
    /// Version when profiled.
    pub version: Option<GodotVersion>,
    /// Edition when profiled.
    pub edition: Option<GodotEdition>,
    /// Edition confidence when profiled.
    pub edition_confidence: Option<GodotEditionConfidence>,
    /// Release channel when profiled.
    pub release_channel: Option<GodotReleaseChannel>,
    /// Human source label.
    pub source_label: String,
    /// Machine source.
    pub source: GodotInstallationSource,
    /// Support when profiled.
    pub support: Option<SiralosGodotSupport>,
    /// Bounded error for invalid candidates; None when valid.
    pub invalid: Option<String>,
    /// True when another candidate shares the same canonical path.
    pub is_duplicate: bool,
    /// True when this candidate is the selected one.
    pub selected: bool,
    /// Short SHA-256 prefix (provider-safe fingerprint).
    pub fingerprint: Option<String>,
    /// False when not yet profiled.
    pub profiled: bool,
}

/// Result of discovery, validation, and selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiscoveryResult {
    /// All candidates.
    pub candidates: Vec<GodotInstallationOverview>,
    /// Effective configuration summary.
    pub configuration: GodotDiscoveryConfiguration,
    /// Selected candidate, if any.
    pub selected: Option<GodotInstallationOverview>,
    /// Bounded automatic-selection rationale.
    pub rationale: Vec<String>,
    /// Bounded diagnostics.
    pub diagnostics: Vec<SafeDiagnostic>,
}

/// Effective configuration summary embedded in a discovery result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiscoveryConfiguration {
    /// Active installation id, if configured.
    pub active_installation: Option<String>,
    /// Number of configured installations.
    pub configured_count: usize,
    /// Whether PATH discovery is enabled.
    pub discover_on_path: bool,
    /// Override descriptions (e.g. `--godot-path`).
    pub overrides: Vec<String>,
}

/// Provider-safe view of the selected installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotSelectedInstallation {
    /// Installation id.
    pub installation_id: String,
    /// Source label.
    pub source_label: String,
    /// Source.
    pub source: GodotInstallationSource,
    /// Version.
    pub version: GodotVersion,
    /// Edition.
    pub edition: GodotEdition,
    /// Confidence.
    pub edition_confidence: GodotEditionConfidence,
    /// Release channel.
    pub release_channel: GodotReleaseChannel,
    /// Support.
    pub support: SiralosGodotSupport,
    /// Advertised capabilities.
    pub capabilities: GodotCommandCapabilities,
    /// Operationally verified capabilities.
    pub verified_capabilities: Vec<String>,
    /// Advertised but degraded capabilities.
    pub degraded_capabilities: Vec<String>,
    /// Short executable SHA-256 prefix.
    pub executable_fingerprint: String,
    /// SHA-256 of the API dump, if available.
    pub api_dump_sha256: Option<String>,
    /// Diagnostics.
    pub diagnostics: Vec<SafeDiagnostic>,
}

/// Truthful platform support for recovery-mode probing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotRecoveryProbeSupport {
    /// `available` or `unavailable`.
    pub state: GodotRecoveryProbeState,
    /// Reason when unavailable.
    pub reason: Option<String>,
    /// Platform string.
    pub platform: String,
}

/// State of recovery-probe support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotRecoveryProbeState {
    /// Available.
    Available,
    /// Unavailable.
    Unavailable,
}

impl GodotRecoveryProbeState {
    /// Canonical string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Truthful support for version-matched API knowledge generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotKnowledgeSupport {
    /// State.
    pub state: GodotKnowledgeSupportState,
    /// Reason when unavailable.
    pub reason: Option<String>,
    /// Platform.
    pub platform: String,
}

/// State of knowledge support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotKnowledgeSupportState {
    /// Available.
    Available,
    /// Unavailable.
    Unavailable,
}

/// Truthful support for GDScript check-only diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiagnosticsSupport {
    /// State.
    pub state: GodotDiagnosticsSupportState,
    /// Reason when unavailable.
    pub reason: Option<String>,
    /// Platform.
    pub platform: String,
}

/// State of diagnostics support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotDiagnosticsSupportState {
    /// Available.
    Available,
    /// Unavailable.
    Unavailable,
}

/// Full bounded diagnostics report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDoctorReport {
    /// Discovery.
    pub discovery: GodotDiscoveryResult,
    /// Project.
    pub project: GodotProjectProfile,
    /// Compatibility.
    pub compatibility: GodotCompatibilityAssessment,
    /// Engine-profile cache summary.
    pub cache: GodotDoctorCache,
    /// Sandbox summary.
    pub sandbox: GodotDoctorSandbox,
    /// Degraded capabilities.
    pub degraded_capabilities: Vec<String>,
    /// Recovery probe support.
    pub recovery_probe: GodotRecoveryProbeSupport,
    /// Knowledge support.
    pub knowledge: GodotKnowledgeSupport,
    /// Diagnostics support.
    pub diagnostics: GodotDiagnosticsSupport,
    /// Per-probe status lines.
    pub probes: Vec<GodotProbeStatusLine>,
}

/// Cache summary in a doctor report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDoctorCache {
    /// Schema version.
    pub schema_version: u32,
    /// Cached profile count.
    pub cached_profile_count: usize,
    /// Whether cache is enabled.
    pub enabled: bool,
}

/// Sandbox summary in a doctor report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDoctorSandbox {
    /// State string.
    pub state: String,
    /// Backend id.
    pub backend_id: String,
    /// Host-read restriction.
    pub filesystem_read_restriction: bool,
    /// Network restriction.
    pub network_restriction: bool,
    /// Writable restriction.
    pub filesystem_write_restriction: bool,
    /// Process-tree restriction.
    pub process_tree_restriction: bool,
}

/// One per-probe status line in a doctor report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotProbeStatusLine {
    /// Installation id.
    pub installation_id: String,
    /// Probe kind.
    pub probe: String,
    /// Status string.
    pub status: String,
}

/// One internally consistent read-only snapshot for lightweight status surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotStatusSnapshot {
    /// Selected installation.
    pub selected: Option<GodotSelectedInstallation>,
    /// Project profile.
    pub project: GodotProjectProfile,
    /// Compatibility.
    pub compatibility: GodotCompatibilityAssessment,
}
