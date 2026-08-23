//! Deterministic fault-injection harness (Stage 3 — Runtime Readiness &
//! Operational Resilience, ADR 0031; R10c H3).
//!
//! Mirrors `packages/core/src/runtime/faults.ts`. Fake process drivers
//! simulate the full failure taxonomy under the H2 controlled clock; no
//! real project or process is needed. The same FaultScript + clock
//! always produces the same observation sequence, so supervision
//! outcomes are reproducible. Observations are pure functions of the
//! script and the controlled clock.

use super::supervision::{
    LivenessKind, RuntimeFailureKind, SupervisorObservation,
};

/// The closed fault-script vocabulary (14 scripts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultScript {
    /// Startup completes; clean exit after 5s.
    Normal,
    /// Spawn fails immediately.
    SpawnFailure,
    /// Sandbox denies startup.
    SandboxDenied,
    /// Startup never reports completion.
    StartupHang,
    /// Runs but never emits output activity.
    IdleHang,
    /// Keeps emitting output until the host ends it.
    HardTimeout,
    /// Cancellation arrives before startup completes.
    CancelDuringStartup,
    /// Cancellation arrives while running.
    CancelWhileRunning,
    /// Clean start; nonzero exit after 3s.
    Crash,
    /// Refuses termination when asked.
    ChildRefusesTermination,
    /// Emits output activity on every tick.
    OutputFlood,
    /// Clean exit after 1s under artifact quota pressure.
    ArtifactQuota,
    /// Simulates a cleanup failure after a successful run.
    CleanupFailure,
    /// A run record left non-terminal by a restart.
    RestartIncomplete,
}

impl FaultScript {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::SpawnFailure => "spawn_failure",
            Self::SandboxDenied => "sandbox_denied",
            Self::StartupHang => "startup_hang",
            Self::IdleHang => "idle_hang",
            Self::HardTimeout => "hard_timeout",
            Self::CancelDuringStartup => "cancel_during_startup",
            Self::CancelWhileRunning => "cancel_while_running",
            Self::Crash => "crash",
            Self::ChildRefusesTermination => "child_refuses_termination",
            Self::OutputFlood => "output_flood",
            Self::ArtifactQuota => "artifact_quota",
            Self::CleanupFailure => "cleanup_failure",
            Self::RestartIncomplete => "restart_incomplete",
        }
    }

    /// Parse a protocol string; unknown scripts are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "spawn_failure" => Some(Self::SpawnFailure),
            "sandbox_denied" => Some(Self::SandboxDenied),
            "startup_hang" => Some(Self::StartupHang),
            "idle_hang" => Some(Self::IdleHang),
            "hard_timeout" => Some(Self::HardTimeout),
            "cancel_during_startup" => Some(Self::CancelDuringStartup),
            "cancel_while_running" => Some(Self::CancelWhileRunning),
            "crash" => Some(Self::Crash),
            "child_refuses_termination" => Some(Self::ChildRefusesTermination),
            "output_flood" => Some(Self::OutputFlood),
            "artifact_quota" => Some(Self::ArtifactQuota),
            "cleanup_failure" => Some(Self::CleanupFailure),
            "restart_incomplete" => Some(Self::RestartIncomplete),
            _ => None,
        }
    }
}

use crate::determinism::stable_sort_by_key;

/// All fault scripts in oracle declaration order.
pub const FAULT_SCRIPTS: [FaultScript; 14] = [
    FaultScript::Normal,
    FaultScript::SpawnFailure,
    FaultScript::SandboxDenied,
    FaultScript::StartupHang,
    FaultScript::IdleHang,
    FaultScript::HardTimeout,
    FaultScript::CancelDuringStartup,
    FaultScript::CancelWhileRunning,
    FaultScript::Crash,
    FaultScript::ChildRefusesTermination,
    FaultScript::OutputFlood,
    FaultScript::ArtifactQuota,
    FaultScript::CleanupFailure,
    FaultScript::RestartIncomplete,
];

