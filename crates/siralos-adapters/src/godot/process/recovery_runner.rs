//! The recovery-mode runner fails closed and never spawns the
//! executable.
//!
//! The required invariant — the executable opened by the OS must be the
//! exact object whose bytes produced the trusted SHA-256 fingerprint,
//! running against a mirror that contains exactly the approved bytes,
//! with cleanup bound to the exact created objects — cannot be enforced
//! with pathname-based spawn against a same-user adversary. Rather than
//! weakening the threat model, every run reports a typed `unavailable`
//! outcome and no mirror is created.
//!
//! Recovery mode remains a requirement, not a fallback: an engine that
//! does not advertise `--recovery-mode`, `--editor`, `--headless`, and
//! `--path` is reported unsupported and no weaker mode is ever
//! substituted.

use siralos_core::identity::sha256_hex;
use siralos_godot::godot::{
    GODOT_LIMITS, GodotEdition, GodotEngineProfile, GodotInstallation,
};

/// Fixed Siralos-owned recovery-mode editor invocation tuple.
pub const GODOT_RECOVERY_BASE_ARGUMENTS: [&str; 3] =
    ["--headless", "--editor", "--recovery-mode"];

/// Canonical placeholder for the Siralos-generated mirror path in digests.
pub const GODOT_RECOVERY_MIRROR_PATH_MARKER: &str = "<disposable-mirror>";

/// Truthful reason reported for every recovery run while launch and
/// mirror lifecycle cannot be bound to verified objects.
pub const GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE: &str = "Recovery-mode Godot execution is unavailable: Node and the pinned sandbox runtime offer no exec-by-handle or directory-handle-relative primitive, so the staged executable's pathname is re-opened at spawn time and a same-user process could substitute different bytes between final verification and launch, the verified parent could be substituted before mirror creation, and cleanup could delete a substituted object. The verified fingerprint could then be attached to bytes that never execute. The runner fails closed and never spawns the executable, and no mirror is created; it will become available only when a mechanically identity-bound launch and mirror-lifecycle primitive exists.";

fn fixed_arguments(path: &str) -> Vec<String> {
    let mut arguments: Vec<String> = GODOT_RECOVERY_BASE_ARGUMENTS
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect();
    arguments.push("--path".to_owned());
    arguments.push(path.to_owned());
    arguments.push("--quit-after".to_owned());
    arguments.push(GODOT_LIMITS.recovery_quit_after_iterations.to_string());
    arguments
}

/// Fixed recovery invocation against one mirror project path.
pub fn godot_recovery_arguments(mirror_project_path: &str) -> Vec<String> {
    fixed_arguments(mirror_project_path)
}

/// Fixed argument template with the mirror path canonicalized to the
/// marker.
pub fn godot_recovery_argument_template() -> Vec<String> {
    fixed_arguments(GODOT_RECOVERY_MIRROR_PATH_MARKER)
}

/// Siralos-chosen aspects of the recovery command bound by the digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotRecoveryCommandDigestParts {
    /// Executable SHA-256.
    pub executable_sha256: String,
    /// Argument template with the mirror path replaced by the marker.
    pub argument_template: Vec<String>,
    /// Recovery profile id.
    pub profile_id: String,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
    /// Stdout limit in bytes.
    pub stdout_limit_bytes: u64,
    /// Stderr limit in bytes.
    pub stderr_limit_bytes: u64,
}

/// Deterministic digest over the fixed recovery command.
///
/// The mirror path is canonicalized to the marker so the digest is
/// stable between approval and execution while still binding every
/// Siralos-chosen aspect of the command.
pub fn compute_godot_recovery_command_digest(
    parts: &GodotRecoveryCommandDigestParts,
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
    sha256_hex(value.to_string().as_bytes())
}

/// The run was cancelled before refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotRecoveryCancelled {
    /// Bounded cancellation message.
    pub message: String,
}

#[cfg(test)]
pub(crate) mod test_support {
    use siralos_godot::godot::{
        GodotCapabilityKey, GodotCommandCapabilities, GodotEdition,
        GodotEditionConfidence, GodotEngineProfile, GodotInstallation,
        GodotInstallationSource, GodotReleaseChannel, GodotVersion,
        GodotVersionStatus, SiralosGodotSupport,
        empty_godot_command_capabilities,
    };

    pub(crate) fn installation(status_valid: bool) -> GodotInstallation {
        GodotInstallation {
            id: "path-1".to_owned(),
            source_label: "explicit path".to_owned(),
            source: GodotInstallationSource::CliPath,
            canonical_path: "C:\\godot\\Godot.exe".to_owned(),
            size_bytes: 1000,
            modified_at_ms: 1000,
            sha256: "a".repeat(64),
            edition_hint: siralos_godot::godot::InstallEditionHint::Unknown,
            status_valid,
            error: None,
        }
    }

