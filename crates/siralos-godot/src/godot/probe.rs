//! Recovery-mode project-probe typed models and digests (R8).
//!
//! Mirrors `packages/core/src/godot/probe.ts`. Provider cannot choose
//! executable, arguments, mirror location, sandbox config, or limits;
//! `prepare` freezes the plan and `execute` checks the digest.

use super::diagnostics::SafeDiagnostic;
use siralos_core::identity::{CanonicalValue, sha256_hex};
use std::collections::BTreeMap;

/// Trust state — one-time approval, never persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotProjectTrustState {
    /// No approval live.
    Untrusted,
    /// Probe approved and executing / prepared awaiting execution.
    ProbeApproved,
    /// Previously prepared probe no longer matches state.
    ProbeInvalidated,
}

impl GodotProjectTrustState {
    /// Canonical string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Untrusted => "untrusted",
            Self::ProbeApproved => "probe-approved",
            Self::ProbeInvalidated => "probe-invalidated",
        }
    }
}

/// Workspace-relative file with bounded content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotFileRiskEntry {
    /// Workspace-relative path.
    pub path: String,
    /// SHA-256 (64 hex).
    pub sha256: String,
    /// Bytes.
    pub bytes: u64,
}

/// Editor-plugin descriptor with script hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotPluginRiskEntry {
    /// Workspace-relative path.
    pub path: String,
    /// Plugin name.
    pub name: String,
    /// Whether enabled.
    pub enabled: bool,
    /// SHA-256.
    pub sha256: String,
    /// Bytes.
    pub bytes: u64,
}

/// Referenced native library of a GDExtension descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotLibraryRiskEntry {
    /// Workspace-relative path.
    pub path: String,
    /// SHA-256 when available.
    pub sha256: Option<String>,
    /// Bytes when available.
    pub bytes: Option<u64>,
}

/// GDExtension descriptor with content hash and referenced libraries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotGDExtensionRiskEntry {
    /// Workspace-relative path.
    pub path: String,
    /// SHA-256.
    pub sha256: String,
    /// Bytes.
    pub bytes: u64,
    /// Referenced libraries.
    pub referenced_libraries: Vec<GodotLibraryRiskEntry>,
}

/// Autoload entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotAutoloadRiskEntry {
    /// Name.
    pub name: String,
    /// Target.
    pub target: String,
}

/// Fresh static risk inventory for one recovery probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotProjectRiskManifest {
    /// SHA-256 of `project.godot` bytes.
    pub project_file_sha256: String,
    /// Engine selection.
    pub engine_selection: GodotProbeEngineSelection,
    /// Tool scripts.
    pub tool_scripts: Vec<GodotFileRiskEntry>,
    /// Enabled editor plugins.
    pub enabled_editor_plugins: Vec<GodotPluginRiskEntry>,
    /// GDExtension descriptors.
    pub gdextension_descriptors: Vec<GodotGDExtensionRiskEntry>,
    /// Autoloads.
    pub autoloads: Vec<GodotAutoloadRiskEntry>,
    /// Dotnet project paths.
    pub dotnet_projects: Vec<String>,
    /// Bounded authored-file manifest.
    pub authored_file_manifest: GodotAuthoredFileManifest,
    /// Scan warnings.
    pub scan_warnings: Vec<SafeDiagnostic>,
    /// Deterministic SHA-256 over every security-relevant field above.
    pub digest: String,
}

/// Engine selection inside a risk manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotProbeEngineSelection {
    /// Installation id.
    pub installation_id: String,
    /// Executable SHA-256.
    pub executable_sha256: String,
    /// Version string.
    pub version: String,
}

/// Bounded authored-file manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotAuthoredFileManifest {
    /// File count.
    pub file_count: usize,
    /// Total bytes.
    pub total_bytes: u64,
    /// Digest over the manifest file list.
    pub digest: String,
    /// True when truncated by bounds.
    pub truncated: bool,
}

