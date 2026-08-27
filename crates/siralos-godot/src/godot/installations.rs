//! Validated Godot executable candidate (R8 Godot Stage-2 parity).

/// User-supplied edition hint. A hint only, never authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotEditionHint {
    /// Standard editor.
    Standard,
    /// .NET editor.
    Dotnet,
    /// Unknown (no hint supplied).
    Unknown,
}

/// Discovery source of a Godot installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotInstallationSource {
    /// From user config.
    UserConfig,
    /// From PATH scan.
    Path,
    /// From CLI `--godot-path`.
    CliPath,
    /// From CLI `--godot-installation`.
    CliInstallation,
    /// From `SIRALOS_GODOT` env.
    EnvironmentPath,
    /// From `SIRALOS_GODOT_INSTALLATION` env.
    EnvironmentInstallation,
    /// Active config entry.
    ActiveConfig,
}

/// A validated Godot executable candidate.
///
/// The canonical path is private to Siralos and must never enter
/// provider-visible results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotInstallation {
    /// Stable id of this candidate.
    pub id: String,
    /// Human-readable discovery source label.
    pub source_label: String,
    /// Machine source for provenance.
    pub source: GodotInstallationSource,
    /// Canonical absolute path of the executable (private).
    pub canonical_path: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Modification time in epoch milliseconds.
    pub modified_at_ms: u64,
    /// SHA-256 of the executable bytes (64 hex chars).
    pub sha256: String,
    /// Edition hint (not the classified edition).
    pub edition_hint: GodotEditionHint,
    /// Whether the candidate is valid.
    pub status_valid: bool,
    /// Present only for invalid candidates; bounded.
    pub error: Option<String>,
}