    fn capabilities() -> GodotCommandCapabilities {
        let mut capabilities = empty_godot_command_capabilities();
        GodotCapabilityKey::Editor.apply(&mut capabilities, true);
        GodotCapabilityKey::Headless.apply(&mut capabilities, true);
        GodotCapabilityKey::RecoveryMode.apply(&mut capabilities, true);
        GodotCapabilityKey::ProjectPath.apply(&mut capabilities, true);
        capabilities
    }

    pub(crate) fn engine_profile(edition: GodotEdition) -> GodotEngineProfile {
        GodotEngineProfile {
            installation_id: "path-1".to_owned(),
            fingerprint: "b".repeat(8),
            version: GodotVersion {
                raw: "4.7.1.stable.official".to_owned(),
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
            release_channel: GodotReleaseChannel::Stable,
            capabilities: capabilities(),
            verified_capabilities: Vec::new(),
            degraded_capabilities: Vec::new(),
            executable_sha256: "a".repeat(64),
            api_dump_sha256: None,
            support: SiralosGodotSupport::Verified,
            diagnostics: Vec::new(),
        }
    }
}

/// Observable outcomes of the fail-closed recovery runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotRecoveryRunOutcome {
    /// The engine cannot run the recovery probe as specified.
    Unsupported {
        /// Bounded truthful reason.
        message: String,
    },
    /// Execution is unavailable under the current enforcement boundary.
    Unavailable {
        /// Bounded truthful reason.
        message: String,
    },
}

/// Inputs to one recovery run attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotRecoveryRunRequest<'a> {
    /// Selected installation.
    pub installation: &'a GodotInstallation,
    /// Selected engine profile.
    pub engine_profile: &'a GodotEngineProfile,
    /// Host-owned cancellation observation; cancelled runs refuse before
    /// any precondition evaluation.
    pub cancelled: bool,
}

/// The fail-closed recovery runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailClosedGodotRecoveryRunner;

impl FailClosedGodotRecoveryRunner {
    /// Reports whether recovery execution can run at all.
    #[must_use]
    pub fn is_available(&self) -> bool {
        false
    }

    /// Refuse the run without spawning anything or creating a mirror.
    pub fn run(
        &self,
        request: GodotRecoveryRunRequest<'_>,
    ) -> Result<GodotRecoveryRunOutcome, GodotRecoveryCancelled> {
        if request.cancelled {
            return Err(GodotRecoveryCancelled {
                message: "The Godot recovery probe was aborted.".to_owned(),
            });
        }
        if let Some(message) = require_recovery_capabilities(request) {
            return Ok(GodotRecoveryRunOutcome::Unsupported { message });
        }
        Ok(GodotRecoveryRunOutcome::Unavailable {
            message: GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE.to_owned(),
        })
    }
}

/// Create the fail-closed recovery runner.
pub fn create_godot_recovery_runner() -> FailClosedGodotRecoveryRunner {
    FailClosedGodotRecoveryRunner
}