/// Compute deterministic SHA-256 over security-relevant fields (excludes `digest`).
#[must_use]
pub fn compute_godot_risk_manifest_digest(
    manifest: &GodotProjectRiskManifest,
) -> String {
    let mut map = BTreeMap::new();
    map.insert(
        "projectFileSha256".to_owned(),
        CanonicalValue::Str(manifest.project_file_sha256.clone()),
    );
    let mut engine = BTreeMap::new();
    engine.insert(
        "installationId".to_owned(),
        CanonicalValue::Str(manifest.engine_selection.installation_id.clone()),
    );
    engine.insert(
        "executableSha256".to_owned(),
        CanonicalValue::Str(
            manifest.engine_selection.executable_sha256.clone(),
        ),
    );
    engine.insert(
        "version".to_owned(),
        CanonicalValue::Str(manifest.engine_selection.version.clone()),
    );
    map.insert("engineSelection".to_owned(), CanonicalValue::Object(engine));
    map.insert(
        "toolScripts".to_owned(),
        CanonicalValue::Array(
            manifest.tool_scripts.iter().map(risk_file_value).collect(),
        ),
    );
    map.insert(
        "enabledEditorPlugins".to_owned(),
        CanonicalValue::Array(
            manifest
                .enabled_editor_plugins
                .iter()
                .map(risk_plugin_value)
                .collect(),
        ),
    );
    map.insert(
        "gdextensionDescriptors".to_owned(),
        CanonicalValue::Array(
            manifest
                .gdextension_descriptors
                .iter()
                .map(risk_gdext_value)
                .collect(),
        ),
    );
    map.insert(
        "autoloads".to_owned(),
        CanonicalValue::Array(
            manifest
                .autoloads
                .iter()
                .map(|a| {
                    let mut m = BTreeMap::new();
                    m.insert(
                        "name".to_owned(),
                        CanonicalValue::Str(a.name.clone()),
                    );
                    m.insert(
                        "target".to_owned(),
                        CanonicalValue::Str(a.target.clone()),
                    );
                    CanonicalValue::Object(m)
                })
                .collect(),
        ),
    );
    map.insert(
        "dotnetProjects".to_owned(),
        CanonicalValue::Array(
            manifest
                .dotnet_projects
                .iter()
                .map(|s| CanonicalValue::Str(s.clone()))
                .collect(),
        ),
    );
    let mut authored = BTreeMap::new();
    authored.insert(
        "fileCount".to_owned(),
        CanonicalValue::U64(manifest.authored_file_manifest.file_count as u64),
    );
    authored.insert(
        "totalBytes".to_owned(),
        CanonicalValue::U64(manifest.authored_file_manifest.total_bytes),
    );
    authored.insert(
        "digest".to_owned(),
        CanonicalValue::Str(manifest.authored_file_manifest.digest.clone()),
    );
    authored.insert(
        "truncated".to_owned(),
        CanonicalValue::Bool(manifest.authored_file_manifest.truncated),
    );
    map.insert(
        "authoredFileManifest".to_owned(),
        CanonicalValue::Object(authored),
    );
    map.insert(
        "scanWarnings".to_owned(),
        CanonicalValue::Array(
            manifest
                .scan_warnings
                .iter()
                .map(|d| {
                    let mut m = BTreeMap::new();
                    m.insert(
                        "severity".to_owned(),
                        CanonicalValue::Str(d.severity.as_str().to_owned()),
                    );
                    m.insert(
                        "message".to_owned(),
                        CanonicalValue::Str(d.message.clone()),
                    );
                    CanonicalValue::Object(m)
                })
                .collect(),
        ),
    );
    let canonical = CanonicalValue::Object(map).to_canonical();
    sha256_hex(canonical.as_bytes())
}

fn risk_file_value(entry: &GodotFileRiskEntry) -> CanonicalValue {
    let mut m = BTreeMap::new();
    m.insert("path".to_owned(), CanonicalValue::Str(entry.path.clone()));
    m.insert("sha256".to_owned(), CanonicalValue::Str(entry.sha256.clone()));
    m.insert("bytes".to_owned(), CanonicalValue::U64(entry.bytes));
    CanonicalValue::Object(m)
}

