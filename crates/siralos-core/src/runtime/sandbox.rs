//! Sandboxed command-execution port (ADR 0038).
//!
//! Core owns the capability vocabulary for the `bash` default tool:
//! the backend trait, the typed unavailability reasons, the
//! availability report, and the bounded-execution contract types
//! (command, limits, outcome). Core launches nothing — spawning a
//! child is forbidden here by the architecture check — so only an
//! adapter implements [`SandboxBackend`]. A skipped or failed live probe is
//! never availability: [`SandboxAvailability::Available`] is
//! constructed only from a passing [`probe`](SandboxBackend::probe).

use std::path::PathBuf;

use crate::provider::CancellationSignal;

/// Why a sandboxed command cannot run.
///
/// Mirrors the SECURITY.md backend states: none of them is treated
/// as secure, and none of them ever authorizes execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxUnavailableReason {
    /// One-time elevated setup never ran (never run automatically,
    /// never UAC from the harness).
    SetupRequired,
    /// A backend dependency (`bwrap`, seatbelt tooling) is absent.
    DependencyMissing,
    /// The backend cannot express the required enforcement
    /// (per-execution filesystem grants, host-read allowlist).
    EnforcementUnsupported,
    /// A live conformance probe did not pass.
    ProbeFailed,
}

impl SandboxUnavailableReason {
    /// The exact externally observable reason.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::SetupRequired => {
                "sandbox setup is required and was never performed"
            }
            Self::DependencyMissing => {
                "a sandbox backend dependency is missing"
            }
            Self::EnforcementUnsupported => {
                "the backend cannot express the required enforcement"
            }
            Self::ProbeFailed => "the live conformance probe did not pass",
        }
    }
}

impl std::fmt::Display for SandboxUnavailableReason {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for SandboxUnavailableReason {}

/// What a passing probe proves, item by item. Every field must hold
/// for [`SandboxAvailability::Available`]; the constructor below is
/// the only one and refuses a partial report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxEnforcement {
    /// The backend grants exactly this execution's filesystem roots.
    pub filesystem_grants: bool,
    /// Host reads are limited to the proven allowlist.
    pub host_read_allowlist: bool,
    /// Outbound network is denied for the child tree.
    pub network_denied: bool,
    /// Grandchildren die with the process tree.
    pub tree_confinement: bool,
}

impl SandboxEnforcement {
    /// The full report a live probe must produce. A partial report
    /// is not a weaker availability: it is unavailability.
    #[must_use]
    pub fn full() -> Self {
        Self {
            filesystem_grants: true,
            host_read_allowlist: true,
            network_denied: true,
            tree_confinement: true,
        }
    }

    /// Every enforcement item holds.
    #[must_use]
    pub fn is_complete(self) -> bool {
        self.filesystem_grants
            && self.host_read_allowlist
            && self.network_denied
            && self.tree_confinement
    }
}

/// Whether sandboxed execution may run. There is no degraded or
/// warn-but-usable state: anything short of a complete live report
/// is [`SandboxAvailability::Unavailable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxAvailability {
    /// A live probe proved every enforcement item.
    Available(SandboxEnforcement),
    /// Execution is refused without spawning anything.
    Unavailable(SandboxUnavailableReason),
}

impl SandboxAvailability {
    /// Report availability only for a complete live enforcement
    /// report; anything else is unavailability with its reason.
    #[must_use]
    pub fn report(
        enforcement: SandboxEnforcement,
        reason: SandboxUnavailableReason,
    ) -> Self {
        if enforcement.is_complete() {
            Self::Available(enforcement)
        } else {
            Self::Unavailable(reason)
        }
    }
}

/// One sandboxed command: program plus argument words (never a shell
/// string the backend must re-split), an explicit environment, and
/// the confined working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxCommand {
    /// Executable name or absolute path, resolved by the backend.
    pub program: String,
    /// Argument words, passed without shell interpretation.
    pub args: Vec<String>,
    /// Exact child environment (allowlist-built, never inherited
    /// verbatim).
    pub env: Vec<(String, String)>,
    /// Directory the child starts in (sandbox-private or
    /// read-only workspace, never an unconfined host path).
    pub workdir: PathBuf,
}

/// Execution bounds. Cancellation arrives separately through the
/// signal; the timeout kills the whole tree when it wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxLimits {
    /// Wall-clock timeout in milliseconds; expiry kills the tree.
    pub timeout_ms: u64,
    /// Retained stdout cap in bytes; excess sets truncation.
    pub stdout_max_bytes: usize,
    /// Retained stderr cap in bytes; excess sets truncation.
    pub stderr_max_bytes: usize,
}

