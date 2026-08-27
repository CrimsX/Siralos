//! Provider-neutral GDScript diagnostic model (R8).
//!
//! Mirrors `packages/core/src/godot/gdscript.ts`.
//!
//! Bounded, sanitized; unknown line/column values are never fabricated.

/// Source of a GDScript diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GdScriptDiagnosticSource {
    /// From `godot --check-only`.
    CheckOnly,
    /// From LSP.
    Lsp,
}

/// Severity of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GdScriptSeverity {
    /// Error.
    Error,
    /// Warning.
    Warning,
    /// Info.
    Info,
    /// Unknown (engine output carried none).
    Unknown,
}

/// One bounded, sanitized GDScript diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotGdScriptDiagnostic {
    /// Source.
    pub source: GdScriptDiagnosticSource,
    /// Severity.
    pub severity: GdScriptSeverity,
    /// Workspace-relative path; `None` when absent.
    pub path: Option<String>,
    /// 1-based line, if present.
    pub line: Option<u32>,
    /// 1-based column, if present.
    pub column: Option<u32>,
    /// Stable code, if present.
    pub code: Option<String>,
    /// Bounded, sanitized message.
    pub message: String,
    /// Raw category token, if present.
    pub raw_category: Option<String>,
}

/// One script target of a prepared diagnostic check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotScriptCheckTarget {
    /// Workspace-relative `/`-separated path.
    pub path: String,
    /// SHA-256 of the script bytes (64 hex).
    pub sha256: String,
    /// Size in bytes.
    pub bytes: usize,
}

/// Preview shown before the one-time approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiagnosticPreview {
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
    /// Script counts + total bytes.
    pub scripts: GodotDiagnosticScripts,
    /// Risk-manifest digest the approval binds to.
    pub manifest_digest: String,
}

/// Script summary inside a diagnostic preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiagnosticScripts {
    /// Script count.
    pub count: usize,
    /// Exact relative paths for single-script checks; `None` for project-wide.
    pub paths: Option<Vec<String>>,
    /// Total bytes.
    pub total_bytes: usize,
}

/// Sandbox profile id for offline GDScript diagnostics.
pub const GODOT_DIAGNOSTICS_OFFLINE_PROFILE_ID: &str =
    "godot-diagnostics-offline";

/// Opaque single-use prepared check allocated by the diagnostics service.
///
/// The service owns the internal plan keyed by this handle; providers and
/// the CLI can only pass it back together with the approved digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreparedGDScriptCheck {
    handle: u64,
}

impl PreparedGDScriptCheck {
    /// Allocate a fresh opaque handle.
    #[must_use]
    pub fn create(handle: u64) -> Self {
        Self { handle }
    }

    /// The internal handle value.
    #[must_use]
    pub fn handle(&self) -> u64 {
        self.handle
    }
}

/// Siralos-fixed aspects of the check-only command bound by the digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotCheckOnlyCommandDigestParts {
    /// Executable SHA-256.
    pub executable_sha256: String,
    /// Marker-canonicalized argument template.
    pub argument_template: Vec<String>,
    /// Sandbox profile id.
    pub profile_id: String,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
    /// Stdout limit in bytes.
    pub stdout_limit_bytes: u64,
    /// Stderr limit in bytes.
    pub stderr_limit_bytes: u64,
}

/// Deterministic digest over the fixed check-only command shape.
#[must_use]
pub fn compute_godot_check_only_command_digest(
    parts: &GodotCheckOnlyCommandDigestParts,
) -> String {
    let value = serde_json::json!({
        "executableSha256": parts.executable_sha256,
        "argumentTemplate": parts.argument_template,
        "workingDirectoryPolicy": "disposable-mirror",
        "profileId": parts.profile_id,
        "environmentPolicy": "minimal",
        "stdinPolicy": "closed",
        "networkPolicy": "denied",
        "timeoutMs": parts.timeout_ms,
        "stdoutLimitBytes": parts.stdout_limit_bytes,
        "stderrLimitBytes": parts.stderr_limit_bytes,
    });
    siralos_core::identity::sha256_hex_str(&value.to_string())
}

/// Siralos-fixed check limits bound into the prepared-check digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GodotPreparedCheckLimits {
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
    /// Maximum scripts per project-wide check.
    pub max_scripts: usize,
    /// Maximum total script bytes.
    pub max_total_bytes: usize,
    /// Maximum diagnostics retained per script.
    pub max_diagnostics_per_script: usize,
    /// Maximum diagnostics retained per run.
    pub max_diagnostics_per_run: usize,
}

