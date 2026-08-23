//! Process supervision contract (Stage 3 — Runtime Readiness &
//! Operational Resilience, ADR 0031; R10c H3).
//!
//! Mirrors `packages/core/src/runtime/supervision.ts`. The supervisor
//! state machine is prepared → starting → running → terminating →
//! terminal; the outcome has exactly one terminal execution disposition
//! plus an independent cleanup status. Transitions are pure: state +
//! typed observation (+ controlled clock) → next state. Completion
//! ordering never decides semantics; the transition table is the single
//! source of truth.

/// Typed runtime failure taxonomy (13 kinds). Failures stay
/// distinguishable so recovery never depends on substring matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFailureKind {
    /// Readiness gate failed before start.
    ReadinessFailed,
    /// Spawn could not start the process.
    SpawnFailed,
    /// Sandbox policy denied execution.
    SandboxDenied,
    /// Startup window elapsed without completion.
    StartupTimeout,
    /// No output activity within the idle window.
    IdleTimeout,
    /// Hard lifetime exceeded.
    HardTimeout,
    /// Host-owned cancellation completed.
    Cancelled,
    /// Child process exited nonzero or abnormally.
    ProcessCrashed,
    /// Termination could not be delivered.
    KillFailed,
    /// Output capture exceeded its byte budget.
    OutputLimit,
    /// Artifact admission exceeded its budget.
    ArtifactLimit,
    /// Required environment is unavailable.
    EnvironmentUnavailable,
    /// Post-run cleanup reported failure (independent status).
    CleanupFailed,
}

impl RuntimeFailureKind {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadinessFailed => "readiness_failed",
            Self::SpawnFailed => "spawn_failed",
            Self::SandboxDenied => "sandbox_denied",
            Self::StartupTimeout => "startup_timeout",
            Self::IdleTimeout => "idle_timeout",
            Self::HardTimeout => "hard_timeout",
            Self::Cancelled => "cancelled",
            Self::ProcessCrashed => "process_crashed",
            Self::KillFailed => "kill_failed",
            Self::OutputLimit => "output_limit",
            Self::ArtifactLimit => "artifact_limit",
            Self::EnvironmentUnavailable => "environment_unavailable",
            Self::CleanupFailed => "cleanup_failed",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "readiness_failed" => Some(Self::ReadinessFailed),
            "spawn_failed" => Some(Self::SpawnFailed),
            "sandbox_denied" => Some(Self::SandboxDenied),
            "startup_timeout" => Some(Self::StartupTimeout),
            "idle_timeout" => Some(Self::IdleTimeout),
            "hard_timeout" => Some(Self::HardTimeout),
            "cancelled" => Some(Self::Cancelled),
            "process_crashed" => Some(Self::ProcessCrashed),
            "kill_failed" => Some(Self::KillFailed),
            "output_limit" => Some(Self::OutputLimit),
            "artifact_limit" => Some(Self::ArtifactLimit),
            "environment_unavailable" => Some(Self::EnvironmentUnavailable),
            "cleanup_failed" => Some(Self::CleanupFailed),
            _ => None,
        }
    }
}

/// The closed failure-kind vocabulary in oracle declaration order.
pub const RUNTIME_FAILURE_KINDS: [&str; 13] = [
    "readiness_failed",
    "spawn_failed",
    "sandbox_denied",
    "startup_timeout",
    "idle_timeout",
    "hard_timeout",
    "cancelled",
    "process_crashed",
    "kill_failed",
    "output_limit",
    "artifact_limit",
    "environment_unavailable",
    "cleanup_failed",
];

/// Terminal execution disposition (exactly one per outcome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTerminalStatus {
    /// The run succeeded.
    Success,
    /// The run failed with a typed failure kind.
    Failure,
    /// The run was cancelled by the host.
    Cancelled,
    /// A resource budget ended the run.
    ResourceLimit,
    /// The outcome cannot be determined conservatively.
    Uncertain,
}

impl RunTerminalStatus {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
            Self::ResourceLimit => "resource_limit",
            Self::Uncertain => "uncertain",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            "cancelled" => Some(Self::Cancelled),
            "resource_limit" => Some(Self::ResourceLimit),
            "uncertain" => Some(Self::Uncertain),
            _ => None,
        }
    }
}

