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
//!   below proved execution, exit passthrough, inside-grant writes,
//!   outside-grant denial, and tree confinement on this machine
//!   right now. Anything less is unavailability with its reason.
//!
//! Kill scope without `unsafe`: the workspace forbids `unsafe_code`,
//! so no `pre_exec`/`setpgid` process-group kill exists here — the
//! timeout/cancel path kills the `srt` supervisor child. Tree
//! confinement therefore rests on the runtime (bwrap/sandbox-exec),
//! and the canary re-proves it live on every probe with a
//! sleeper-marker check: if grandchildren ever outlive the tree,
//! the probe fails closed instead of reporting availability.
//! Network denial for loopback in particular is expressed by the
//! deny-all settings the canary runs under and proven by the C3
//! live row; if that row ever fails, the probe's `Available` claim
//! is falsified and must be revisited (never silently kept).

use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::time::{Duration, Instant};

use siralos_core::provider::CancellationSignal;
use siralos_core::runtime::sandbox::{
    SandboxAvailability, SandboxBackend, SandboxCommand, SandboxError,
    SandboxLimits, SandboxOutput, SandboxUnavailableReason,
};

#[cfg(unix)]
use crate::process::sandbox_backend::{build_sandbox_env, truncate_bounded};

/// Pinned distribution resolved on `PATH`.
pub const PINNED_SRT_DISTRIBUTION: &str = "@anthropic-ai/sandbox-runtime";
/// Exact pinned version; nothing else is driven.
pub const PINNED_SRT_VERSION: &str = "0.0.70";
/// Binary name resolved on `PATH` (Unix only; non-Unix targets never
/// resolve or spawn).
pub const SRT_BINARY_NAME: &str = "srt";
/// Probe evidence: the runtime is the pinned distribution.
const SRT_PACKAGE_JSON_NAME: &str = "package.json";

/// Canary timeout: the runtime must prove itself quickly or not at
/// all.
#[cfg(unix)]
const CANARY_TIMEOUT_MS: u64 = 30_000;
/// Sleeper delay inside the canary shell: the background marker must
/// fire while the probe still watches.
#[cfg(unix)]
const CANARY_SLEEP_SECS: u64 = 2;
/// Extra watch time after the canary run ends: covers a slow runtime
/// startup so a late sleeper cannot slip past the window.
#[cfg(unix)]
const CANARY_SETTLE_SECS: u64 = 3;
/// Poll cadence while waiting on the supervisor child.
#[cfg(unix)]
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(5);
/// Poll cadence while watching for the sleeper marker.
#[cfg(unix)]
const MARKER_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// `srt` stderr line prefixes that name a runtime infrastructure
/// failure rather than child output. They match the CLI's own
/// `console.error` call sites (`Error:`, `Failed to execute
/// command:`, `Process killed by signal:`), which always start the
/// line. A child that prints one of these prefixes at a line start
/// and exits nonzero is misclassified as an infrastructure fault —
/// a documented residual: both mappings refuse success, and the
/// live rows pin the real CLI's behavior, not a hostile child's.
/// Consumed only on Unix (plus the Unix marker test).
#[cfg(unix)]
const SRT_INFRA_MARKERS: &[&str] =
    &["Error:", "Failed to execute command:", "Process killed by signal:"];

/// Monotonic canary generation for unique temp paths.
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
                Err(()) => Err(SandboxError::ExecutionFailed),
            }
        }
    }
}

/// Search `PATH` for one executable file, canonicalized. Symlinks
/// resolve here so the version gate reads the real distribution.
/// Pure filesystem reads; nothing spawns.
fn resolve_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path_var) {
        let candidate = directory.join(name);
        if let Ok(metadata) = std::fs::symlink_metadata(&candidate) {
            if metadata.file_type().is_file()
                && let Ok(canonical) = std::fs::canonicalize(&candidate)
            {
                return Some(canonical);
            }
        }
    }
    None
}

/// Read the distribution version beside a resolved binary: walk up
/// from the canonical binary path to the first `package.json` and
/// require it to name the pinned distribution. Returns the version
/// string (the caller compares to the pin) or `None` when the
/// layout is unexpected. Pure filesystem reads; nothing spawns.
fn read_srt_version(binary: &Path) -> Option<String> {
    let canonical =
        std::fs::canonicalize(binary).unwrap_or_else(|_| binary.into());
    for ancestor in canonical.ancestors().skip(1) {
        let manifest = ancestor.join(SRT_PACKAGE_JSON_NAME);
        // A missing manifest is not the distribution root: keep
        // climbing. A manifest that parses but names another
        // package owns this path, so stop instead of rummaging
        // further up.
        let Ok(bytes) = std::fs::read(&manifest) else { continue };
        let Ok(parsed): Result<serde_json::Value, _> =
            serde_json::from_slice(&bytes)
        else {
            return None;
        };
        let name = parsed.get("name").and_then(|n| n.as_str())?;
        if name != PINNED_SRT_DISTRIBUTION {
            return None;
        }
        return parsed
            .get("version")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
    }
    None
}

