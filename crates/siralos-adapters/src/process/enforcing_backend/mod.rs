//! Enforcing sandbox backend over the pinned Anthropic Sandbox
//! Runtime CLI (ADR 0038, Phase C).
//!
//! This backend drives the `srt` binary pinned at
//! [`PINNED_SRT_VERSION`] on `PATH`: every execution runs under a
//! generated per-execution settings file (`--settings`) expressing
//! the offline network posture and the deny-root-re-allow
//! filesystem grants, with the fixed shell plus the single
//! model-supplied word passed as argv words (the CLI shell-quotes
//! them into exactly one `sh -c` layer, so no host-side re-parse
//! can smuggle arguments). Standard input is closed, the child
//! environment is the explicit allowlist (never the parent
//! verbatim), output is bounded with explicit truncation flags,
//! and timeout/cancel kill the supervisor child.
//!
//! What this backend does NOT do, by construction:
//!
//! - No automatic setup, ever: the Windows one-shot install needs a
//!   UAC prompt and is never performed here, so non-Unix targets
//!   always report [`SandboxUnavailableReason::EnforcementUnsupported`]
//!   without spawning anything (the Windows milestone stays a named
//!   follow-up; see ADR 0038 §2).
//! - No version drift: the distribution's `package.json` must name
//!   the pinned distribution at exactly the pinned version,
//!   otherwise the probe reports
//!   [`SandboxUnavailableReason::DependencyMissing`]. `--version`
//!   is deliberately NOT the pin: the CLI prints
//!   `npm_package_version` or falls back to `1.0.0`, so a direct
//!   invocation reports `1.0.0` on every install.
//! - No degraded availability: a passing probe means the live canary
//!   (`canary`) proved execution, exit passthrough, inside-grant
//!   writes, outside-grant denial, and tree confinement on this
//!   machine right now. Anything less is unavailability with its
//!   reason.
//!
//! Kill scope without `unsafe`: the workspace forbids `unsafe_code`,
//! so no `pre_exec`/`setpgid` process-group kill exists here — the
//! timeout/cancel path kills the `srt` supervisor child (`run`). Tree
//! confinement therefore rests on the runtime (bwrap/sandbox-exec),
//! and the canary re-proves it live on every probe with a
//! sleeper-marker check: if grandchildren ever outlive the tree,
//! the probe fails closed instead of reporting availability.
//! Network denial for loopback in particular is expressed by the
//! deny-all settings the canary runs under and proven by the C3
//! live row; if that row ever fails, the probe's `Available` claim
//! is falsified and must be revisited (never silently kept).
//!
//! The surface is split at cohesive boundaries: `resolution` owns
//! PATH lookup and the version gate, `settings` owns the per-run
//! document, `run` owns the supervisor loop and outcome mapping,
//! `canary` owns the live probe, and `tests` owns the behavioral
//! inventory.

use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::sync::atomic::AtomicU64;

use siralos_core::provider::CancellationSignal;
use siralos_core::runtime::sandbox::{
    SandboxAvailability, SandboxBackend, SandboxCommand, SandboxError,
    SandboxLimits, SandboxOutput, SandboxUnavailableReason,
};

#[cfg(unix)]
use canary::run_canary;
use resolution::{read_srt_version, resolve_on_path};
#[cfg(unix)]
use run::{SrtSpawnFault, classify_run, run_srt};
#[cfg(unix)]
mod canary;
mod resolution;
#[cfg(unix)]
mod run;
mod settings;
#[cfg(test)]
mod tests;

pub use settings::build_srt_settings;

/// Pinned distribution resolved on `PATH`.
pub const PINNED_SRT_DISTRIBUTION: &str = "@anthropic-ai/sandbox-runtime";
/// Exact pinned version; nothing else is driven.
pub const PINNED_SRT_VERSION: &str = "0.0.70";
/// Binary name resolved on `PATH` (Unix only; non-Unix targets never
/// resolve or spawn).
pub const SRT_BINARY_NAME: &str = "srt";