/// Inputs frozen into the prepared-check digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotPreparedCheckDigestParts {
    /// Sorted script targets (path + content hash).
    pub script_targets: Vec<GodotScriptCheckTarget>,
    /// Risk-manifest digest at preparation time.
    pub manifest_digest: String,
    /// Fixed command digest.
    pub command_digest: String,
    /// Sandbox profile id.
    pub sandbox_profile_id: String,
    /// Immutable Siralos-fixed limits.
    pub check_limits: GodotPreparedCheckLimits,
}

/// Deterministic digest binding every security-relevant prepared-check
/// input; approval binds to exactly this value.
#[must_use]
pub fn compute_godot_prepared_check_digest(
    parts: &GodotPreparedCheckDigestParts,
) -> String {
    let targets: Vec<serde_json::Value> = parts
        .script_targets
        .iter()
        .map(|target| {
            serde_json::json!({
                "path": target.path,
                "sha256": target.sha256,
                "bytes": target.bytes,
            })
        })
        .collect();
    let value = serde_json::json!({
        "scriptTargets": targets,
        "manifestDigest": parts.manifest_digest,
        "commandDigest": parts.command_digest,
        "sandboxProfileId": parts.sandbox_profile_id,
        "checkLimits": {
            "timeoutMs": parts.check_limits.timeout_ms,
            "maxScripts": parts.check_limits.max_scripts,
            "maxTotalBytes": parts.check_limits.max_total_bytes,
            "maxDiagnosticsPerScript": parts.check_limits.max_diagnostics_per_script,
            "maxDiagnosticsPerRun": parts.check_limits.max_diagnostics_per_run,
        },
    });
    siralos_core::identity::sha256_hex_str(&value.to_string())
}

/// Non-ready preparation statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotCheckPreparationStatus {
    /// Execution cannot run under the current enforcement boundary.
    Unavailable,
    /// The selected engine cannot parse scripts as specified.
    Unsupported,
    /// The request was malformed.
    InvalidInput,
    /// Preparation ran and failed.
    Failed,
}

impl GodotCheckPreparationStatus {
    /// Canonical string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::InvalidInput => "invalid_input",
            Self::Failed => "failed",
        }
    }
}

/// Outcome of preparing a GDScript check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotCheckPreparationResult {
    /// Scripts validated and hashed; the plan is frozen under `digest`.
    Ready {
        /// Opaque single-use handle.
        check: PreparedGDScriptCheck,
        /// Reader-facing preview shown before approval.
        preview: Box<GodotDiagnosticPreview>,
        /// Digest the one-time approval binds to.
        digest: String,
    },
    /// Typed non-ready outcome.
    NotReady {
        /// Status.
        status: GodotCheckPreparationStatus,
        /// Bounded truthful reason.
        message: String,
    },
}

/// Non-checked run statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotProjectCheckRunStatus {
    /// The approval did not match the prepared plan.
    Denied,
    /// Fresh state no longer matches the frozen plan.
    Conflict,
    /// Cancelled before completion.
    Cancelled,
    /// Engine invocation timed out.
    TimedOut,
    /// The engine cannot run the check as specified.
    Unsupported,
    /// Execution is unavailable on this platform.
    Unavailable,
    /// Sandbox enforcement failed.
    SandboxFailed,
    /// The run failed.
    Failed,
}

impl GodotProjectCheckRunStatus {
    /// Canonical string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::Conflict => "conflict",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::SandboxFailed => "sandbox_failed",
            Self::Failed => "failed",
        }
    }
}

/// Outcome of executing one prepared check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotProjectCheckResult {
    /// Every requested script was parsed; per-script parse failures are
    /// VALID results carried as diagnostics (`valid_count`/`invalid_count`),
    /// never as run failures.
    Checked {
        /// Exact engine version string.
        engine_version: String,
        /// Number of scripts checked.
        scripts_checked: usize,
        /// Scripts whose parse reported no error diagnostics.
        valid_count: usize,
        /// Scripts whose parse reported errors.
        invalid_count: usize,
        /// Bounded aggregated diagnostics.
        diagnostics: Vec<GodotGdScriptDiagnostic>,
        /// True when any aggregation bound applied.
        truncated: bool,
    },
    /// The run produced no checked outcome.
    NotChecked {
        /// Typed run status.
        status: GodotProjectCheckRunStatus,
        /// Bounded truthful reason.
        message: String,
    },
}

/// Truthful platform-level support for check execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiagnosticsSupport {
    /// Availability.
    pub state: super::knowledge::KnowledgeSupportState,
    /// Exact reason when unavailable; `None` when available.
    pub reason: Option<String>,
    /// Platform string.
    pub platform: String,
}

/// In-memory trust state of the diagnostics surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotDiagnosticsState {
    /// No approval is live.
    Untrusted,
    /// A previous approval was invalidated (conflict/staleness).
    CheckInvalidated,
}

