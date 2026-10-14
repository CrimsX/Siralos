//! Host sandbox backend over the core [`SandboxBackend`] port.
//!
//! The backend answers the two questions honestly: the live probe
//! reports what enforcement exists (none is pinned in this
//! repository, so no probe passes), and `execute` re-checks the
//! probe before it would spawn anything, refusing without a child
//! process when enforcement is unproven. No `Command::spawn` exists
//! in this module. This module also owns the pure execution
//! contracts the tool shares: the explicit environment allowlist
//! builder (never the parent environment verbatim) and bounded
//! stream truncation with explicit flags.

use siralos_core::provider::CancellationSignal;
use siralos_core::runtime::sandbox::{
    SandboxAvailability, SandboxBackend, SandboxCommand, SandboxError,
    SandboxLimits, SandboxOutput, SandboxUnavailableReason,
};

/// Parent environment names kept for the child, matched
/// case-insensitively (Windows names are case-insensitive; the
/// conservative match applies everywhere).
const SANDBOX_KEPT_ENV: &[&str] = &[
    "PATH",
    "SystemRoot",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "TEMP",
    "TMP",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "TERM",
];

/// Siralos-controlled values that always win over the parent.
const SANDBOX_FIXED_ENV: &[(&str, &str)] = &[
    ("NO_COLOR", "1"),
    ("FORCE_COLOR", "0"),
    ("TERM", "dumb"),
    ("GIT_TERMINAL_PROMPT", "0"),
];

/// Case-insensitive name equality for environment matching.
fn env_name_eq(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

/// Whether an environment name is denied under the SECURITY.md
/// credential-isolation contract: secret-shaped suffixes,
/// provider/cloud prefixes, repository-control names, proxy names,
/// editor-path overrides, and loader-injection names. Denial wins
/// over the keep list.
#[must_use]
pub fn is_denied_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    for suffix in ["_API_KEY", "_TOKEN", "_SECRET", "_PASSWORD"] {
        if upper.ends_with(suffix) {
            return true;
        }
    }
    for prefix in [
        "AWS_",
        "AZURE_",
        "GOOGLE_",
        "GIT_",
        "NPM_CONFIG",
        "DYLD_",
        "LD_",
        "GODOT",
    ] {
        if upper.starts_with(prefix) {
            return true;
        }
    }
    matches!(
        upper.as_str(),
        "GH_TOKEN"
            | "SSH_AUTH_SOCK"
            | "NPM_TOKEN"
            | "NODE_AUTH_TOKEN"
            | "SIRALOS_CONFIG"
            | "NODE_OPTIONS"
            | "BASH_ENV"
            | "ENV"
            | "CDPATH"
            | "HTTP_PROXY"
            | "HTTPS_PROXY"
            | "ALL_PROXY"
            | "NO_PROXY"
    )
}

/// Build the exact child environment: kept parent names minus
/// denied names, then caller-controlled overrides (sandbox-private
/// home/temp), then the fixed Siralos values. Later layers replace
/// same-name entries; nothing else survives. Pure over injected
/// pairs, so tests never touch the real process environment.
#[must_use]
pub fn build_sandbox_env(
    parent: &[(String, String)],
    controlled: &[(String, String)],
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    let mut place = |name: String, value: String| {
        if let Some(slot) =
            env.iter_mut().find(|(kept, _)| env_name_eq(kept, &name))
        {
            slot.1 = value;
        } else {
            env.push((name, value));
        }
    };
    for (name, value) in parent {
        if is_denied_env_name(name) {
            continue;
        }
        if SANDBOX_KEPT_ENV.iter().any(|kept| env_name_eq(kept, name)) {
            place(name.clone(), value.clone());
        }
    }
    for (name, value) in controlled {
        if is_denied_env_name(name) {
            continue;
        }
        place(name.clone(), value.clone());
    }
    for (name, value) in SANDBOX_FIXED_ENV {
        place((*name).to_owned(), (*value).to_owned());
    }
    env
}

/// Cut one stream to its cap, flagging truncation. Byte-exact: the
/// caller reports the flag, never a silent prefix.
#[must_use]
pub fn truncate_bounded(bytes: &[u8], max_bytes: usize) -> (Vec<u8>, bool) {
    if bytes.len() <= max_bytes {
        (bytes.to_vec(), false)
    } else {
        (bytes[..max_bytes].to_vec(), true)
    }
}

/// The host backend. Probing is real (it inspects the pinned
/// enforcement) and honestly negative: this repository pins no
/// enforcing backend, so no probe passes on any platform.
pub struct HostSandboxBackend;

impl SandboxBackend for HostSandboxBackend {
    fn probe(&self) -> SandboxAvailability {
        // No backend binary or OS facility is pinned and driven by
        // this repository, so per-execution filesystem grants and
        // the host-read allowlist cannot be expressed. The probe
        // reports exactly that instead of passing an unobserved
        // enforcement.
        SandboxAvailability::Unavailable(
            SandboxUnavailableReason::EnforcementUnsupported,
        )
    }

