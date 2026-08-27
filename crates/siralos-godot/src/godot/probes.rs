//! Narrow fixed-probe interface and typed probe outcomes.
//!
//! No arbitrary argument array, provider-controlled argument, project
//! path, or working directory is accepted: the adapter chooses every
//! argument and every working directory. Probes always run through the
//! sandbox backend. Provider adapters cannot invoke a runner directly;
//! only the Godot probe adapter implements this trait and only Siralos
//! composition consumes it.

use super::capabilities::GodotCommandCapabilities;
use super::installations::GodotInstallation;
use super::version::GodotVersion;

/// Outcome of a `--version` probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotVersionProbe {
    /// The executable reported its version.
    Success {
        /// Exact parsed version.
        version: GodotVersion,
    },
    /// Probing cannot execute under the current enforcement boundary.
    Unavailable {
        /// Bounded truthful reason.
        message: String,
    },
    /// The probe ran and failed.
    Failed {
        /// Bounded truthful reason.
        message: String,
    },
}

/// Outcome of a `--help` probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotHelpProbe {
    /// The executable advertised its options.
    Success {
        /// Parsed advertised capabilities.
        capabilities: GodotCommandCapabilities,
        /// Count of unrecognized options, preserved as a bounded diagnostic.
        unknown_option_count: u64,
    },
    /// Probing cannot execute under the current enforcement boundary.
    Unavailable {
        /// Bounded truthful reason.
        message: String,
    },
    /// The probe ran with degraded output; capabilities may be partial.
    Degraded {
        /// Bounded truthful reason.
        message: String,
        /// Parsed advertised capabilities.
        capabilities: GodotCommandCapabilities,
        /// Count of unrecognized options, preserved as a bounded diagnostic.
        unknown_option_count: u64,
    },
    /// The probe ran and failed.
    Failed {
        /// Bounded truthful reason.
        message: String,
    },
}

/// Bounded summary of an extension API dump; never the dump itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpSummary {
    /// Dump header version, when present.
    pub header_version: Option<String>,
    /// API hash, when present.
    pub api_hash: Option<String>,
    /// Class count, when present.
    pub class_count: Option<u64>,
    /// Builtin class count, when present.
    pub builtin_class_count: Option<u64>,
    /// Global enum count, when present.
    pub global_enum_count: Option<u64>,
    /// Utility function count, when present.
    pub utility_function_count: Option<u64>,
    /// Configuration version, when present.
    pub configuration_version: Option<u64>,
    /// Dump size in bytes.
    pub file_size_bytes: u64,
    /// SHA-256 of the dump bytes.
    pub sha256: String,
}

/// Outcome of an extension-API dump probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotApiDumpProbe {
    /// The dump was produced and summarized.
    Success {
        /// Bounded summary.
        summary: GodotApiDumpSummary,
    },
    /// Probing cannot execute under the current enforcement boundary.
    Unavailable {
        /// Bounded truthful reason.
        message: String,
    },
    /// The probe ran with degraded output.
    Degraded {
        /// Bounded truthful reason.
        message: String,
    },
    /// The probe ran and failed.
    Failed {
        /// Bounded truthful reason.
        message: String,
    },
}

/// Fixed-probe runner owned by core and implemented by the Godot probe
/// adapter.
///
/// When the enforcement boundary cannot bind a sandboxed launch to the
/// exact fingerprinted executable bytes, implementations report
/// unavailable and never spawn the executable.
pub trait GodotProbeRunner {
    /// Reports whether engine probing can execute at all.
    fn is_available(&self) -> bool;

    /// Probe `--version` for one installation.
    fn probe_version(
        &self,
        installation: &GodotInstallation,
    ) -> GodotVersionProbe;

    /// Probe `--help` for one installation.
    fn probe_help(&self, installation: &GodotInstallation) -> GodotHelpProbe;

    /// Probe `--dump-extension-api` for one installation.
    fn dump_extension_api(
        &self,
        installation: &GodotInstallation,
    ) -> GodotApiDumpProbe;
}

#[cfg(test)]
mod tests {
    use super::{
        GodotApiDumpProbe, GodotHelpProbe, GodotProbeRunner, GodotVersionProbe,
    };
    use crate::godot::installations::{
        GodotEditionHint, GodotInstallation, GodotInstallationSource,
    };

    fn installation() -> GodotInstallation {
        GodotInstallation {
            id: "probe-test".to_owned(),
            source_label: "user config".to_owned(),
            source: GodotInstallationSource::UserConfig,
            canonical_path: "C:\\godot\\Godot.exe".to_owned(),
            size_bytes: 1000,
            modified_at_ms: 0,
            sha256: "a".repeat(64),
            edition_hint: GodotEditionHint::Standard,
            status_valid: true,
            error: None,
        }
    }

    struct NeverAvailableRunner;

    impl GodotProbeRunner for NeverAvailableRunner {
        fn is_available(&self) -> bool {
            false
        }

        fn probe_version(
            &self,
            _installation: &GodotInstallation,
        ) -> GodotVersionProbe {
            GodotVersionProbe::Unavailable {
                message: "unavailable".to_owned(),
            }
        }

        fn probe_help(
            &self,
            _installation: &GodotInstallation,
        ) -> GodotHelpProbe {
            GodotHelpProbe::Unavailable { message: "unavailable".to_owned() }
        }

        fn dump_extension_api(
            &self,
            _installation: &GodotInstallation,
        ) -> GodotApiDumpProbe {
            GodotApiDumpProbe::Unavailable {
                message: "unavailable".to_owned(),
            }
        }
    }

    #[test]
    fn trait_contract_accepts_fail_closed_runner() {
        let runner = NeverAvailableRunner;
        assert!(!runner.is_available());
        let installation = installation();
        assert!(matches!(
            runner.probe_version(&installation),
            GodotVersionProbe::Unavailable { .. }
        ));
        assert!(matches!(
            runner.probe_help(&installation),
            GodotHelpProbe::Unavailable { .. }
        ));
        assert!(matches!(
            runner.dump_extension_api(&installation),
            GodotApiDumpProbe::Unavailable { .. }
        ));
    }
}
