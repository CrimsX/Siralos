//! The Godot engine probe runner fails closed and never spawns the
//! executable.
//!
//! The required invariant — the executable opened by the OS must be the
//! exact object whose bytes produced the trusted SHA-256 fingerprint —
//! cannot be enforced when process launch re-opens the staged copy's
//! pathname and a same-user adversary can substitute it between final
//! verification and launch. Re-checking after launch is not prevention.
//! Rather than weakening the same-user threat model to keep probes
//! available, every probe reports unavailable and the executable is
//! never spawned.

use siralos_core::godot::{
    GodotApiDumpProbe, GodotHelpProbe, GodotInstallation, GodotProbeRunner,
    GodotVersionProbe,
};

/// Truthful reason reported by every probe while launch cannot be bound
/// to the fingerprinted bytes.
pub const GODOT_PROBING_UNAVAILABLE_MESSAGE: &str = "Godot engine probing is unavailable: Node and the pinned sandbox runtime offer no identity-bound launch primitive, so the staged executable copy's pathname is re-opened at spawn time and a same-user process can substitute different bytes between final verification and launch. The verified fingerprint could then be attached to bytes that never execute. Probing fails closed and the executable is never spawned; it will become available when a mechanically identity-bound launch primitive exists.";

/// The fail-closed probe runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailClosedGodotProbeRunner;

impl GodotProbeRunner for FailClosedGodotProbeRunner {
    fn is_available(&self) -> bool {
        false
    }

    fn probe_version(
        &self,
        _installation: &GodotInstallation,
    ) -> GodotVersionProbe {
        GodotVersionProbe::Unavailable {
            message: GODOT_PROBING_UNAVAILABLE_MESSAGE.to_owned(),
        }
    }

    fn probe_help(&self, _installation: &GodotInstallation) -> GodotHelpProbe {
        GodotHelpProbe::Unavailable {
            message: GODOT_PROBING_UNAVAILABLE_MESSAGE.to_owned(),
        }
    }

    fn dump_extension_api(
        &self,
        _installation: &GodotInstallation,
    ) -> GodotApiDumpProbe {
        GodotApiDumpProbe::Unavailable {
            message: GODOT_PROBING_UNAVAILABLE_MESSAGE.to_owned(),
        }
    }
}

/// Create the fail-closed probe runner.
pub fn create_godot_probe_runner() -> FailClosedGodotProbeRunner {
    FailClosedGodotProbeRunner
}

#[cfg(test)]
mod tests {
    use super::{
        GODOT_PROBING_UNAVAILABLE_MESSAGE, create_godot_probe_runner,
    };
    use siralos_core::godot::{
        GodotApiDumpProbe, GodotHelpProbe, GodotInstallation,
        GodotInstallationSource, GodotProbeRunner, GodotVersionProbe,
        InstallEditionHint,
    };

    #[test]
    fn reports_probing_unavailable() {
        let runner = create_godot_probe_runner();
        assert!(!runner.is_available());
    }

    #[test]
    fn every_probe_reports_the_same_typed_unavailable() {
        let runner = create_godot_probe_runner();
        let installation = GodotInstallation {
            id: "probe-test".to_owned(),
            source_label: "user config".to_owned(),
            source: GodotInstallationSource::UserConfig,
            canonical_path: "C:\\godot\\Godot.exe".to_owned(),
            size_bytes: 1000,
            modified_at_ms: 0,
            sha256: "a".repeat(64),
            edition_hint: InstallEditionHint::Standard,
            status_valid: true,
            error: None,
        };
        let version = runner.probe_version(&installation);
        assert_eq!(
            version,
            GodotVersionProbe::Unavailable {
                message: GODOT_PROBING_UNAVAILABLE_MESSAGE.to_owned()
            }
        );
        let help = runner.probe_help(&installation);
        assert!(matches!(help, GodotHelpProbe::Unavailable { .. }));
        let api = runner.dump_extension_api(&installation);
        assert!(matches!(api, GodotApiDumpProbe::Unavailable { .. }));
    }

    #[test]
    fn message_states_the_substitution_boundary() {
        assert!(GODOT_PROBING_UNAVAILABLE_MESSAGE.contains("substitute"));
        assert!(GODOT_PROBING_UNAVAILABLE_MESSAGE.contains("never spawned"));
    }
}