/// Independent cleanup status for a finished run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupStatus {
    /// All run-owned paths cleaned.
    Cleaned,
    /// Some paths cleaned.
    Partial,
    /// Cleanup reported failure.
    Failed,
    /// Cleanup was skipped.
    Skipped,
    /// No run-owned paths existed.
    NotApplicable,
}

impl CleanupStatus {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cleaned => "cleaned",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::NotApplicable => "not_applicable",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "cleaned" => Some(Self::Cleaned),
            "partial" => Some(Self::Partial),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            "not_applicable" => Some(Self::NotApplicable),
            _ => None,
        }
    }
}

/// Controlled-clock timing recorded on an outcome.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunTiming {
    /// Start time when observed.
    pub started_at_ms: Option<u64>,
    /// Terminal time when reached.
    pub terminal_at_ms: Option<u64>,
    /// Total duration when both ends are known.
    pub total_ms: Option<u64>,
}

/// Observed resource summary at termination.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceSummary {
    /// Stdout bytes captured.
    pub stdout_bytes: u64,
    /// Stderr bytes captured.
    pub stderr_bytes: u64,
    /// Artifacts admitted.
    pub artifact_count: u64,
    /// Live child processes observed.
    pub child_processes: u64,
}

/// One run's terminal record: exactly one disposition plus an
/// independent cleanup status that never hides the primary result.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    /// The run this outcome belongs to.
    pub run_id: String,
    /// Terminal execution disposition.
    pub status: RunTerminalStatus,
    /// Typed failure kind for failures; `None` otherwise.
    pub failure_kind: Option<RuntimeFailureKind>,
    /// Exit code when one exists.
    pub exit_code: Option<i64>,
    /// Timing.
    pub timing: RunTiming,
    /// Resource summary.
    pub resource_summary: ResourceSummary,
    /// Artifact references (bounded).
    pub artifact_refs: Vec<String>,
    /// Evidence references (bounded).
    pub evidence_refs: Vec<String>,
    /// Independent cleanup status.
    pub cleanup_status: CleanupStatus,
}

/// Validated inputs for [`create_run_outcome`].
pub struct RunOutcomeInput {
    /// Owning run id.
    pub run_id: String,
    /// Requested disposition.
    pub status: RunTerminalStatus,
    /// Failure kind; required for and forbidden to non-failures.
    pub failure_kind: Option<RuntimeFailureKind>,
    /// Exit code when one exists.
    pub exit_code: Option<i64>,
    /// Timing.
    pub timing: RunTiming,
    /// Resource summary.
    pub resource_summary: ResourceSummary,
    /// Artifact references.
    pub artifact_refs: Vec<String>,
    /// Evidence references.
    pub evidence_refs: Vec<String>,
    /// Cleanup status; defaults to not-applicable.
    pub cleanup_status: Option<CleanupStatus>,
}

/// Create a validated run outcome. A `failure` disposition requires a
/// typed kind, and `cleanup_failed` is rejected as a terminal kind — it
/// is the independent cleanup status only.
pub fn create_run_outcome(
    input: RunOutcomeInput,
) -> Result<RunOutcome, super::RuntimeError> {
    if input.status == RunTerminalStatus::Failure
        && input.failure_kind.is_none()
    {
        return Err(super::runtime_error(
            "A failure outcome requires a typed failure kind.",
        ));
    }
    if input.status == RunTerminalStatus::Failure
        && input.failure_kind == Some(RuntimeFailureKind::CleanupFailed)
    {
        return Err(super::runtime_error(
            "cleanup_failed is an independent cleanup status, never a terminal execution disposition.",
        ));
    }
    Ok(RunOutcome {
        run_id: input.run_id,
        status: input.status,
        failure_kind: input.failure_kind,
        exit_code: input.exit_code,
        timing: input.timing,
        resource_summary: input.resource_summary,
        artifact_refs: input.artifact_refs,
        evidence_refs: input.evidence_refs,
        cleanup_status: input
            .cleanup_status
            .unwrap_or(CleanupStatus::NotApplicable),
    })
}