/// Monotonic generation for unique temp paths (settings files,
/// canary dirs, live-row workdirs).
#[cfg(unix)]
static CANARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The enforcing backend. Resolution is per call (no cached
/// availability): an install or removal between calls changes the
/// next probe honestly. Tests pin a binary with
/// [`SrtEnforcingBackend::with_binary`]; production resolves on
/// `PATH`.
#[derive(Debug, Clone, Default)]
pub struct SrtEnforcingBackend {
    /// Explicit binary override (tests). `None` resolves on `PATH`.
    binary_override: Option<PathBuf>,
}

impl SrtEnforcingBackend {
    /// Resolve on `PATH` at every probe/execution.
    #[must_use]
    pub fn new() -> Self {
        Self { binary_override: None }
    }

    /// Pin one binary path (tests drive a stub `srt`).
    #[must_use]
    pub fn with_binary(path: &Path) -> Self {
        Self { binary_override: Some(path.to_path_buf()) }
    }

    /// The binary this instance drives: the override, or the first
    /// `srt` on `PATH` (canonicalized, so a swapped `PATH` entry
    /// cannot redirect between resolution and spawn — resolution and
    /// spawn still race in principle, and the version gate below is
    /// the backstop, not an atomic binding). Resolution is a pure
    /// filesystem read on every platform; only Unix ever spawns what
    /// it finds.
    fn binary(&self) -> Option<PathBuf> {
        if let Some(path) = &self.binary_override {
            return Some(path.clone());
        }
        resolve_on_path(SRT_BINARY_NAME)
    }

    /// Non-Unix probe: distinguish a missing or drifted runtime
    /// (`DependencyMissing`) from a pinned one this adapter still
    /// cannot drive (`EnforcementUnsupported` — its spawn path is
    /// Unix-only). Neither spawns; neither is availability.
    #[cfg(not(unix))]
    fn probe_non_unix(&self) -> SandboxAvailability {
        let pinned = self
            .binary()
            .and_then(|binary| read_srt_version(&binary))
            .is_some_and(|version| version == PINNED_SRT_VERSION);
        SandboxAvailability::Unavailable(if pinned {
            SandboxUnavailableReason::EnforcementUnsupported
        } else {
            SandboxUnavailableReason::DependencyMissing
        })
    }

    #[cfg(unix)]
    fn probe_unix(&self) -> SandboxAvailability {
        let Some(binary) = self.binary() else {
            return SandboxAvailability::Unavailable(
                SandboxUnavailableReason::DependencyMissing,
            );
        };
        match read_srt_version(&binary) {
            Some(version) if version == PINNED_SRT_VERSION => {}
            _ => {
                return SandboxAvailability::Unavailable(
                    SandboxUnavailableReason::DependencyMissing,
                );
            }
        }
        if run_canary(&binary) {
            SandboxAvailability::Available(
                siralos_core::runtime::sandbox::SandboxEnforcement::full(),
            )
        } else {
            SandboxAvailability::Unavailable(
                SandboxUnavailableReason::ProbeFailed,
            )
        }
    }
}

impl SandboxBackend for SrtEnforcingBackend {
    fn probe(&self) -> SandboxAvailability {
        // Non-Unix targets resolve and version-gate (pure filesystem
        // reads) but never spawn: the one-shot setup needs elevation
        // and is never performed here, so enforcement is unproven by
        // construction even for a pinned binary. This keeps the
        // Windows milestone a named follow-up (ADR 0038 §2), not a
        // silent partial availability.
        #[cfg(not(unix))]
        {
            self.probe_non_unix()
        }
        #[cfg(unix)]
        {
            self.probe_unix()
        }
    }

    fn execute(
        &self,
        command: &SandboxCommand,
        limits: &SandboxLimits,
        cancellation: CancellationSignal<'_>,
    ) -> Result<SandboxOutput, SandboxError> {
        // Defense in depth, mirroring the host backend: a direct
        // caller without a passing probe is refused without
        // spawning anything, even though the tool path maps
        // unavailability first.
        #[cfg(not(unix))]
        {
            let _ = (command, limits, cancellation);
            let _ = self;
            Err(SandboxError::BackendUnavailable)
        }
        #[cfg(unix)]
        {
            match self.probe() {
                SandboxAvailability::Available(_) => {}
                SandboxAvailability::Unavailable(_) => {
                    return Err(SandboxError::BackendUnavailable);
                }
            }
            let Some(binary) = self.binary() else {
                return Err(SandboxError::BackendUnavailable);
            };
            let Some(settings) = build_srt_settings(&command.workdir) else {
                return Err(SandboxError::ExecutionFailed);
            };
            // Argv words cross exactly: the fixed shell program plus
            // the single model word, never a re-split shell string.
            let mut argv =
                Vec::with_capacity(command.args.len().saturating_add(1));
            argv.push(command.program.clone());
            argv.extend(command.args.iter().cloned());
            match run_srt(
                &binary,
                &settings,
                &argv,
                &command.workdir,
                limits,
                cancellation,
            ) {
                Ok(run) => classify_run(&run, limits),
                // A spawn fault names its stage locally
                // (`SrtSpawnFault`); every fault maps to the same
                // refusal here: nothing ran.
                Err(
                    SrtSpawnFault::Settings
                    | SrtSpawnFault::Spawn
                    | SrtSpawnFault::Pipes
                    | SrtSpawnFault::Wait,
                ) => Err(SandboxError::ExecutionFailed),
            }
        }
    }
}