/// Build the exact per-execution settings document: offline network
/// (`allowedDomains: []`, `deniedDomains: ["*"]` deny-all,
/// `strictAllowlist`) plus deny-root-re-allow filesystem grants
/// around the confined workdir (readable and writable; everything
/// else unreadable and unwritable). `None` when the workdir is not
/// absolute — settings paths resolve against the supervisor's cwd,
/// so a relative grant would mis-scope, and refusing is the only
/// safe mapping.
#[must_use]
pub fn build_srt_settings(workdir: &Path) -> Option<serde_json::Value> {
    if !workdir.is_absolute() {
        return None;
    }
    let root = workdir.to_string_lossy().into_owned();
    Some(serde_json::json!({
        "network": {
            "allowedDomains": [],
            "deniedDomains": ["*"],
            "strictAllowlist": true,
        },
        "filesystem": {
            "denyRead": ["/"],
            "allowRead": [root],
            "allowWrite": [root],
            "denyWrite": ["/"],
        },
    }))
}

/// Whether a finished run names a runtime infrastructure failure:
/// nonzero exit with one of the CLI's own error prefixes at a
/// stderr line start. Child output without the exit, or an exit
/// without the marker, maps to the child status (see the residual
/// note on [`SRT_INFRA_MARKERS`]). Mapped only on Unix (plus the
/// Unix marker test).
#[cfg(unix)]
fn is_srt_infra_failure(status_code: Option<i32>, stderr: &[u8]) -> bool {
    if status_code == Some(0) {
        return false;
    }
    let text = String::from_utf8_lossy(stderr);
    text.lines().any(|line| {
        SRT_INFRA_MARKERS.iter().any(|marker| line.starts_with(marker))
    })
}

/// Map a finished supervisor run to the bounded outcome: timeout and
/// cancellation win over completion, infrastructure faults refuse,
/// otherwise the child status passes through with explicit
/// truncation flags (never a silent prefix).
#[cfg(unix)]
fn classify_run(
    run: &SrtRun,
    limits: &SandboxLimits,
) -> Result<SandboxOutput, SandboxError> {
    if run.timed_out {
        return Ok(SandboxOutput {
            status_code: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            timed_out: true,
            cancelled: false,
        });
    }
    if run.cancelled {
        return Ok(SandboxOutput {
            status_code: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            timed_out: false,
            cancelled: true,
        });
    }
    if is_srt_infra_failure(run.status_code, &run.stderr) {
        return Err(SandboxError::ExecutionFailed);
    }
    let (stdout, stdout_truncated) =
        truncate_bounded(&run.stdout, limits.stdout_max_bytes);
    let (stderr, stderr_truncated) =
        truncate_bounded(&run.stderr, limits.stderr_max_bytes);
    Ok(SandboxOutput {
        status_code: run.status_code,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        timed_out: false,
        cancelled: false,
    })
}

/// One finished supervisor run: the raw material `classify_run`
/// maps. `stdout`/`stderr` already respect the stream caps plus one
/// probe byte (so truncation is observable, never silent).
#[cfg(unix)]
struct SrtRun {
    status_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
    cancelled: bool,
}

/// Run one command under the supervisor: private settings file,
/// closed stdin, allowlist environment, piped bounded streams, and
/// a wait loop that honors cancellation and the wall-clock limit.
/// `argv` is the fixed shell program plus the single command word
/// (never a shell string this side re-splits). `Err(())` names a
/// spawn fault (settings unwritable, supervisor missing at spawn,
/// piped-stream failure): nothing ran, and the caller maps it to
/// [`SandboxError::ExecutionFailed`].
#[cfg(unix)]
fn run_srt(
    binary: &Path,
    settings: &serde_json::Value,
    argv: &[String],
    workdir: &Path,
    limits: &SandboxLimits,
    cancellation: CancellationSignal<'_>,
) -> Result<SrtRun, ()> {
    use std::io::Read as _;
    use std::process::{Command, Stdio};

    let settings_path = write_settings_file(settings)?;
    let mut env =
        build_sandbox_env(&std::env::vars().collect::<Vec<_>>(), &[]);
    // The supervisor reads `SRT_DEBUG` for debug logging (which
    // would log the command string): never forward it, even though
    // the allowlist would otherwise drop it as an unknown name.
    env.retain(|(name, _)| name != "SRT_DEBUG");
    let mut child = Command::new(binary)
        .arg("--settings")
        .arg(&settings_path)
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(env)
        .current_dir(workdir)
        .spawn()
        .map_err(|_| {
            let _ = std::fs::remove_file(&settings_path);
        })?;
    let stdout_handle = std::thread::spawn({
        let mut pipe = child.stdout.take().ok_or(())?;
        let cap = limits.stdout_max_bytes.saturating_add(1);
        move || read_bounded(&mut pipe, cap)
    });
    let stderr_handle = std::thread::spawn({
        let mut pipe = child.stderr.take().ok_or(())?;
        let cap = limits.stderr_max_bytes.saturating_add(1);
        move || read_bounded(&mut pipe, cap)
    });
    let deadline = Instant::now() + Duration::from_millis(limits.timeout_ms);
    let mut timed_out = false;
    let mut cancelled = false;
    loop {
        match child.try_wait().map_err(|_| ())? {
            Some(status) => {
                let output = join_run(
                    status.code(),
                    stdout_handle,
                    stderr_handle,
                    false,
                    false,
                );
                let _ = std::fs::remove_file(&settings_path);
                return Ok(output);
            }
            None => {
                if cancellation.is_cancelled() {
                    cancelled = true;
                } else if Instant::now() >= deadline {
                    timed_out = true;
                }
                if cancelled || timed_out {
                    // No process-group kill exists here (`unsafe`
                    // is forbidden): the supervisor child dies and
                    // tree confinement rests on the runtime, which
                    // the canary re-proves on every probe.
                    let _ = child.kill();
                    let _ = child.wait();
                    let output = join_run(
                        None,
                        stdout_handle,
                        stderr_handle,
                        timed_out,
                        cancelled,
                    );
                    let _ = std::fs::remove_file(&settings_path);
                    return Ok(output);
                }
                std::thread::sleep(WAIT_POLL_INTERVAL);
            }
        }
    }
}

