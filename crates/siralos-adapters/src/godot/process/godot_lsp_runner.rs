//! Godot LSP server startup fails closed and never spawns the editor.
//!
//! The fixed invocation would run `--headless --editor --recovery-mode
//! --path <disposable-mirror> --lsp-port <allocated-loopback-port>`
//! inside an enforcing sandbox with the workspace excluded from readable
//! roots, external network denied, stdin closed, and the process tree
//! confined; until launch and mirror lifecycle can be mechanically bound
//! to verified identities, every start reports a typed `unavailable`
//! outcome with zero filesystem or network side effects.
//!
//! The architecture check enforces that this module is the only runtime
//! module that may pair `--lsp-port` with `--recovery-mode`, that `--path`
//! only references the disposable mirror, and that scene, script, import,
//! DAP/debug-server, export, and quit options never appear.

use siralos_core::godot::{
    GodotEdition, GodotEngineProfile, GodotInstallation,
};
use siralos_core::identity::sha256_hex;

/// Marker for the disposable mirror project path (never a real path).
pub const GODOT_LSP_MIRROR_PATH_MARKER: &str = "<disposable-mirror>";

/// Marker for the allocated loopback port (never a real port).
pub const GODOT_LSP_PORT_MARKER: &str = "<allocated-loopback-port>";

/// Fixed Siralos-owned Godot LSP session argument template.
pub const GODOT_LSP_BASE_ARGUMENTS: [&str; 7] = [
    "--headless",
    "--editor",
    "--recovery-mode",
    "--path",
    GODOT_LSP_MIRROR_PATH_MARKER,
    "--lsp-port",
    GODOT_LSP_PORT_MARKER,
];

/// The fixed argument template with markers, used for the command digest.
pub fn godot_lsp_argument_template() -> Vec<String> {
    GODOT_LSP_BASE_ARGUMENTS
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect()
}

/// The invocation arguments for one LSP session against the mirror.
pub fn godot_lsp_arguments(
    mirror_project_path: &str,
    allocated_port: u16,
) -> Vec<String> {
    vec![
        "--headless".to_owned(),
        "--editor".to_owned(),
        "--recovery-mode".to_owned(),
        "--path".to_owned(),
        mirror_project_path.to_owned(),
        "--lsp-port".to_owned(),
        allocated_port.to_string(),
    ]
}

/// Siralos-fixed aspects of the LSP command bound by the digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotLspSessionCommandDigestParts {
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

/// Deterministic digest over the fixed LSP session command.
#[must_use]
pub fn compute_godot_lsp_session_command_digest(
    parts: &GodotLspSessionCommandDigestParts,
) -> String {
    let value = serde_json::json!({
        "executableSha256": parts.executable_sha256,
        "argumentTemplate": parts.argument_template,
        "workingDirectoryPolicy": "disposable-mirror",
        "profileId": parts.profile_id,
        "environmentPolicy": "minimal",
        "stdinPolicy": "closed",
        "networkPolicy": "denied",
        "loopbackPolicy": "lsp-only",
        "timeoutMs": parts.timeout_ms,
        "stdoutLimitBytes": parts.stdout_limit_bytes,
        "stderrLimitBytes": parts.stderr_limit_bytes,
    });
    sha256_hex(value.to_string().as_bytes())
}

/// The session startup was cancelled before refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotLspCancelled {
    /// Bounded cancellation message.
    pub message: String,
}

/// Observable outcomes of the fail-closed LSP server runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotLspStartOutcome {
    /// The engine cannot host a language session as specified.
    Unsupported {
        /// Bounded truthful reason.
        message: String,
    },
    /// Startup is unavailable under the current enforcement boundary.
    Unavailable {
        /// Bounded truthful reason.
        message: String,
    },
}

/// Inputs to one session start attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotLspStartRequest<'a> {
    /// Selected installation.
    pub installation: &'a GodotInstallation,
    /// Selected engine profile.
    pub engine_profile: &'a GodotEngineProfile,
    /// Host-owned cancellation observation.
    pub cancelled: bool,
}