fn risk_plugin_value(entry: &GodotPluginRiskEntry) -> CanonicalValue {
    let mut m = BTreeMap::new();
    m.insert("path".to_owned(), CanonicalValue::Str(entry.path.clone()));
    m.insert("name".to_owned(), CanonicalValue::Str(entry.name.clone()));
    m.insert("enabled".to_owned(), CanonicalValue::Bool(entry.enabled));
    m.insert("sha256".to_owned(), CanonicalValue::Str(entry.sha256.clone()));
    m.insert("bytes".to_owned(), CanonicalValue::U64(entry.bytes));
    CanonicalValue::Object(m)
}

fn risk_gdext_value(entry: &GodotGDExtensionRiskEntry) -> CanonicalValue {
    let mut m = BTreeMap::new();
    m.insert("path".to_owned(), CanonicalValue::Str(entry.path.clone()));
    m.insert("sha256".to_owned(), CanonicalValue::Str(entry.sha256.clone()));
    m.insert("bytes".to_owned(), CanonicalValue::U64(entry.bytes));
    m.insert(
        "referencedLibraries".to_owned(),
        CanonicalValue::Array(
            entry
                .referenced_libraries
                .iter()
                .map(|l| {
                    let mut lm = BTreeMap::new();
                    lm.insert(
                        "path".to_owned(),
                        CanonicalValue::Str(l.path.clone()),
                    );
                    lm.insert(
                        "sha256".to_owned(),
                        l.sha256
                            .as_ref()
                            .map(|s| CanonicalValue::Str(s.clone()))
                            .unwrap_or(CanonicalValue::Null),
                    );
                    lm.insert(
                        "bytes".to_owned(),
                        l.bytes
                            .map(CanonicalValue::U64)
                            .unwrap_or(CanonicalValue::Null),
                    );
                    CanonicalValue::Object(lm)
                })
                .collect(),
        ),
    );
    CanonicalValue::Object(m)
}

/// Fixed Siralos-owned parts the prepared-probe digest binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotPreparedProbeDigestParts {
    /// Manifest digest.
    pub manifest_digest: String,
    /// Command digest.
    pub command_digest: String,
    /// Mirror policy version.
    pub mirror_policy_version: u32,
    /// Sandbox profile id.
    pub sandbox_profile_id: String,
    /// Probe limits.
    pub probe_limits: GodotProbeLimits,
}

/// Probe limits bound into the prepared-probe digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotProbeLimits {
    /// Timeout ms.
    pub timeout_ms: u64,
    /// Max files.
    pub max_files: usize,
    /// Max bytes.
    pub max_bytes: u64,
    /// Max single-file bytes.
    pub max_single_file_bytes: usize,
    /// Max depth.
    pub max_depth: usize,
    /// Max relative path bytes.
    pub max_relative_path_bytes: usize,
}

/// Compute prepared-probe digest.
#[must_use]
pub fn compute_godot_prepared_probe_digest(
    parts: &GodotPreparedProbeDigestParts,
) -> String {
    let mut map = BTreeMap::new();
    map.insert(
        "manifestDigest".to_owned(),
        CanonicalValue::Str(parts.manifest_digest.clone()),
    );
    map.insert(
        "commandDigest".to_owned(),
        CanonicalValue::Str(parts.command_digest.clone()),
    );
    map.insert(
        "mirrorPolicyVersion".to_owned(),
        CanonicalValue::U64(u64::from(parts.mirror_policy_version)),
    );
    map.insert(
        "sandboxProfileId".to_owned(),
        CanonicalValue::Str(parts.sandbox_profile_id.clone()),
    );
    let mut limits = BTreeMap::new();
    limits.insert(
        "timeoutMs".to_owned(),
        CanonicalValue::U64(parts.probe_limits.timeout_ms),
    );
    limits.insert(
        "maxFiles".to_owned(),
        CanonicalValue::U64(parts.probe_limits.max_files as u64),
    );
    limits.insert(
        "maxBytes".to_owned(),
        CanonicalValue::U64(parts.probe_limits.max_bytes),
    );
    limits.insert(
        "maxSingleFileBytes".to_owned(),
        CanonicalValue::U64(parts.probe_limits.max_single_file_bytes as u64),
    );
    limits.insert(
        "maxDepth".to_owned(),
        CanonicalValue::U64(parts.probe_limits.max_depth as u64),
    );
    limits.insert(
        "maxRelativePathBytes".to_owned(),
        CanonicalValue::U64(parts.probe_limits.max_relative_path_bytes as u64),
    );
    map.insert("probeLimits".to_owned(), CanonicalValue::Object(limits));
    sha256_hex(CanonicalValue::Object(map).to_canonical().as_bytes())
}