fn require_recovery_capabilities(
    request: GodotRecoveryRunRequest<'_>,
) -> Option<String> {
    if !request.installation.status_valid {
        return Some(
            "The installation is invalid; rediscovery is required.".to_owned(),
        );
    }
    if request.engine_profile.edition == GodotEdition::RuntimeOnly {
        return Some(
            "The selected executable is runtime-only; it cannot run the editor recovery probe."
                .to_owned(),
        );
    }
    let capabilities = &request.engine_profile.capabilities;
    if !capabilities.recovery_mode {
        return Some(
            "The selected Godot version does not advertise --recovery-mode; the recovery probe is unsupported and no weaker mode is used."
                .to_owned(),
        );
    }
    if !capabilities.editor {
        return Some(
            "The selected Godot version does not advertise --editor; the recovery probe is unsupported."
                .to_owned(),
        );
    }
    if !capabilities.headless {
        return Some(
            "The selected Godot version does not advertise --headless; the recovery probe is unsupported."
                .to_owned(),
        );
    }
    if !capabilities.project_path {
        return Some(
            "The selected Godot version does not advertise --path; the recovery probe is unsupported."
                .to_owned(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::test_support::{engine_profile, installation};
    use super::{
        GODOT_RECOVERY_BASE_ARGUMENTS, GODOT_RECOVERY_MIRROR_PATH_MARKER,
        GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE,
        GodotRecoveryCommandDigestParts, GodotRecoveryRunOutcome,
        GodotRecoveryRunRequest, compute_godot_recovery_command_digest,
        create_godot_recovery_runner, godot_recovery_argument_template,
        godot_recovery_arguments,
    };
    use siralos_godot::godot::GodotEdition;

    #[test]
    fn fixed_headless_recovery_mode_editor_tuple() {
        assert_eq!(
            GODOT_RECOVERY_BASE_ARGUMENTS,
            ["--headless", "--editor", "--recovery-mode"]
        );
        assert_eq!(
            godot_recovery_arguments("C:\\mirror\\project"),
            [
                "--headless",
                "--editor",
                "--recovery-mode",
                "--path",
                "C:\\mirror\\project",
                "--quit-after",
                "120"
            ]
        );
    }

    #[test]
    fn template_canonicalizes_the_mirror_path() {
        assert_eq!(
            godot_recovery_argument_template(),
            [
                "--headless",
                "--editor",
                "--recovery-mode",
                "--path",
                GODOT_RECOVERY_MIRROR_PATH_MARKER,
                "--quit-after",
                "120"
            ]
        );
    }

    #[test]
    fn template_never_carries_execution_options() {
        for argument in godot_recovery_argument_template() {
            assert!(
                ![
                    "--script",
                    "--scene",
                    "--import",
                    "--upwards",
                    "--export",
                    "--lsp",
                    "--dap"
                ]
                .contains(&argument.as_str())
            );
        }
    }

    #[test]
    fn digest_is_deterministic_and_binds_inputs() {
        let parts = || GodotRecoveryCommandDigestParts {
            executable_sha256: "a".repeat(64),
            argument_template: godot_recovery_argument_template(),
            profile_id: "godot-recovery-probe-offline".to_owned(),
            timeout_ms: 60_000,
            stdout_limit_bytes: 1024 * 1024,
            stderr_limit_bytes: 1024 * 1024,
        };
        let base = compute_godot_recovery_command_digest(&parts());
        assert_eq!(base, compute_godot_recovery_command_digest(&parts()));
        let changed_executable = GodotRecoveryCommandDigestParts {
            executable_sha256: "b".repeat(64),
            ..parts()
        };
        assert_ne!(
            compute_godot_recovery_command_digest(&changed_executable),
            base
        );
        let changed_timeout =
            GodotRecoveryCommandDigestParts { timeout_ms: 30_000, ..parts() };
        assert_ne!(
            compute_godot_recovery_command_digest(&changed_timeout),
            base
        );
    }

    #[test]
    fn reports_unavailable_without_launching() {
        let runner = create_godot_recovery_runner();
        assert!(!runner.is_available());
        let installation = installation(true);
        let profile = engine_profile(GodotEdition::Standard);
        let outcome = runner
            .run(GodotRecoveryRunRequest {
                installation: &installation,
                engine_profile: &profile,
                cancelled: false,
            })
            .expect("not cancelled");
        assert_eq!(
            outcome,
            GodotRecoveryRunOutcome::Unavailable {
                message: GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE.to_owned()
            }
        );
    }

    #[test]
    fn rejects_unsupported_engines_before_the_unavailable_gate() {
        let runner = create_godot_recovery_runner();
        let valid_installation = installation(true);
        let invalid_installation = installation(false);
        let runtime_only = engine_profile(GodotEdition::RuntimeOnly);
        let outcome = runner
            .run(GodotRecoveryRunRequest {
                installation: &valid_installation,
                engine_profile: &runtime_only,
                cancelled: false,
            })
            .expect("not cancelled");
        assert!(matches!(
            outcome,
            GodotRecoveryRunOutcome::Unsupported { .. }
        ));
        let profile = engine_profile(GodotEdition::Standard);
        let outcome = runner
            .run(GodotRecoveryRunRequest {
                installation: &invalid_installation,
                engine_profile: &profile,
                cancelled: false,
            })
            .expect("not cancelled");
        assert!(matches!(
            outcome,
            GodotRecoveryRunOutcome::Unsupported { .. }
        ));
    }

    #[test]
    fn refuses_cancelled_runs_before_preconditions() {
        let runner = create_godot_recovery_runner();
        let installation = installation(true);
        let profile = engine_profile(GodotEdition::Standard);
        let error = runner
            .run(GodotRecoveryRunRequest {
                installation: &installation,
                engine_profile: &profile,
                cancelled: true,
            })
            .unwrap_err();
        assert_eq!(error.message, "The Godot recovery probe was aborted.");
    }
}