/// Truthful reason reported for every start attempt while launch and
/// mirror lifecycle cannot be bound to verified objects.
pub const GODOT_LSP_UNAVAILABLE_MESSAGE: &str = "The Godot GDScript language session is unavailable on this platform: Node and the pinned sandbox runtime offer no exec-by-handle, directory-relative create, or delete-by-handle primitive, so the approved Godot editor cannot be launched against exactly the approved mirrored project bytes, the disposable mirror cannot be constructed or cleaned up identity-bound, and the loopback LSP channel cannot be tied to a verified process identity. Sessions fail closed and the editor is never spawned; no mirror is created, no port is opened, and nothing is executed.";

/// The fail-closed LSP server runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailClosedGodotLspServerRunner;

impl FailClosedGodotLspServerRunner {
    /// Reports whether an LSP session can start at all.
    #[must_use]
    pub fn is_available(&self) -> bool {
        false
    }

    /// Refuse the start without spawning anything, creating a mirror, or
    /// opening a port.
    pub fn start_server(
        &self,
        request: GodotLspStartRequest<'_>,
    ) -> Result<GodotLspStartOutcome, GodotLspCancelled> {
        if request.cancelled {
            return Err(GodotLspCancelled {
                message: "The Godot language session startup was aborted."
                    .to_owned(),
            });
        }
        if let Some(message) = require_lsp_session_capabilities(request) {
            return Ok(GodotLspStartOutcome::Unsupported { message });
        }
        Ok(GodotLspStartOutcome::Unavailable {
            message: GODOT_LSP_UNAVAILABLE_MESSAGE.to_owned(),
        })
    }
}

/// Create the fail-closed LSP server runner.
pub fn create_godot_lsp_server_runner() -> FailClosedGodotLspServerRunner {
    FailClosedGodotLspServerRunner
}