/// Stable ordered fault-script listing (deterministic id order).
#[must_use]
pub fn list_fault_scripts() -> Vec<FaultScript> {
    stable_sort_by_key(&FAULT_SCRIPTS, |script| script.as_str().to_owned())
}

/// Deterministic failure kind expected for a fault script.
/// `cleanup_failed` is the INDEPENDENT cleanup status, never a terminal
/// execution disposition, so `cleanup_failure` expects no terminal kind.
#[must_use]
pub fn expected_failure_kind(
    script: FaultScript,
) -> Option<RuntimeFailureKind> {
    match script {
        FaultScript::SpawnFailure => Some(RuntimeFailureKind::SpawnFailed),
        FaultScript::SandboxDenied => Some(RuntimeFailureKind::SandboxDenied),
        FaultScript::StartupHang => Some(RuntimeFailureKind::StartupTimeout),
        FaultScript::IdleHang => Some(RuntimeFailureKind::IdleTimeout),
        FaultScript::HardTimeout => Some(RuntimeFailureKind::HardTimeout),
        FaultScript::CancelDuringStartup | FaultScript::CancelWhileRunning => {
            Some(RuntimeFailureKind::Cancelled)
        }
        FaultScript::Crash => Some(RuntimeFailureKind::ProcessCrashed),
        FaultScript::ChildRefusesTermination => {
            Some(RuntimeFailureKind::KillFailed)
        }
        FaultScript::OutputFlood => Some(RuntimeFailureKind::OutputLimit),
        FaultScript::ArtifactQuota => Some(RuntimeFailureKind::ArtifactLimit),
        FaultScript::CleanupFailure
        | FaultScript::Normal
        | FaultScript::RestartIncomplete => None,
    }
}

fn startup(
    ok: bool,
    failure_kind: Option<RuntimeFailureKind>,
) -> SupervisorObservation {
    SupervisorObservation::StartupResult { ok, failure_kind }
}

/// Deterministic fake-process driver observations at one controlled
/// clock reading (`nowMs`) for the given host requests. Mirrors the
/// oracle switch exactly, including push order.
#[must_use]
pub fn observe_fault_script(
    script: FaultScript,
    now_ms: u64,
    requested: &[&str],
) -> Vec<SupervisorObservation> {
    let mut observations = Vec::new();
    let tick = now_ms / 100;
    let wants = |name: &str| requested.contains(&name);
    match script {
        FaultScript::Normal => {
            if wants("start") {
                observations.push(startup(true, None));
            }
            if now_ms > 5_000 {
                observations.push(SupervisorObservation::ChildExit {
                    exit_code: Some(0),
                });
            }
        }
        FaultScript::SpawnFailure => {
            if wants("start") {
                observations.push(startup(
                    false,
                    Some(RuntimeFailureKind::SpawnFailed),
                ));
            }
        }
        FaultScript::SandboxDenied => {
            if wants("start") {
                observations.push(startup(
                    false,
                    Some(RuntimeFailureKind::SandboxDenied),
                ));
            }
        }
        FaultScript::StartupHang | FaultScript::CancelDuringStartup => {
            if wants("start") {
                observations.push(startup(true, None));
            }
        }
        FaultScript::IdleHang => {
            if wants("start") {
                observations.push(startup(true, None));
            }
            if wants("liveness") {
                observations.push(SupervisorObservation::Liveness {
                    kind: LivenessKind::ProcessAlive,
                });
            }
        }
        FaultScript::HardTimeout => {
            if wants("start") {
                observations.push(startup(true, None));
            }
            if tick % 2 == 0 {
                observations.push(SupervisorObservation::OutputActivity);
            }
        }
        FaultScript::CancelWhileRunning => {
            if wants("start") {
                observations.push(startup(true, None));
            }
            if now_ms > 2_000 {
                observations.push(SupervisorObservation::ChildExit {
                    exit_code: Some(0),
                });
            }
        }
        FaultScript::Crash => {
            if wants("start") {
                observations.push(startup(true, None));
            }
            if now_ms > 3_000 {
                observations.push(SupervisorObservation::ChildExit {
                    exit_code: Some(1),
                });
            }
        }
        FaultScript::ChildRefusesTermination => {
            if wants("start") {
                observations.push(startup(true, None));
            }
            if wants("terminate") {
                observations
                    .push(SupervisorObservation::ChildRefusedTermination);
            }
        }
        FaultScript::OutputFlood => {
            if wants("start") {
                observations.push(startup(true, None));
            }
            observations.push(SupervisorObservation::OutputActivity);
        }
        FaultScript::ArtifactQuota => {
            if wants("start") {
                observations.push(startup(true, None));
            }
            if now_ms > 1_000 {
                observations.push(SupervisorObservation::ChildExit {
                    exit_code: Some(0),
                });
            }
        }
        FaultScript::CleanupFailure => {
            if wants("start") {
                observations.push(startup(true, None));
            }
            if now_ms > 4_000 {
                observations.push(SupervisorObservation::ChildExit {
                    exit_code: Some(0),
                });
            }
        }
        // No startup observation: the driver represents a run record
        // left non-terminal by a restart.
        FaultScript::RestartIncomplete => {}
    }
    observations
}