/// Collect one finished run from the reaped status and the reader
/// threads. A failed reader join maps to empty output (readers only
/// perform bounded IO; a panic there is a bug, never child
/// content).
#[cfg(unix)]
fn join_run(
    status_code: Option<i32>,
    stdout_handle: std::thread::JoinHandle<Vec<u8>>,
    stderr_handle: std::thread::JoinHandle<Vec<u8>>,
    timed_out: bool,
    cancelled: bool,
) -> SrtRun {
    SrtRun {
        status_code,
        stdout: stdout_handle.join().unwrap_or_default(),
        stderr: stderr_handle.join().unwrap_or_default(),
        timed_out,
        cancelled,
    }
}

/// Read at most `cap` bytes from one piped stream. Bounded so a
/// hostile child cannot grow the host buffer without bound; the
/// caller derives the truncation flag from the cap.
#[cfg(unix)]
fn read_bounded(pipe: &mut impl std::io::Read, cap: usize) -> Vec<u8> {
    use std::io::Read as _;

    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8_192];
    while buffer.len() < cap {
        match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let take = (cap - buffer.len()).min(n);
                buffer.extend_from_slice(&chunk[..take]);
                if take < n {
                    break;
                }
            }
        }
    }
    buffer
}

/// Write one settings document to a private temp file. Unix
/// permissions are `0600`; the caller removes the file after the
/// run (best effort — the document holds paths, never secrets).
#[cfg(unix)]
fn write_settings_file(settings: &serde_json::Value) -> Result<PathBuf, ()> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let bytes = serde_json::to_vec(settings).map_err(|_| ())?;
    let path = std::env::temp_dir().join(format!(
        "siralos-srt-settings-{}-{}.json",
        std::process::id(),
        CANARY_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    match options.open(&path) {
        Ok(mut file) => {
            use std::io::Write as _;
            if file.write_all(&bytes).is_err() {
                let _ = std::fs::remove_file(&path);
                return Err(());
            }
        }
        Err(_) => return Err(()),
    }
    Ok(path)
}

/// Single-quote one host path for the canary shell string. Paths
/// here are host-constructed (never model input); quoting still
/// closes the only interpolation gap.
#[cfg(unix)]
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// Run the live conformance canary: one supervisor execution under
/// the generated offline settings must (a) run and exit zero with
/// passthrough output, (b) write inside its grant, (c) fail to
/// write outside its grant on the host too, and (d) reap a
/// background grandchild (the sleeper marker must never appear).
/// `false` on any deviation, including any spawn fault.
#[cfg(unix)]
fn run_canary(binary: &Path) -> bool {
    let generation = CANARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "siralos-srt-canary-{}-{generation}",
        std::process::id(),
    ));
    if std::fs::create_dir_all(&directory).is_err() {
        return false;
    }
    let canonical =
        std::fs::canonicalize(&directory).unwrap_or(directory.clone());
    let inside = canonical.join("inside-ok");
    let late = canonical.join("late-marker");
    let escape = std::env::temp_dir().join(format!(
        "siralos-canary-escape-{}-{generation}",
        std::process::id(),
    ));
    let _ = std::fs::remove_file(&escape);
    let Some(settings) = build_srt_settings(&canonical) else {
        cleanup_canary(&directory, &escape);
        return false;
    };
    // One shell word in argv mode (the CLI quotes it into a single
    // `sh -c` layer): the exact probes are `CANARY_OK`
    // (passthrough), `INSIDE_OK` (inside-grant write), `CONTAINED`
    // (outside-grant write refused), and the background sleeper
    // (tree confinement — the marker must never land).
    let command = format!(
        "printf 'CANARY_OK\\n'; touch {} && printf 'INSIDE_OK\\n'; \
         touch {} 2>/dev/null && printf 'ESCAPED\\n' || printf 'CONTAINED\\n'; \
         (sleep {CANARY_SLEEP} && touch {} &)",
        shell_quote(&inside),
        shell_quote(&escape),
        shell_quote(&late),
        CANARY_SLEEP = CANARY_SLEEP_SECS,
    );
    let token = siralos_core::provider::CancellationToken::new();
    let limits = SandboxLimits {
        timeout_ms: CANARY_TIMEOUT_MS,
        stdout_max_bytes: 8_192,
        stderr_max_bytes: 8_192,
    };
    let start = Instant::now();
    // Argv mode: the fixed shell plus the single canary word cross
    // as words (the CLI quotes them into one `sh -c` layer).
    let argv = vec!["sh".to_owned(), "-c".to_owned(), command];
    let passed = match run_srt(
        binary,
        &settings,
        &argv,
        &canonical,
        &limits,
        token.signal(),
    ) {
        Ok(run) => {
            let text = String::from_utf8_lossy(&run.stdout);
            !run.timed_out
                && !run.cancelled
                && run.status_code == Some(0)
                && text.contains("CANARY_OK")
                && text.contains("INSIDE_OK")
                && text.contains("CONTAINED")
                && !text.contains("ESCAPED")
                && inside.exists()
                && !escape.exists()
        }
        Err(()) => false,
    };
    // The sleeper fires after the shell starts; the window covers a
    // slow supervisor startup, then cleanup runs only once no live
    // sleeper can remain (it self-terminates after its touch).
    let watch_until =
        start + Duration::from_secs(CANARY_SLEEP_SECS + CANARY_SETTLE_SECS);
    let mut reaped = passed;
    while Instant::now() < watch_until {
        if late.exists() {
            reaped = false;
            break;
        }
        std::thread::sleep(MARKER_POLL_INTERVAL);
    }
    reaped = reaped && !late.exists();
    cleanup_canary(&directory, &escape);
    reaped
}