    fn execute(
        &self,
        _command: &SandboxCommand,
        _limits: &SandboxLimits,
        _cancellation: CancellationSignal<'_>,
    ) -> Result<SandboxOutput, SandboxError> {
        // Defense in depth: the tool path maps unavailability
        // before calling here, but a direct caller without a
        // passing probe is refused without spawning anything.
        match self.probe() {
            SandboxAvailability::Available(_) => {
                Err(SandboxError::ExecutionFailed)
            }
            SandboxAvailability::Unavailable(_) => {
                Err(SandboxError::BackendUnavailable)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HostSandboxBackend, build_sandbox_env, is_denied_env_name,
        truncate_bounded,
    };
    use siralos_core::provider::CancellationToken;
    use siralos_core::runtime::sandbox::{
        SandboxAvailability, SandboxBackend, SandboxCommand, SandboxLimits,
        SandboxUnavailableReason,
    };

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn denied_env_names_cover_the_credential_contract() {
        for secret in [
            "SIRALOS_TEST_API_KEY",
            "github_token",
            "NPM_TOKEN",
            "AWS_REGION",
            "GIT_DIR",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "HTTP_PROXY",
            "NODE_OPTIONS",
            "GODOT_EDITOR_PATH",
            "GODOT4_EDITOR_PATH",
            "BASH_ENV",
        ] {
            assert!(is_denied_env_name(secret), "{secret}");
        }
        for innocent in ["PATH", "HOME", "LANG", "MY_APP_MODE"] {
            assert!(!is_denied_env_name(innocent), "{innocent}");
        }
    }

    #[test]
    fn env_builder_keeps_allowlist_drops_secrets_applies_controlled() {
        let parent = [
            pair("PATH", "/bin"),
            pair("TERM", "xterm"),
            pair("SIRALOS_TEST_API_KEY", "canary"),
            pair("UNRELATED_VAR", "dropped"),
        ];
        let controlled = [pair("HOME", "/private/home")];
        let env = build_sandbox_env(&parent, &controlled);
        let get = |name: &str| {
            env.iter().find(|(kept, _)| kept == name).map(|(_, v)| v)
        };
        assert_eq!(get("PATH").map(String::as_str), Some("/bin"));
        // Siralos-controlled values always win, even over kept names.
        assert_eq!(get("TERM").map(String::as_str), Some("dumb"));
        assert_eq!(get("HOME").map(String::as_str), Some("/private/home"));
        assert_eq!(get("NO_COLOR").map(String::as_str), Some("1"));
        assert!(get("SIRALOS_TEST_API_KEY").is_none());
        assert!(get("UNRELATED_VAR").is_none());
        // A denied controlled name cannot smuggle itself in either.
        let smuggled = build_sandbox_env(&[], &[pair("AWS_KEY", "x")]);
        assert!(smuggled.iter().all(|(name, _)| name != "AWS_KEY"));
    }

    #[test]
    fn truncation_flags_every_cut_and_nothing_else() {
        assert_eq!(truncate_bounded(b"", 4), (b"".to_vec(), false));
        assert_eq!(truncate_bounded(b"abc", 3), (b"abc".to_vec(), false));
        assert_eq!(truncate_bounded(b"abcd", 3), (b"abc".to_vec(), true));
        assert_eq!(truncate_bounded(b"a", 0), (b"".to_vec(), true));
        assert_eq!(truncate_bounded(b"", 0), (b"".to_vec(), false));
    }

    #[test]
    fn probe_reports_typed_unavailability_without_side_effects() {
        let backend = HostSandboxBackend;
        assert_eq!(
            backend.probe(),
            SandboxAvailability::Unavailable(
                SandboxUnavailableReason::EnforcementUnsupported
            )
        );
    }

    #[test]
    fn execute_without_proven_enforcement_spawns_nothing() {
        // The command would create this file if it ever ran: its
        // absence after the refused call proves no spawn happened.
        let marker = std::env::temp_dir().join(format!(
            "siralos-bash-never-{}-{}",
            std::process::id(),
            "spawned.txt"
        ));
        let _ = std::fs::remove_file(&marker);
        let backend = HostSandboxBackend;
        let error = backend
            .execute(
                &SandboxCommand {
                    program: "sh".to_owned(),
                    args: vec![
                        "-c".to_owned(),
                        format!("touch {}", marker.display()),
                    ],
                    env: Vec::new(),
                    workdir: std::env::temp_dir(),
                },
                &SandboxLimits::default_tool_limits(),
                CancellationToken::new().signal(),
            )
            .expect_err("unproven enforcement must refuse");
        assert_eq!(
            error,
            siralos_core::runtime::sandbox::SandboxError::BackendUnavailable
        );
        assert!(
            !marker.exists(),
            "a refused execution must leave no child effects"
        );
    }
}