fn require_lsp_session_capabilities(
    request: GodotLspStartRequest<'_>,
) -> Option<String> {
    if !request.installation.status_valid {
        return Some(
            "The installation is invalid; rediscovery is required.".to_owned(),
        );
    }
    if request.engine_profile.edition == GodotEdition::RuntimeOnly {
        return Some(
            "The selected executable is runtime-only; it cannot host a GDScript language server."
                .to_owned(),
        );
    }
    let capabilities = &request.engine_profile.capabilities;
    if !capabilities.lsp {
        return Some(
            "The selected Godot version does not advertise --lsp-port; the language session is unsupported."
                .to_owned(),
        );
    }
    if !capabilities.recovery_mode
        || !capabilities.editor
        || !capabilities.headless
        || !capabilities.project_path
    {
        return Some(
            "The selected Godot version does not advertise the required --recovery-mode, --editor, --headless, and --path options; the language session is unsupported."
                .to_owned(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        GODOT_LSP_MIRROR_PATH_MARKER, GODOT_LSP_PORT_MARKER,
        GODOT_LSP_UNAVAILABLE_MESSAGE, GodotLspSessionCommandDigestParts,
        GodotLspStartOutcome, compute_godot_lsp_session_command_digest,
        create_godot_lsp_server_runner, godot_lsp_argument_template,
        godot_lsp_arguments,
    };
    use crate::godot::process::recovery_runner::test_support::{
        engine_profile, installation,
    };
    use siralos_core::godot::{GodotCapabilityKey, GodotEdition};

    fn full_capabilities() -> siralos_core::godot::GodotCommandCapabilities {
        let mut capabilities =
            siralos_core::godot::empty_godot_command_capabilities();
        for key in [
            GodotCapabilityKey::Lsp,
            GodotCapabilityKey::RecoveryMode,
            GodotCapabilityKey::Editor,
            GodotCapabilityKey::Headless,
            GodotCapabilityKey::ProjectPath,
        ] {
            key.apply(&mut capabilities, true);
        }
        capabilities
    }

    #[test]
    fn template_pairs_lsp_port_with_recovery_tuple_only() {
        assert_eq!(
            godot_lsp_argument_template(),
            [
                "--headless",
                "--editor",
                "--recovery-mode",
                "--path",
                GODOT_LSP_MIRROR_PATH_MARKER,
                "--lsp-port",
                GODOT_LSP_PORT_MARKER
            ]
        );
        let real = godot_lsp_arguments("C:\\mirror", 6006);
        assert_eq!(real[5], "--lsp-port");
        assert_eq!(real[6], "6006");
        for argument in godot_lsp_argument_template() {
            assert!(
                ![
                    "--scene",
                    "--script",
                    "--import",
                    "--check-only",
                    "--dap-port",
                    "--debug-server",
                    "--quit",
                    "--export"
                ]
                .contains(&argument.as_str())
            );
        }
    }

    #[test]
    fn digest_binds_the_loopback_policy_and_inputs() {
        let parts = || GodotLspSessionCommandDigestParts {
            executable_sha256: "a".repeat(64),
            argument_template: godot_lsp_argument_template(),
            profile_id: "godot-lsp-local".to_owned(),
            timeout_ms: 30_000,
            stdout_limit_bytes: 1024 * 1024,
            stderr_limit_bytes: 1024 * 1024,
        };
        let base = compute_godot_lsp_session_command_digest(&parts());
        assert_eq!(base, compute_godot_lsp_session_command_digest(&parts()));
        let changed = GodotLspSessionCommandDigestParts {
            profile_id: "other".to_owned(),
            ..parts()
        };
        assert_ne!(compute_godot_lsp_session_command_digest(&changed), base);
    }

    #[test]
    fn reports_unavailable_without_spawning_or_opening_ports() {
        let runner = create_godot_lsp_server_runner();
        assert!(!runner.is_available());
        let valid_installation = installation(true);
        let mut profile = engine_profile(GodotEdition::Standard);
        profile.capabilities = full_capabilities();
        let outcome = runner
            .start_server(super::GodotLspStartRequest {
                installation: &valid_installation,
                engine_profile: &profile,
                cancelled: false,
            })
            .expect("not cancelled");
        assert_eq!(
            outcome,
            GodotLspStartOutcome::Unavailable {
                message: GODOT_LSP_UNAVAILABLE_MESSAGE.to_owned()
            }
        );
    }

    #[test]
    fn missing_lsp_capability_is_unsupported() {
        let runner = create_godot_lsp_server_runner();
        let valid_installation = installation(true);
        let mut profile = engine_profile(GodotEdition::Standard);
        profile.capabilities = full_capabilities();
        GodotCapabilityKey::Lsp.apply(&mut profile.capabilities, false);
        let outcome = runner
            .start_server(super::GodotLspStartRequest {
                installation: &valid_installation,
                engine_profile: &profile,
                cancelled: false,
            })
            .expect("not cancelled");
        assert_eq!(
            outcome,
            GodotLspStartOutcome::Unsupported {
                message:
                    "The selected Godot version does not advertise --lsp-port; the language session is unsupported."
                        .to_owned()
            }
        );
    }

    #[test]
    fn runtime_only_engines_are_unsupported_and_cancellation_propagates() {
        let runner = create_godot_lsp_server_runner();
        let valid_installation = installation(true);
        let runtime_only = engine_profile(GodotEdition::RuntimeOnly);
        let outcome = runner
            .start_server(super::GodotLspStartRequest {
                installation: &valid_installation,
                engine_profile: &runtime_only,
                cancelled: false,
            })
            .expect("not cancelled");
        assert!(matches!(outcome, GodotLspStartOutcome::Unsupported { .. }));
        let standard = engine_profile(GodotEdition::Standard);
        let error = runner
            .start_server(super::GodotLspStartRequest {
                installation: &valid_installation,
                engine_profile: &standard,
                cancelled: true,
            })
            .unwrap_err();
        assert_eq!(
            error.message,
            "The Godot language session startup was aborted."
        );
    }
}