/// Remove canary paths (best effort; holds no secrets).
#[cfg(unix)]
fn cleanup_canary(directory: &Path, escape: &Path) {
    let _ = std::fs::remove_dir_all(directory);
    let _ = std::fs::remove_file(escape);
}

#[cfg(test)]
mod tests {
    use super::{
        PINNED_SRT_DISTRIBUTION, PINNED_SRT_VERSION, build_srt_settings,
        read_srt_version,
    };
    #[cfg(unix)]
    use super::{
        SrtEnforcingBackend, SrtRun, classify_run, is_srt_infra_failure,
        run_srt,
    };
    #[cfg(unix)]
    use siralos_core::runtime::sandbox::{
        SandboxAvailability, SandboxBackend, SandboxError, SandboxLimits,
        SandboxUnavailableReason,
    };
    #[cfg(not(unix))]
    use siralos_core::runtime::sandbox::{
        SandboxAvailability, SandboxBackend, SandboxUnavailableReason,
    };

    #[cfg(unix)]
    fn limits() -> SandboxLimits {
        SandboxLimits {
            timeout_ms: 5_000,
            stdout_max_bytes: 8,
            stderr_max_bytes: 8,
        }
    }

    #[test]
    fn settings_deny_all_network_and_deny_root_re_allow() {
        // `temp_dir` is absolute on every platform (`/work/root`
        // is not absolute on Windows).
        let workdir = std::env::temp_dir().join("siralos-srt-settings-shape");
        let settings =
            build_srt_settings(&workdir).expect("absolute grants build");
        let root = workdir.to_string_lossy().into_owned();
        assert_eq!(
            settings["network"]["allowedDomains"],
            serde_json::json!([])
        );
        assert_eq!(
            settings["network"]["deniedDomains"],
            serde_json::json!(["*"])
        );
        assert_eq!(settings["network"]["strictAllowlist"], true);
        assert_eq!(
            settings["filesystem"]["denyRead"],
            serde_json::json!(["/"])
        );
        assert_eq!(
            settings["filesystem"]["allowRead"],
            serde_json::json!([root])
        );
        assert_eq!(
            settings["filesystem"]["allowWrite"],
            serde_json::json!([root])
        );
        assert_eq!(
            settings["filesystem"]["denyWrite"],
            serde_json::json!(["/"])
        );
    }

    #[test]
    fn settings_refuse_relative_workdirs() {
        assert!(
            build_srt_settings(std::path::Path::new("relative/root"))
                .is_none(),
            "a relative grant would mis-scope against the cwd"
        );
    }