/// Bounded human-readable outcome projection.
#[must_use]
pub fn render_run_outcome(outcome: &RunOutcome) -> String {
    let failure = match outcome.failure_kind {
        Some(kind) => format!(" ({})", kind.as_str()),
        None => String::new(),
    };
    let exit = match outcome.exit_code {
        Some(code) => code.to_string(),
        None => "n/a".to_owned(),
    };
    format!(
        "run {} -> {}{} exit={} cleanup={} artifacts={}",
        outcome.run_id,
        outcome.status.as_str(),
        failure,
        exit,
        outcome.cleanup_status.as_str(),
        outcome.artifact_refs.len()
    )
}

// ---------------------------------------------------------------------------
// Supervisor state machine
// ---------------------------------------------------------------------------

/// Supervisor lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorState {
    /// Prepared but not started.
    Prepared,
    /// Start requested; startup pending.
    Starting,
    /// Startup completed.
    Running,
    /// Termination in progress.
    Terminating,
    /// Absorbing terminal state.
    Terminal,
}

impl SupervisorState {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Terminating => "terminating",
            Self::Terminal => "terminal",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "prepared" => Some(Self::Prepared),
            "starting" => Some(Self::Starting),
            "running" => Some(Self::Running),
            "terminating" => Some(Self::Terminating),
            "terminal" => Some(Self::Terminal),
            _ => None,
        }
    }
}

/// One typed observation delivered to the supervisor. Host timers and
/// cancellation are host-injected; process facts come from the driver.
#[derive(Debug, Clone, PartialEq)]
pub enum SupervisorObservation {
    /// Startup completed or failed.
    StartupResult {
        /// Whether startup succeeded.
        ok: bool,
        /// Typed failure when `ok` is false.
        failure_kind: Option<RuntimeFailureKind>,
    },
    /// Output activity was observed.
    OutputActivity,
    /// A liveness signal arrived.
    Liveness {
        /// Which liveness signal.
        kind: LivenessKind,
    },
    /// The idle window elapsed without activity.
    IdleTimeout,
    /// The hard lifetime elapsed.
    HardTimeout,
    /// A capture budget was exceeded.
    ResourceLimit {
        /// Which budget.
        kind: ResourceLimitKind,
    },
    /// The host requested cancellation.
    CancelRequested,
    /// The child process exited.
    ChildExit {
        /// Exit code when one exists.
        exit_code: Option<i64>,
    },
    /// A termination attempt reported its result.
    KillResult {
        /// Whether termination was delivered.
        ok: bool,
    },
    /// The child refused termination within its grace period.
    ChildRefusedTermination,
}

/// Liveness signal kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessKind {
    /// The OS reports the process alive.
    ProcessAlive,
    /// Startup completed.
    StartupCompleted,
    /// A runtime heartbeat arrived.
    RuntimeHeartbeat,
}

impl LivenessKind {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProcessAlive => "process_alive",
            Self::StartupCompleted => "startup_completed",
            Self::RuntimeHeartbeat => "runtime_heartbeat",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "process_alive" => Some(Self::ProcessAlive),
            "startup_completed" => Some(Self::StartupCompleted),
            "runtime_heartbeat" => Some(Self::RuntimeHeartbeat),
            _ => None,
        }
    }
}

/// Capture-budget limit kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceLimitKind {
    /// Output byte budget exceeded.
    OutputLimit,
    /// Artifact budget exceeded.
    ArtifactLimit,
}

impl ResourceLimitKind {
    /// Canonical protocol string (also the failure kind).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OutputLimit => "output_limit",
            Self::ArtifactLimit => "artifact_limit",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "output_limit" => Some(Self::OutputLimit),
            "artifact_limit" => Some(Self::ArtifactLimit),
            _ => None,
        }
    }
}

/// Snapshot of the supervisor state machine at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorStateView {
    /// Current state.
    pub state: SupervisorState,
    /// Start time once running.
    pub started_at_ms: Option<u64>,
    /// Terminal time once terminal.
    pub terminated_at_ms: Option<u64>,
    /// Terminal disposition once terminal.
    pub terminal_disposition: Option<RunTerminalStatus>,
    /// Failure kind attached to the disposition.
    pub failure_kind: Option<RuntimeFailureKind>,
}

