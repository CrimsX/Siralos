//! GDScript check-only execution fails closed and never spawns the
//! executable.
//!
//! The fixed invocation would run `--headless --path <disposable-mirror>
//! --script <mirror-script> --check-only` inside an enforcing sandbox
//! with the workspace excluded from readable roots, stdin closed, and
//! network denied; until launch and mirror lifecycle can be mechanically
//! bound to verified identities, every check reports a typed
//! `unavailable` outcome with zero filesystem side effects.
//!
//! The only legitimate `--script` invocation in Siralos is this check-only
//! diagnostic adapter. `--check-only` is the security-relevant invariant:
//! Godot parses the script and reports diagnostics without executing
//! gameplay, scenes, or scripts. If the selected engine does not advertise
//! `--check-only`, the check refuses as unsupported and the script is
//! never run normally.

use siralos_godot::godot::{
    GodotEdition, GodotEngineProfile, GodotInstallation,
};

/// Marker for the disposable mirror project path (never a real path).
pub const GODOT_CHECK_ONLY_MIRROR_PATH_MARKER: &str = "<disposable-mirror>";

/// Marker for the mirrored script path (never a real path).
pub const GODOT_CHECK_ONLY_MIRROR_SCRIPT_MARKER: &str = "<mirror-script>";

/// Fixed Siralos-owned GDScript check-only argument template.
pub const GODOT_CHECK_ONLY_BASE_ARGUMENTS: [&str; 6] = [
    "--headless",
    "--path",
    GODOT_CHECK_ONLY_MIRROR_PATH_MARKER,
    "--script",
    GODOT_CHECK_ONLY_MIRROR_SCRIPT_MARKER,
    "--check-only",
];

/// The fixed argument template, used for command digests.
///
/// Digesting the markers (never absolute paths) keeps the template stable
/// across runs and keeps absolute mirror paths out of every digest and
/// event.
pub fn godot_check_only_argument_template() -> Vec<String> {
    GODOT_CHECK_ONLY_BASE_ARGUMENTS
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect()
}

/// The invocation arguments for one check-only run against the mirror.
pub fn godot_check_only_arguments(
    mirror_project_path: &str,
    mirror_script_path: &str,
) -> Vec<String> {
    vec![
        "--headless".to_owned(),
        "--path".to_owned(),
        mirror_project_path.to_owned(),
        "--script".to_owned(),
        mirror_script_path.to_owned(),
        "--check-only".to_owned(),
    ]
}

/// The check was cancelled before refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotCheckOnlyCancelled {
    /// Bounded cancellation message.
    pub message: String,
}

/// Observable outcomes of the fail-closed check-only runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotCheckOnlyRunOutcome {
    /// The engine cannot parse scripts as specified.
    Unsupported {
        /// Bounded truthful reason.
        message: String,
    },
    /// Diagnostics are unavailable under the current enforcement boundary.
    Unavailable {
        /// Bounded truthful reason.
        message: String,
    },
}

/// Inputs to one check-only attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotCheckOnlyRunRequest<'a> {
    /// Selected installation.
    pub installation: &'a GodotInstallation,
    /// Selected engine profile.
    pub engine_profile: &'a GodotEngineProfile,
    /// Host-owned cancellation observation; cancelled checks refuse
    /// before any precondition evaluation.
    pub cancelled: bool,
}

/// Truthful reason reported for every check while launch and mirror
/// lifecycle cannot be bound to verified objects.
pub const GODOT_CHECK_ONLY_UNAVAILABLE_MESSAGE: &str = "GDScript check-only diagnostics are unavailable on this platform: Node and the pinned sandbox runtime offer no exec-by-handle, directory-relative create, or delete-by-handle primitive, so the approved Godot identity cannot be launched against exactly the approved mirrored script bytes and the disposable mirror cannot be constructed or cleaned up identity-bound. Diagnostics fail closed and the engine is never spawned; no mirror is created and nothing is executed.";

/// The fail-closed check-only runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailClosedGodotCheckOnlyRunner;

impl FailClosedGodotCheckOnlyRunner {
    /// Reports whether check-only diagnostics can run at all.
    #[must_use]
    pub fn is_available(&self) -> bool {
        false
    }

    /// Refuse the check without spawning anything or creating a mirror.
    pub fn run_check_only(
        &self,
        request: GodotCheckOnlyRunRequest<'_>,
    ) -> Result<GodotCheckOnlyRunOutcome, GodotCheckOnlyCancelled> {
        if request.cancelled {
            return Err(GodotCheckOnlyCancelled {
                message: "The GDScript check was aborted.".to_owned(),
            });
        }
        if let Some(message) = require_check_only_capabilities(request) {
            return Ok(GodotCheckOnlyRunOutcome::Unsupported { message });
        }
        Ok(GodotCheckOnlyRunOutcome::Unavailable {
            message: GODOT_CHECK_ONLY_UNAVAILABLE_MESSAGE.to_owned(),
        })
    }
}