    #[test]
    fn version_gate_reads_package_json_not_cli_output() {
        let base = std::env::temp_dir()
            .join(format!("siralos-srt-pin-{}-version", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let bin = base.join("pkg").join("dist").join("srt");
        std::fs::create_dir_all(bin.parent().expect("dist")).expect("fixture");
        std::fs::write(&bin, b"stub").expect("binary stand-in");
        std::fs::write(
            base.join("pkg").join("package.json"),
            format!(
                "{{\"name\":{PINNED_SRT_DISTRIBUTION:?},\
                 \"version\":{PINNED_SRT_VERSION:?}}}"
            ),
        )
        .expect("manifest");
        assert_eq!(read_srt_version(&bin), Some("0.0.70".to_owned()));
        // Wrong version is reported (the caller refuses it); wrong
        // package or missing manifest refuses outright.
        std::fs::write(
            base.join("pkg").join("package.json"),
            r#"{"name":"@anthropic-ai/sandbox-runtime","version":"9.9.9"}"#,
        )
        .expect("drifted manifest");
        assert_eq!(read_srt_version(&bin), Some("9.9.9".to_owned()));
        std::fs::write(
            base.join("pkg").join("package.json"),
            r#"{"name":"evil-fork","version":"0.0.70"}"#,
        )
        .expect("renamed manifest");
        assert_eq!(read_srt_version(&bin), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    // Unix-only mapping (plus the Unix-only classifier): the marker
    // vocabulary is asserted where it is consumed.
    #[cfg(unix)]
    #[test]
    fn infra_markers_name_only_cli_failures() {
        assert!(is_srt_infra_failure(Some(1), b"Error: boom\n"));
        assert!(is_srt_infra_failure(
            Some(1),
            b"child said hi\nFailed to execute command: gone\n"
        ));
        assert!(is_srt_infra_failure(
            Some(1),
            b"Process killed by signal: SIGKILL\n"
        ));
        // Zero never faults, and child output without the marker
        // maps to the child status — but a nonzero exit WITH the
        // marker faults even when the child printed it (documented
        // residual on `SRT_INFRA_MARKERS`: both mappings refuse
        // success, and the live rows pin the real CLI).
        assert!(!is_srt_infra_failure(Some(0), b"Error: zero\n"));
        assert!(is_srt_infra_failure(Some(3), b"Error: late\n"));
        assert!(!is_srt_infra_failure(Some(3), b"plain failure\n"));
        // No status is never success: with the marker it faults
        // (an externally killed supervisor after its own error),
        // without it the caller maps the empty status.
        assert!(is_srt_infra_failure(None, b"Error: no status\n"));
        assert!(!is_srt_infra_failure(None, b"plain output\n"));
    }

    #[cfg(unix)]
    #[test]
    fn classification_maps_timeout_cancel_fault_and_status() {
        // Timeout and cancellation win over completion.
        let timed =
            classify_run(&timed_run(), &SandboxLimits::default_tool_limits())
                .expect("timeout maps");
        assert!(timed.timed_out && timed.status_code.is_none());
        // Infrastructure faults refuse.
        assert_eq!(
            classify_run(
                &SrtRun {
                    status_code: Some(1),
                    stdout: Vec::new(),
                    stderr: b"Error: no settings\n".to_vec(),
                    timed_out: false,
                    cancelled: false,
                },
                &limits(),
            ),
            Err(SandboxError::ExecutionFailed)
        );
        // Child statuses pass through with explicit truncation.
        let mapped = classify_run(
            &SrtRun {
                status_code: Some(3),
                stdout: b"0123456789abcdef".to_vec(),
                stderr: Vec::new(),
                timed_out: false,
                cancelled: false,
            },
            &limits(),
        )
        .expect("child status maps");
        assert_eq!(mapped.status_code, Some(3));
        assert!(mapped.stdout_truncated);
        assert_eq!(mapped.stdout.len(), 8);
    }

    #[cfg(unix)]
    fn timed_run() -> SrtRun {
        SrtRun {
            status_code: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            timed_out: true,
            cancelled: false,
        }
    }

    #[cfg(unix)]
    #[test]
    fn missing_binary_is_dependency_missing() {
        let backend = SrtEnforcingBackend::with_binary(std::path::Path::new(
            "/nonexistent/srt-missing",
        ));
        assert_eq!(
            backend.probe(),
            SandboxAvailability::Unavailable(
                SandboxUnavailableReason::DependencyMissing
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn drifted_version_is_dependency_missing() {
        let base = std::env::temp_dir()
            .join(format!("siralos-srt-drift-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let bin = base.join("srt");
        std::fs::create_dir_all(&base).expect("fixture");
        std::fs::write(&bin, b"stub").expect("binary stand-in");
        std::fs::write(
            base.join("package.json"),
            r#"{"name":"@anthropic-ai/sandbox-runtime","version":"9.9.9"}"#,
        )
        .expect("drifted manifest");
        let backend = SrtEnforcingBackend::with_binary(&bin);
        // The binary resolves but the pin rejects it — before any
        // canary could run.
        assert_eq!(
            backend.probe(),
            SandboxAvailability::Unavailable(
                SandboxUnavailableReason::DependencyMissing
            )
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn stub_supervisor_shapes_args_and_passes_status_through() {
        use std::os::unix::fs::PermissionsExt as _;

        // A stub supervisor: records its argv, emits fixed streams,
        // and exits with the child's code. Driven through `run_srt`
        // directly — the probe canary needs the real runtime, and
        // the live rows (C3) prove the integrated path.
        let base = std::env::temp_dir()
            .join(format!("siralos-srt-stub-{}-shape", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("stub dir");
        let stub = base.join("srt");
        let log = base.join("argv.log");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {log}\n\
                 printf 'stub-stdout\\n'\nprintf 'stub-stderr\\n' >&2\nexit 3\n",
                log = log.to_string_lossy(),
            ),
        )
        .expect("stub");
        let mut permissions =
            std::fs::metadata(&stub).expect("meta").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&stub, permissions).expect("exec");
        let workdir = base.join("work");
        std::fs::create_dir_all(&workdir).expect("work");
        let settings =
            super::build_srt_settings(&workdir).expect("settings build");
        let token = siralos_core::provider::CancellationToken::new();
        let word = "echo 'exact bytes &;\"quoted\"'";
        let run = super::run_srt(
            &stub,
            &settings,
            &["sh".to_owned(), "-c".to_owned(), word.to_owned()],
            &workdir,
            &SandboxLimits::default_tool_limits(),
            token.signal(),
        )
        .expect("stub runs");
        assert_eq!(run.status_code, Some(3));
        assert!(!run.timed_out && !run.cancelled);
        assert_eq!(run.stdout, b"stub-stdout\n");
        assert_eq!(run.stderr, b"stub-stderr\n");
        let argv = std::fs::read_to_string(&log).expect("argv");
        let lines: Vec<&str> = argv.lines().collect();
        assert!(lines.contains(&"--settings"), "{argv:?}");
        // Argv mode: the fixed shell and the model word cross as
        // separate words — the word itself byte-identical, never
        // re-split or re-quoted on this side.
        assert!(lines.contains(&"sh"), "{argv:?}");
        assert!(lines.contains(&"-c"), "{argv:?}");
        assert!(lines.contains(&word), "{argv:?}");
        let settings_path = argv
            .lines()
            .skip_while(|line| *line != "--settings")
            .nth(1)
            .expect("settings path follows its flag")
            .to_owned();
        // The settings carried the confined workdir grant, and the
        // file was consumed and cleaned up after the run.
        let document =
            std::fs::read_to_string(&settings_path).unwrap_or_default();
        assert!(document.is_empty(), "settings file must not outlive");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn stub_supervisor_timeout_kills_and_reports() {
        use std::os::unix::fs::PermissionsExt as _;

        let base = std::env::temp_dir()
            .join(format!("siralos-srt-stub-{}-timeout", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("stub dir");
        let stub = base.join("srt");
        std::fs::write(&stub, "#!/bin/sh\nsleep 30\nexit 0\n").expect("stub");
        let mut permissions =
            std::fs::metadata(&stub).expect("meta").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&stub, permissions).expect("exec");
        let settings =
            super::build_srt_settings(&base).expect("settings build");
        let token = siralos_core::provider::CancellationToken::new();
        let run = super::run_srt(
            &stub,
            &settings,
            &["sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            &base,
            &SandboxLimits {
                timeout_ms: 500,
                stdout_max_bytes: 8_192,
                stderr_max_bytes: 8_192,
            },
            token.signal(),
        )
        .expect("timeout still returns a run");
        assert!(run.timed_out, "the wall clock must win");
        assert!(!run.cancelled);
        assert_eq!(run.status_code, None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn stub_supervisor_cancel_kills_and_reports() {
        use std::os::unix::fs::PermissionsExt as _;

        let base = std::env::temp_dir()
            .join(format!("siralos-srt-stub-{}-cancel", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("stub dir");
        let stub = base.join("srt");
        std::fs::write(&stub, "#!/bin/sh\nsleep 30\nexit 0\n").expect("stub");
        let mut permissions =
            std::fs::metadata(&stub).expect("meta").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&stub, permissions).expect("exec");
        let settings =
            super::build_srt_settings(&base).expect("settings build");
        let token = siralos_core::provider::CancellationToken::new();
        token.cancel();
        let run = super::run_srt(
            &stub,
            &settings,
            &["sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            &base,
            &SandboxLimits::default_tool_limits(),
            token.signal(),
        )
        .expect("cancel still returns a run");
        assert!(run.cancelled, "cancellation must win");
        assert!(!run.timed_out);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(not(unix))]
    #[test]
    fn non_unix_probe_never_drives_the_backend() {
        // Missing or drifted runtime: dependency-missing. A pinned
        // binary is still undrivable here: unsupported, never
        // available, and nothing spawns either way.
        let missing = super::SrtEnforcingBackend::with_binary(
            std::path::Path::new("/nonexistent/srt-missing"),
        );
        assert_eq!(
            missing.probe(),
            SandboxAvailability::Unavailable(
                SandboxUnavailableReason::DependencyMissing
            )
        );
        let base = std::env::temp_dir()
            .join(format!("siralos-srt-nonunix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let bin = base.join("srt.exe");
        std::fs::create_dir_all(&base).expect("fixture");
        std::fs::write(&bin, b"stub").expect("binary stand-in");
        std::fs::write(
            base.join("package.json"),
            format!(
                "{{\"name\":{:?},\"version\":{:?}}}",
                super::PINNED_SRT_DISTRIBUTION,
                super::PINNED_SRT_VERSION,
            ),
        )
        .expect("manifest");
        let pinned = super::SrtEnforcingBackend::with_binary(&bin);
        assert_eq!(
            pinned.probe(),
            SandboxAvailability::Unavailable(
                SandboxUnavailableReason::EnforcementUnsupported
            )
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(not(unix))]
    #[test]
    fn non_unix_execute_spawns_nothing() {
        use siralos_core::runtime::sandbox::SandboxError;

        // The command would create this file if it ever ran: its
        // absence after the refused call proves no spawn happened.
        let marker = std::env::temp_dir().join(format!(
            "siralos-srt-never-{}-spawned.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&marker);
        let backend = super::SrtEnforcingBackend::new();
        let token = siralos_core::provider::CancellationToken::new();
        let error = backend
            .execute(
                &siralos_core::runtime::sandbox::SandboxCommand {
                    program: "sh".to_owned(),
                    args: vec![
                        "-c".to_owned(),
                        format!("touch {}", marker.display()),
                    ],
                    env: Vec::new(),
                    workdir: std::env::temp_dir(),
                },
                &siralos_core::runtime::sandbox::SandboxLimits::default_tool_limits(
                ),
                token.signal(),
            )
            .expect_err("unproven enforcement must refuse");
        assert_eq!(error, SandboxError::BackendUnavailable);
        assert!(
            !marker.exists(),
            "a refused execution must leave no child effects"
        );
    }

    // Live enforcement rows (C3). Unix-only and opt-in: they run
    // only with `SIRALOS_PROVE_SRT=1` and the pinned `srt` on PATH,
    // and they FAIL (never skip) when proof is requested but the
    // probe refuses — a skipped probe is never a pass. Without the
    // variable they pass vacuously and the ADR evidence table
    // records them `unknown`; the CI evidence is the test log
    // showing each row PROVEN.
    #[cfg(unix)]
    fn require_live_backend() -> Option<SrtEnforcingBackend> {
        if std::env::var("SIRALOS_PROVE_SRT").is_err() {
            eprintln!(
                "SKIP live row: set SIRALOS_PROVE_SRT=1 with srt 0.0.70 on \
                 PATH to prove (recorded unknown until then)"
            );
            return None;
        }
        let backend = SrtEnforcingBackend::new();
        match backend.probe() {
            SandboxAvailability::Available(_) => Some(backend),
            SandboxAvailability::Unavailable(reason) => {
                panic!(
                    "SIRALOS_PROVE_SRT=1 but the live probe refuses: {reason}"
                )
            }
        }
    }

    #[cfg(unix)]
    fn live_workdir(label: &str) -> std::path::PathBuf {
        let generation = super::CANARY_SEQUENCE
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "siralos-srt-row-{}-{label}-{generation}",
            std::process::id(),
        ));
        std::fs::create_dir_all(&directory).expect("row workdir");
        std::fs::canonicalize(&directory).unwrap_or(directory)
    }

    #[cfg(unix)]
    fn row_limits(timeout_ms: u64) -> SandboxLimits {
        SandboxLimits {
            timeout_ms,
            stdout_max_bytes: 65_536,
            stderr_max_bytes: 16_384,
        }
    }

    #[cfg(unix)]
    fn row_command(
        workdir: &std::path::Path,
        program: &str,
        word: &str,
    ) -> siralos_core::runtime::sandbox::SandboxCommand {
        siralos_core::runtime::sandbox::SandboxCommand {
            program: program.to_owned(),
            args: vec!["-c".to_owned(), word.to_owned()],
            env: Vec::new(),
            workdir: workdir.to_path_buf(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn live_row_grandchild_reaping_kills_the_tree() {
        let Some(backend) = require_live_backend() else { return };
        let workdir = live_workdir("reap");
        let marker = workdir.join("late-marker");
        let token = siralos_core::provider::CancellationToken::new();
        let word =
            format!("(sleep 2 && touch {} &)", super::shell_quote(&marker));
        let outcome = backend
            .execute(
                &row_command(&workdir, "sh", &word),
                &row_limits(15_000),
                token.signal(),
            )
            .expect("reaping row runs");
        assert_eq!(outcome.status_code, Some(0), "{outcome:?}");
        // The sleeper fires 2s after the shell starts; the shell
        // already exited, so any marker means the grandchild
        // outlived the tree.
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if marker.exists() {
                panic!("grandchild outlived the tree: enforcement broken");
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!marker.exists(), "tree confinement proved: no marker");
        eprintln!("PROVEN live_row_grandchild_reaping_kills_the_tree");
        let _ = std::fs::remove_dir_all(&workdir);
    }

    #[cfg(unix)]
    #[test]
    fn live_row_timeout_kill_terminates() {
        let Some(backend) = require_live_backend() else { return };
        let workdir = live_workdir("timeout");
        let token = siralos_core::provider::CancellationToken::new();
        let outcome = backend
            .execute(
                &row_command(&workdir, "sh", "sleep 30"),
                &row_limits(2_000),
                token.signal(),
            )
            .expect("timeout row returns a run");
        assert!(outcome.timed_out, "{outcome:?}");
        assert_eq!(outcome.status_code, None);
        eprintln!("PROVEN live_row_timeout_kill_terminates");
        let _ = std::fs::remove_dir_all(&workdir);
    }

    #[cfg(unix)]
    #[test]
    fn live_row_cancel_kills() {
        let Some(backend) = require_live_backend() else { return };
        let workdir = live_workdir("cancel");
        // Single-threaded cooperative cancel: the token is already
        // cancelled, so the wait loop kills the supervisor right
        // after spawn — the kill itself is what this row proves.
        let token = siralos_core::provider::CancellationToken::new();
        token.cancel();
        let outcome = backend
            .execute(
                &row_command(&workdir, "sh", "sleep 30"),
                &row_limits(15_000),
                token.signal(),
            )
            .expect("cancel row returns a run");
        assert!(outcome.cancelled, "{outcome:?}");
        assert!(!outcome.timed_out);
        eprintln!("PROVEN live_row_cancel_kills");
        let _ = std::fs::remove_dir_all(&workdir);
    }

    #[cfg(unix)]
    #[test]
    fn live_row_loopback_connect_is_denied() {
        let Some(backend) = require_live_backend() else { return };
        let workdir = live_workdir("network");
        let token = siralos_core::provider::CancellationToken::new();
        // Tooling gate: the row needs a connect primitive inside the
        // sandbox. Absent tooling is UNKNOWN (recorded), never a
        // pass claimed on settings alone.
        let probe = backend
            .execute(
                &row_command(&workdir, "sh", "command -v node"),
                &row_limits(15_000),
                token.signal(),
            )
            .expect("tooling probe runs");
        let tooling = String::from_utf8_lossy(&probe.stdout);
        if !tooling.contains("node") {
            eprintln!("UNKNOWN live_row_loopback: no node in sandbox");
            let _ = std::fs::remove_dir_all(&workdir);
            return;
        }
        // The listener proves the port is reachable: a failure to
        // connect is then denial, not refusal.
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().expect("port").port();
        let script = workdir.join("connect.js");
        std::fs::write(
            &script,
            format!(
                "fetch('http://127.0.0.1:{port}/',\
                 {{signal:AbortSignal.timeout(5000)}})\
                 .then(r=>console.log('CONNECTED:'+r.status))\
                 .catch(()=>console.log('DENIED'))"
            ),
        )
        .expect("script");
        let quoted = super::shell_quote(&script);
        let word = format!("node {quoted}");
        let outcome = backend
            .execute(
                &row_command(&workdir, "sh", &word),
                &row_limits(15_000),
                token.signal(),
            )
            .expect("network row runs");
        drop(listener);
        let text = String::from_utf8_lossy(&outcome.stdout);
        assert!(
            !text.contains("CONNECTED"),
            "loopback connected: network denial broken: {text:?}"
        );
        if text.contains("DENIED") {
            eprintln!("PROVEN live_row_loopback_connect_is_denied");
        } else {
            eprintln!("UNKNOWN live_row_loopback: {text:?}");
        }
        let _ = std::fs::remove_dir_all(&workdir);
    }

    #[cfg(unix)]
    #[test]
    fn live_row_environment_is_allowlisted() {
        let Some(backend) = require_live_backend() else { return };
        let workdir = live_workdir("env");
        let token = siralos_core::provider::CancellationToken::new();
        // The fixed layer always wins over the parent: TERM arrives
        // as `dumb` no matter what the harness process carries.
        let outcome = backend
            .execute(
                &row_command(&workdir, "sh", "echo TERM=$TERM"),
                &row_limits(15_000),
                token.signal(),
            )
            .expect("env row runs");
        let text = String::from_utf8_lossy(&outcome.stdout);
        assert_eq!(outcome.status_code, Some(0), "{outcome:?}");
        assert!(
            text.contains("TERM=dumb"),
            "fixed env must win, got: {text:?}"
        );
        eprintln!("PROVEN live_row_environment_is_allowlisted");
        let _ = std::fs::remove_dir_all(&workdir);
    }

    #[cfg(unix)]
    #[test]
    fn live_row_stdin_is_closed() {
        let Some(backend) = require_live_backend() else { return };
        let workdir = live_workdir("stdin");
        let token = siralos_core::provider::CancellationToken::new();
        let outcome = backend
            .execute(
                &row_command(&workdir, "sh", "wc -c <&0"),
                &row_limits(15_000),
                token.signal(),
            )
            .expect("stdin row runs");
        let text = String::from_utf8_lossy(&outcome.stdout);
        assert_eq!(outcome.status_code, Some(0), "{outcome:?}");
        assert_eq!(text.trim(), "0", "stdin must be EOF, got: {text:?}");
        eprintln!("PROVEN live_row_stdin_is_closed");
        let _ = std::fs::remove_dir_all(&workdir);
    }

    #[cfg(unix)]
    #[test]
    fn live_row_workdir_confinement_holds() {
        let Some(backend) = require_live_backend() else { return };
        let workdir = live_workdir("confine");
        let generation = super::CANARY_SEQUENCE
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let inside = workdir.join("inside-ok");
        let escape = std::env::temp_dir().join(format!(
            "siralos-row-escape-{}-{generation}",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&escape);
        let token = siralos_core::provider::CancellationToken::new();
        let word = format!(
            "touch {} && printf 'INSIDE_OK\\n'; \
             touch {} 2>/dev/null && printf 'ESCAPED\\n' || printf 'CONTAINED\\n'",
            super::shell_quote(&inside),
            super::shell_quote(&escape),
        );
        let outcome = backend
            .execute(
                &row_command(&workdir, "sh", &word),
                &row_limits(15_000),
                token.signal(),
            )
            .expect("confinement row runs");
        let text = String::from_utf8_lossy(&outcome.stdout);
        assert_eq!(outcome.status_code, Some(0), "{outcome:?}");
        assert!(text.contains("INSIDE_OK"), "{text:?}");
        assert!(text.contains("CONTAINED"), "{text:?}");
        assert!(!text.contains("ESCAPED"), "{text:?}");
        assert!(inside.exists(), "inside-grant write must land");
        assert!(
            !escape.exists(),
            "outside-grant write must fail without host effects"
        );
        eprintln!("PROVEN live_row_workdir_confinement_holds");
        let _ = std::fs::remove_dir_all(&workdir);
        let _ = std::fs::remove_file(&escape);
    }
}