/// The initial supervisor state (`prepared`, nothing observed).
pub fn initial_supervisor_state() -> SupervisorStateView {
    SupervisorStateView {
        state: SupervisorState::Prepared,
        started_at_ms: None,
        terminated_at_ms: None,
        terminal_disposition: None,
        failure_kind: None,
    }
}

/// Pure deterministic supervisor transition: state + typed observation
/// (+ controlled clock) → next state. Terminal is absorbing. The
/// transition table mirrors the oracle switch exactly.
#[must_use]
pub fn transition_supervisor(
    current: &SupervisorStateView,
    observation: &SupervisorObservation,
    now_ms: u64,
) -> SupervisorStateView {
    use SupervisorObservation as Observation;
    use SupervisorState as State;
    if current.state == State::Terminal {
        return current.clone();
    }
    match current.state {
        State::Prepared | State::Starting => match observation {
            Observation::StartupResult { ok: true, .. } => {
                SupervisorStateView {
                    started_at_ms: Some(now_ms),
                    ..current.clone()
                }
                .with_state(State::Running)
            }
            Observation::StartupResult { ok: false, failure_kind } => {
                SupervisorStateView {
                    state: State::Terminal,
                    terminated_at_ms: Some(now_ms),
                    terminal_disposition: Some(RunTerminalStatus::Failure),
                    failure_kind: Some(
                        failure_kind
                            .unwrap_or(RuntimeFailureKind::SpawnFailed),
                    ),
                    ..current.clone()
                }
            }
            Observation::CancelRequested
                if current.state == State::Prepared =>
            {
                SupervisorStateView {
                    state: State::Terminal,
                    terminated_at_ms: Some(now_ms),
                    terminal_disposition: Some(RunTerminalStatus::Cancelled),
                    failure_kind: Some(RuntimeFailureKind::Cancelled),
                    ..current.clone()
                }
            }
            _ => current.clone(),
        },
        State::Running => match observation {
            Observation::StartupResult { ok: false, failure_kind } => {
                // Startup never completed within the window: the host
                // timeout arrives as a failed startup result.
                SupervisorStateView {
                    state: State::Terminal,
                    terminated_at_ms: Some(now_ms),
                    terminal_disposition: Some(RunTerminalStatus::Failure),
                    failure_kind: Some(
                        failure_kind
                            .unwrap_or(RuntimeFailureKind::StartupTimeout),
                    ),
                    ..current.clone()
                }
            }
            Observation::ChildExit { exit_code } => {
                let success = *exit_code == Some(0);
                SupervisorStateView {
                    state: State::Terminal,
                    terminated_at_ms: Some(now_ms),
                    terminal_disposition: Some(if success {
                        RunTerminalStatus::Success
                    } else {
                        RunTerminalStatus::Failure
                    }),
                    failure_kind: if success {
                        None
                    } else {
                        Some(RuntimeFailureKind::ProcessCrashed)
                    },
                    ..current.clone()
                }
            }
            Observation::CancelRequested => {
                current.clone().with_state(State::Terminating)
            }
            Observation::IdleTimeout => SupervisorStateView {
                state: State::Terminal,
                terminated_at_ms: Some(now_ms),
                terminal_disposition: Some(RunTerminalStatus::Failure),
                failure_kind: Some(RuntimeFailureKind::IdleTimeout),
                ..current.clone()
            },
            Observation::HardTimeout => SupervisorStateView {
                state: State::Terminal,
                terminated_at_ms: Some(now_ms),
                terminal_disposition: Some(RunTerminalStatus::ResourceLimit),
                failure_kind: Some(RuntimeFailureKind::HardTimeout),
                ..current.clone()
            },
            Observation::ResourceLimit { kind } => SupervisorStateView {
                state: State::Terminal,
                terminated_at_ms: Some(now_ms),
                terminal_disposition: Some(RunTerminalStatus::ResourceLimit),
                failure_kind: Some(match kind {
                    ResourceLimitKind::OutputLimit => {
                        RuntimeFailureKind::OutputLimit
                    }
                    ResourceLimitKind::ArtifactLimit => {
                        RuntimeFailureKind::ArtifactLimit
                    }
                }),
                ..current.clone()
            },
            _ => current.clone(),
        },
        State::Terminating => match observation {
            Observation::KillResult { ok: true } => SupervisorStateView {
                state: State::Terminal,
                terminated_at_ms: Some(now_ms),
                terminal_disposition: Some(RunTerminalStatus::Cancelled),
                failure_kind: Some(RuntimeFailureKind::Cancelled),
                ..current.clone()
            },
            Observation::ChildExit { .. } => SupervisorStateView {
                state: State::Terminal,
                terminated_at_ms: Some(now_ms),
                terminal_disposition: Some(RunTerminalStatus::Cancelled),
                failure_kind: Some(RuntimeFailureKind::Cancelled),
                ..current.clone()
            },
            Observation::KillResult { ok: false } => SupervisorStateView {
                state: State::Terminal,
                terminated_at_ms: Some(now_ms),
                terminal_disposition: Some(RunTerminalStatus::Failure),
                failure_kind: Some(RuntimeFailureKind::KillFailed),
                ..current.clone()
            },
            Observation::ChildRefusedTermination => SupervisorStateView {
                state: State::Terminal,
                terminated_at_ms: Some(now_ms),
                terminal_disposition: Some(RunTerminalStatus::Failure),
                failure_kind: Some(RuntimeFailureKind::KillFailed),
                ..current.clone()
            },
            _ => current.clone(),
        },
        State::Terminal => current.clone(),
    }
}