/// Create the fail-closed check-only runner.
pub fn create_godot_check_only_runner() -> FailClosedGodotCheckOnlyRunner {
    FailClosedGodotCheckOnlyRunner
}

fn require_check_only_capabilities(
    request: GodotCheckOnlyRunRequest<'_>,
) -> Option<String> {
    if !request.installation.status_valid {
        return Some(
            "The installation is invalid; rediscovery is required.".to_owned(),
        );
    }
    if request.engine_profile.edition == GodotEdition::RuntimeOnly {
        return Some(
            "The selected executable is runtime-only; it cannot parse GDScript."
                .to_owned(),
        );
    }
    let capabilities = &request.engine_profile.capabilities;
    if !capabilities.check_only {
        return Some(
            "The selected Godot version does not advertise --check-only; GDScript diagnostics are unsupported and the script is never run normally."
                .to_owned(),
        );
    }
    if !capabilities.script || !capabilities.headless {
        return Some(
            "The selected Godot version does not advertise --script and --headless; GDScript diagnostics are unsupported."
                .to_owned(),
        );
    }
    if !capabilities.project_path {
        return Some(
            "The selected Godot version does not advertise --path; the mirror project cannot be opened for diagnostics."
                .to_owned(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        GODOT_CHECK_ONLY_BASE_ARGUMENTS, GODOT_CHECK_ONLY_MIRROR_PATH_MARKER,
        GODOT_CHECK_ONLY_MIRROR_SCRIPT_MARKER,
        GODOT_CHECK_ONLY_UNAVAILABLE_MESSAGE, GodotCheckOnlyRunOutcome,
        create_godot_check_only_runner, godot_check_only_argument_template,
        godot_check_only_arguments,
    };
    use crate::godot::process::recovery_runner::test_support::{
        engine_profile, installation,
    };
    use siralos_godot::godot::{GodotCapabilityKey, GodotEdition};

    #[test]
    fn template_pairs_script_with_check_only_only() {
        assert_eq!(
            godot_check_only_argument_template(),
            [
                "--headless",
                "--path",
                GODOT_CHECK_ONLY_MIRROR_PATH_MARKER,
                "--script",
                GODOT_CHECK_ONLY_MIRROR_SCRIPT_MARKER,
                "--check-only"
            ]
        );
        assert_eq!(
            GODOT_CHECK_ONLY_BASE_ARGUMENTS.len(),
            godot_check_only_argument_template().len()
        );
        let real =
            godot_check_only_arguments("C:\\mirror", "C:\\mirror\\x.gd");
        assert_eq!(real[2], "C:\\mirror");
        assert_eq!(real[4], "C:\\mirror\\x.gd");
        assert!(real.contains(&"--check-only".to_owned()));
    }

    #[test]
    fn reports_unavailable_without_launching() {
        let runner = create_godot_check_only_runner();
        assert!(!runner.is_available());
        let valid_installation = installation(true);
        let mut profile = engine_profile(GodotEdition::Standard);
        for key in [
            GodotCapabilityKey::CheckOnly,
            GodotCapabilityKey::Script,
            GodotCapabilityKey::Headless,
            GodotCapabilityKey::ProjectPath,
        ] {
            key.apply(&mut profile.capabilities, true);
        }
        let outcome = runner
            .run_check_only(super::GodotCheckOnlyRunRequest {
                installation: &valid_installation,
                engine_profile: &profile,
                cancelled: false,
            })
            .expect("not cancelled");
        assert_eq!(
            outcome,
            GodotCheckOnlyRunOutcome::Unavailable {
                message: GODOT_CHECK_ONLY_UNAVAILABLE_MESSAGE.to_owned()
            }
        );
    }

    #[test]
    fn missing_check_only_capability_is_unsupported_never_normal_run() {
        let runner = create_godot_check_only_runner();
        let valid_installation = installation(true);
        let mut profile = engine_profile(GodotEdition::Standard);
        GodotCapabilityKey::Script.apply(&mut profile.capabilities, true);
        GodotCapabilityKey::Headless.apply(&mut profile.capabilities, true);
        GodotCapabilityKey::ProjectPath.apply(&mut profile.capabilities, true);
        let outcome = runner
            .run_check_only(super::GodotCheckOnlyRunRequest {
                installation: &valid_installation,
                engine_profile: &profile,
                cancelled: false,
            })
            .expect("not cancelled");
        assert_eq!(
            outcome,
            GodotCheckOnlyRunOutcome::Unsupported {
                message: "The selected Godot version does not advertise --check-only; GDScript diagnostics are unsupported and the script is never run normally."
                    .to_owned()
            }
        );
    }

    #[test]
    fn refuses_cancelled_checks_before_preconditions() {
        let runner = create_godot_check_only_runner();
        let valid_installation = installation(true);
        let profile = engine_profile(GodotEdition::Standard);
        let error = runner
            .run_check_only(super::GodotCheckOnlyRunRequest {
                installation: &valid_installation,
                engine_profile: &profile,
                cancelled: true,
            })
            .unwrap_err();
        assert_eq!(error.message, "The GDScript check was aborted.");
    }
}