impl SandboxLimits {
    /// The default tool bounds (ADR 0038 §5): bounded streams with
    /// explicit truncation flags, never silent prefixes.
    #[must_use]
    pub fn default_tool_limits() -> Self {
        Self {
            timeout_ms: 120_000,
            stdout_max_bytes: 1_048_576,
            stderr_max_bytes: 262_144,
        }
    }
}

/// Bounded outcome of one sandboxed execution. Truncation is always
/// flagged; a prefix is never presented as complete output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxOutput {
    /// Process exit code (`None` when killed or cancelled).
    pub status_code: Option<i32>,
    /// Retained stdout bytes (at most the stream cap).
    pub stdout: Vec<u8>,
    /// Retained stderr bytes (at most the stream cap).
    pub stderr: Vec<u8>,
    /// stdout exceeded its cap and was cut.
    pub stdout_truncated: bool,
    /// stderr exceeded its cap and was cut.
    pub stderr_truncated: bool,
    /// The timeout killed the tree.
    pub timed_out: bool,
    /// Cancellation won before completion.
    pub cancelled: bool,
}

/// Why a proven-available execution still failed. Unavailability
/// never reaches this type through the tool path: the caller maps
/// a failing probe to the typed refusal without calling `execute`.
/// [`SandboxError::BackendUnavailable`] is the defense-in-depth
/// refusal when `execute` is reached without a passing probe all
/// the same: nothing was spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxError {
    /// Spawn, kill, or wait faulted after enforcement was proven.
    ExecutionFailed,
    /// Reached without a passing probe; nothing was spawned.
    BackendUnavailable,
}

impl SandboxError {
    /// The exact externally observable message.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::ExecutionFailed => {
                "sandbox execution faulted after enforcement was proven"
            }
            Self::BackendUnavailable => {
                "sandbox execution was requested without proven enforcement"
            }
        }
    }
}

impl std::fmt::Display for SandboxError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for SandboxError {}

/// The sandbox backend port. Object-safe by construction (no
/// generics, no `Self` in return position); the adapter owns the
/// only implementation and every spawn it performs.
pub trait SandboxBackend {
    /// Run the live conformance probe and report honestly. A probe
    /// that cannot run, or that proves anything less than complete
    /// enforcement, reports [`SandboxAvailability::Unavailable`].
    fn probe(&self) -> SandboxAvailability;

    /// Execute one command under proven enforcement. The caller
    /// consults [`probe`](SandboxBackend::probe) first and maps
    /// unavailability to the typed refusal without calling this
    /// method: `execute` runs only after a passing probe, and its
    /// [`SandboxError`] covers faults after enforcement was proven,
    /// never unavailability itself.
    ///
    /// # Cancellation
    ///
    /// The signal is observed between spawn, wait, and kill; once
    /// the outcome is recorded, cancellation cannot rewrite it.
    fn execute(
        &self,
        command: &SandboxCommand,
        limits: &SandboxLimits,
        cancellation: CancellationSignal<'_>,
    ) -> Result<SandboxOutput, SandboxError>;
}

#[cfg(test)]
mod tests {
    use super::{
        SandboxAvailability, SandboxEnforcement, SandboxLimits,
        SandboxUnavailableReason,
    };

    #[test]
    fn partial_enforcement_is_unavailability_not_degraded() {
        let partial = SandboxEnforcement {
            filesystem_grants: true,
            host_read_allowlist: false,
            network_denied: true,
            tree_confinement: true,
        };
        assert!(!partial.is_complete());
        assert_eq!(
            SandboxAvailability::report(
                partial,
                SandboxUnavailableReason::EnforcementUnsupported
            ),
            SandboxAvailability::Unavailable(
                SandboxUnavailableReason::EnforcementUnsupported
            )
        );
        assert_eq!(
            SandboxAvailability::report(
                SandboxEnforcement::full(),
                SandboxUnavailableReason::ProbeFailed
            ),
            SandboxAvailability::Available(SandboxEnforcement::full())
        );
    }

    #[test]
    fn default_tool_limits_bound_both_streams() {
        let limits = SandboxLimits::default_tool_limits();
        assert!(limits.timeout_ms > 0);
        assert!(limits.stdout_max_bytes > 0);
        assert!(limits.stderr_max_bytes > 0);
    }
}