impl SupervisorStateView {
    fn with_state(mut self, state: SupervisorState) -> Self {
        self.state = state;
        self
    }
}

/// Terminal reconciliation invariant: a terminal disposition can only
/// be produced by [`transition_supervisor`], so "process exited but the
/// supervisor remains running" is structurally impossible.
#[must_use]
pub fn is_supervisor_terminal(state: &SupervisorStateView) -> bool {
    state.state == SupervisorState::Terminal
        && state.terminal_disposition.is_some()
}

#[cfg(test)]
mod tests {
    use super::{
        CleanupStatus, ResourceLimitKind, ResourceSummary, RunOutcomeInput,
        RunTerminalStatus, RunTiming, RuntimeFailureKind,
        SupervisorObservation as Observation, SupervisorState,
        SupervisorStateView, create_run_outcome, initial_supervisor_state,
        is_supervisor_terminal, render_run_outcome, transition_supervisor,
    };
    use crate::runtime::{RuntimeError, runtime_error};

    fn running() -> SupervisorStateView {
        transition_supervisor(
            &initial_supervisor_state(),
            &Observation::StartupResult { ok: true, failure_kind: None },
            100,
        )
    }

    #[test]
    fn transitions_follow_the_oracle_table() {
        let prepared = initial_supervisor_state();
        // Cancel during preparation terminates immediately.
        let cancelled = transition_supervisor(
            &prepared,
            &Observation::CancelRequested,
            50,
        );
        assert_eq!(cancelled.state, SupervisorState::Terminal);
        assert_eq!(
            cancelled.terminal_disposition,
            Some(RunTerminalStatus::Cancelled)
        );
        assert_eq!(
            cancelled.failure_kind,
            Some(RuntimeFailureKind::Cancelled)
        );
        // Cancellation during STARTING is a no-op (oracle has no case).
        let starting = SupervisorStateView {
            state: SupervisorState::Starting,
            ..initial_supervisor_state()
        };
        let unchanged = transition_supervisor(
            &starting,
            &Observation::CancelRequested,
            150,
        );
        assert_eq!(unchanged.state, SupervisorState::Starting);
        // Startup success moves prepared/starting to running.
        let running = transition_supervisor(
            &initial_supervisor_state(),
            &Observation::StartupResult { ok: true, failure_kind: None },
            100,
        );
        assert_eq!(running.state, SupervisorState::Running);
        assert_eq!(
            running.started_at_ms,
            Some(100),
            "start time is bound at the controlled clock"
        );
        // Cancel while running enters terminating.
        let terminating = transition_supervisor(
            &running,
            &Observation::CancelRequested,
            200,
        );
        assert_eq!(terminating.state, SupervisorState::Terminating);
        // Successful kill completes cancellation.
        let done = transition_supervisor(
            &terminating,
            &Observation::KillResult { ok: true },
            250,
        );
        assert!(is_supervisor_terminal(&done));
        // Terminal is absorbing.
        let absorbed = transition_supervisor(
            &done,
            &Observation::ChildExit { exit_code: Some(0) },
            300,
        );
        assert_eq!(absorbed, done);
    }