#[cfg(test)]
mod tests {
    use super::{
        FAULT_SCRIPTS, FaultScript, expected_failure_kind, list_fault_scripts,
        observe_fault_script,
    };
    use crate::runtime::RuntimeFailureKind;

    #[test]
    fn scripts_replay_identically_under_the_same_clock() {
        let first =
            observe_fault_script(FaultScript::Crash, 3_100, &["start"]);
        let second =
            observe_fault_script(FaultScript::Crash, 3_100, &["start"]);
        assert_eq!(first, second);
        // Startup observation precedes the late child exit.
        assert_eq!(first.len(), 2);
        assert!(matches!(
            &first[1],
            crate::runtime::SupervisorObservation::ChildExit {
                exit_code: Some(1)
            }
        ));
        // Below the crash threshold only the startup result appears.
        let early = observe_fault_script(FaultScript::Crash, 500, &["start"]);
        assert_eq!(early.len(), 1);
    }

    #[test]
    fn tick_parity_and_thresholds_match_the_oracle() {
        // hard_timeout emits output activity on even ticks only
        // (tick = floor(nowMs / 100)).
        assert!(
            observe_fault_script(FaultScript::HardTimeout, 300, &["start"])
                .len()
                == 1,
            "odd tick carries no output activity"
        );
        assert!(
            observe_fault_script(FaultScript::HardTimeout, 400, &["start"])
                .len()
                == 2,
            "even tick emits output activity"
        );
        // normal exits cleanly after 5s; restart_incomplete never starts.
        assert_eq!(
            observe_fault_script(FaultScript::Normal, 5_001, &["start"]).len(),
            2
        );
        assert!(
            observe_fault_script(
                FaultScript::RestartIncomplete,
                9_999,
                &["start"],
            )
            .is_empty()
        );
    }

    #[test]
    fn expected_kinds_and_listing_are_deterministic() {
        assert_eq!(
            expected_failure_kind(FaultScript::SpawnFailure),
            Some(RuntimeFailureKind::SpawnFailed)
        );
        assert_eq!(
            expected_failure_kind(FaultScript::CancelWhileRunning),
            Some(RuntimeFailureKind::Cancelled)
        );
        assert_eq!(expected_failure_kind(FaultScript::CleanupFailure), None);
        let listed = list_fault_scripts();
        assert_eq!(listed.len(), FAULT_SCRIPTS.len());
        for window in listed.windows(2) {
            assert!(window[0].as_str() < window[1].as_str());
        }
    }
}