/// Immutable preview shown before one-time approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotProbePreview {
    /// Project name, if known.
    pub project_name: Option<String>,
    /// Engine version string.
    pub engine_version: String,
    /// Installation id.
    pub installation_id: String,
    /// Engine edition string.
    pub engine_edition: String,
    /// Support string.
    pub support: String,
    /// Compatibility string.
    pub compatibility: String,
    /// Risk counts.
    pub risks: GodotProbeRiskCounts,
    /// Mirror estimate.
    pub mirror: GodotProbeMirrorEstimate,
    /// Manifest digest.
    pub manifest_digest: String,
}

/// Risk counts in a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotProbeRiskCounts {
    /// Tool scripts.
    pub tool_scripts: usize,
    /// Enabled editor plugins.
    pub enabled_editor_plugins: usize,
    /// GDExtensions.
    pub gdextensions: usize,
    /// Autoloads.
    pub autoloads: usize,
    /// Dotnet projects.
    pub dotnet_projects: usize,
}

/// Mirror estimate in a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotProbeMirrorEstimate {
    /// Estimated file count.
    pub estimated_file_count: usize,
    /// Estimated bytes.
    pub estimated_bytes: u64,
}

/// Probe status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotProbeStatus {
    /// Completed.
    Completed,
    /// Completed with diagnostics.
    CompletedWithDiagnostics,
    /// Denied.
    Denied,
    /// Conflict.
    Conflict,
    /// Unsupported.
    Unsupported,
    /// Mirror too large.
    MirrorTooLarge,
    /// Unavailable.
    Unavailable,
    /// Timed out.
    TimedOut,
    /// Cancelled.
    Cancelled,
    /// Sandbox failed.
    SandboxFailed,
    /// Workspace changed.
    WorkspaceChanged,
    /// Failed.
    Failed,
}

impl GodotProbeStatus {
    /// Canonical string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::CompletedWithDiagnostics => "completed_with_diagnostics",
            Self::Denied => "denied",
            Self::Conflict => "conflict",
            Self::Unsupported => "unsupported",
            Self::MirrorTooLarge => "mirror_too_large",
            Self::Unavailable => "unavailable",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::SandboxFailed => "sandbox_failed",
            Self::WorkspaceChanged => "workspace_changed",
            Self::Failed => "failed",
        }
    }
}

/// Normalized diagnostic from recovery probe output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiagnostic {
    /// Severity.
    pub severity: crate::godot::diagnostics::DiagnosticSeverity,
    /// Category.
    pub category: GodotDiagnosticCategory,
    /// Bounded message.
    pub message: String,
}

/// Category of a Godot diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotDiagnosticCategory {
    /// Startup.
    Startup,
    /// Parser.
    Parser,
    /// Import.
    Import,
    /// Resource.
    Resource,
    /// Script.
    Script,
    /// Editor.
    Editor,
    /// Unknown.
    Unknown,
}

/// Import state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotImportState {
    /// Project opened.
    ProjectOpened,
    /// Resources scanned.
    ResourcesScanned,
    /// Imports observed.
    ImportsObserved,
    /// Imports not observed.
    ImportsNotObserved,
    /// Unknown.
    ImportStateUnknown,
}

#[cfg(test)]
mod tests {
    use super::GodotProjectTrustState;

    #[test]
    fn trust_state_strings() {
        assert_eq!(GodotProjectTrustState::Untrusted.as_str(), "untrusted");
    }
}