    #[test]
    fn child_exit_and_budget_paths_classify_dispositions() {
        let success = transition_supervisor(
            &running(),
            &Observation::ChildExit { exit_code: Some(0) },
            400,
        );
        assert_eq!(
            success.terminal_disposition,
            Some(RunTerminalStatus::Success)
        );
        assert_eq!(success.failure_kind, None);
        let crashed = transition_supervisor(
            &running(),
            &Observation::ChildExit { exit_code: Some(1) },
            400,
        );
        assert_eq!(
            crashed.failure_kind,
            Some(RuntimeFailureKind::ProcessCrashed)
        );
        let hard =
            transition_supervisor(&running(), &Observation::HardTimeout, 500);
        assert_eq!(
            hard.terminal_disposition,
            Some(RunTerminalStatus::ResourceLimit)
        );
        assert_eq!(hard.failure_kind, Some(RuntimeFailureKind::HardTimeout));
        let flood = transition_supervisor(
            &running(),
            &Observation::ResourceLimit {
                kind: ResourceLimitKind::OutputLimit,
            },
            600,
        );
        assert_eq!(flood.failure_kind, Some(RuntimeFailureKind::OutputLimit));
        // A failed startup in running means the startup window elapsed.
        let late = transition_supervisor(
            &running(),
            &Observation::StartupResult { ok: false, failure_kind: None },
            700,
        );
        assert_eq!(
            late.failure_kind,
            Some(RuntimeFailureKind::StartupTimeout)
        );
    }

    #[test]
    fn outcomes_validate_failure_kinds_with_oracle_messages() {
        let base = || RunOutcomeInput {
            run_id: "r1".to_owned(),
            status: RunTerminalStatus::Success,
            failure_kind: None,
            exit_code: Some(0),
            timing: RunTiming::default(),
            resource_summary: ResourceSummary::default(),
            artifact_refs: vec!["a".to_owned(), "b".to_owned()],
            evidence_refs: Vec::new(),
            cleanup_status: Some(CleanupStatus::Partial),
        };
        let outcome = create_run_outcome(base()).expect("valid outcome");
        assert_eq!(
            render_run_outcome(&outcome),
            "run r1 -> success exit=0 cleanup=partial artifacts=2"
        );
        let mut missing_kind = base();
        missing_kind.status = RunTerminalStatus::Failure;
        assert_eq!(
            create_run_outcome(missing_kind),
            Err(runtime_error(
                "A failure outcome requires a typed failure kind."
            ))
        );
        let mut cleanup_kind = base();
        cleanup_kind.status = RunTerminalStatus::Failure;
        cleanup_kind.failure_kind = Some(RuntimeFailureKind::CleanupFailed);
        assert_eq!(
            create_run_outcome(cleanup_kind),
            Err(RuntimeError {
                message:
                    "cleanup_failed is an independent cleanup status, never a terminal execution disposition."
                        .to_owned()
            })
        );
        let mut crashed = base();
        crashed.status = RunTerminalStatus::Failure;
        crashed.failure_kind = Some(RuntimeFailureKind::ProcessCrashed);
        crashed.exit_code = Some(1);
        let rendered =
            render_run_outcome(&create_run_outcome(crashed).expect("valid"));
        assert_eq!(
            rendered,
            "run r1 -> failure (process_crashed) exit=1 cleanup=partial artifacts=2"
        );
        let mut no_exit = base();
        no_exit.exit_code = None;
        assert!(
            render_run_outcome(&create_run_outcome(no_exit).expect("valid"))
                .contains("exit=n/a")
        );
    }
}