impl GodotDiagnosticsState {
    /// Canonical string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Untrusted => "untrusted",
            Self::CheckInvalidated => "check-invalidated",
        }
    }
}

/// Bounded in-memory diagnostics state for CLI diagnostics. Nothing here
/// is a persistent trust grant; approval is one-time and never persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiagnosticsStatus {
    /// Trust state.
    pub state: GodotDiagnosticsState,
    /// Last run outcome, if any.
    pub last_result: Option<GodotProjectCheckResult>,
    /// Last refreshed manifest digest, if any.
    pub last_manifest_digest: Option<String>,
    /// Last engine version, if any.
    pub last_engine_version: Option<String>,
}

/// Diagnostic request. `paths` is an optional bounded subset of
/// workspace-relative `.gd` files; absent means project-wide enumeration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GodotDiagnosticsRequest {
    /// Optional bounded path subset.
    pub paths: Option<Vec<String>>,
}

/// Execution context supplied by the host after one-time approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiagnosticsExecutionContext {
    /// The approved digest; must equal the prepared plan's digest.
    pub approved_digest: String,
    /// Host-owned cancellation observation.
    pub cancelled: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        GdScriptDiagnosticSource, GdScriptSeverity,
        GodotCheckOnlyCommandDigestParts, GodotDiagnosticsState,
        GodotPreparedCheckDigestParts, GodotPreparedCheckLimits,
        GodotScriptCheckTarget, compute_godot_check_only_command_digest,
        compute_godot_prepared_check_digest,
    };

    #[test]
    fn diagnostic_source_variants() {
        assert_ne!(
            GdScriptDiagnosticSource::CheckOnly,
            GdScriptDiagnosticSource::Lsp
        );
    }

    #[test]
    fn severity_variants() {
        assert_ne!(GdScriptSeverity::Error, GdScriptSeverity::Unknown);
    }

    fn command_parts() -> GodotCheckOnlyCommandDigestParts {
        GodotCheckOnlyCommandDigestParts {
            executable_sha256: "a".repeat(64),
            argument_template: vec![
                "--headless".to_owned(),
                "--check-only".to_owned(),
            ],
            profile_id: "godot-diagnostics-offline".to_owned(),
            timeout_ms: 30_000,
            stdout_limit_bytes: 8 * 1024 * 1024,
            stderr_limit_bytes: 8 * 1024 * 1024,
        }
    }

    #[test]
    fn check_only_command_digest_is_deterministic_and_binding() {
        let base = compute_godot_check_only_command_digest(&command_parts());
        assert_eq!(
            base,
            compute_godot_check_only_command_digest(&command_parts())
        );
        let changed = GodotCheckOnlyCommandDigestParts {
            executable_sha256: "b".repeat(64),
            ..command_parts()
        };
        assert_ne!(compute_godot_check_only_command_digest(&changed), base);
    }

    #[test]
    fn prepared_check_digest_binds_targets_limits_and_manifest() {
        let parts = || GodotPreparedCheckDigestParts {
            script_targets: vec![GodotScriptCheckTarget {
                path: "src/player.gd".to_owned(),
                sha256: "b".repeat(64),
                bytes: 12,
            }],
            manifest_digest: "c".repeat(64),
            command_digest: compute_godot_check_only_command_digest(
                &command_parts(),
            ),
            sandbox_profile_id: super::GODOT_DIAGNOSTICS_OFFLINE_PROFILE_ID
                .to_owned(),
            check_limits: GodotPreparedCheckLimits {
                timeout_ms: 30_000,
                max_scripts: 500,
                max_total_bytes: 32 * 1024 * 1024,
                max_diagnostics_per_script: 500,
                max_diagnostics_per_run: 2000,
            },
        };
        let base = compute_godot_prepared_check_digest(&parts());
        assert_eq!(base, compute_godot_prepared_check_digest(&parts()));
        let changed_target = GodotPreparedCheckDigestParts {
            script_targets: vec![GodotScriptCheckTarget {
                path: "src/other.gd".to_owned(),
                sha256: "b".repeat(64),
                bytes: 12,
            }],
            ..parts()
        };
        assert_ne!(compute_godot_prepared_check_digest(&changed_target), base);
        let changed_manifest = GodotPreparedCheckDigestParts {
            manifest_digest: "d".repeat(64),
            ..parts()
        };
        assert_ne!(
            compute_godot_prepared_check_digest(&changed_manifest),
            base
        );
        assert_eq!(super::PreparedGDScriptCheck::create(1).handle(), 1);
        assert_eq!(
            super::GodotProjectCheckRunStatus::Unavailable.as_str(),
            "unavailable"
        );
        assert_eq!(GodotDiagnosticsState::Untrusted.as_str(), "untrusted");
    }
}
