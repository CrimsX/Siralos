//! The supervisor loop (`run`): private settings file, closed
//! stdin, allowlist environment, piped bounded streams, and a wait
//! loop honoring cancellation and the wall clock, then the mapping
//! of finished runs to bounded outcomes.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use siralos_core::provider::CancellationSignal;
use siralos_core::runtime::sandbox::{
    SandboxError, SandboxLimits, SandboxOutput,
};

use super::CANARY_SEQUENCE;
use crate::process::sandbox_backend::{build_sandbox_env, truncate_bounded};

/// Poll cadence while waiting on the supervisor child.
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// `srt` stderr line prefixes that name a runtime infrastructure
/// failure rather than child output. They match the CLI's own
/// `console.error` call sites (`Error:`, `Failed to execute
/// command:`, `Process killed by signal:`), which always start the
/// line. A child that prints one of these prefixes at a line start
/// and exits nonzero is misclassified as an infrastructure fault —
/// a documented residual: both mappings refuse success, and the
/// live rows pin the real CLI's behavior, not a hostile child's.
const SRT_INFRA_MARKERS: &[&str] =
    &["Error:", "Failed to execute command:", "Process killed by signal:"];

/// Where the supervisor run stopped before it ever produced child
/// output: a settings file that would not write, a binary missing
/// at spawn, a piped stream unavailable, or a wait failure. Nothing
/// ran; the caller maps every variant to
/// [`SandboxError::ExecutionFailed`].
#[derive(Debug)]
pub(super) enum SrtSpawnFault {
    /// The settings document could not be written.
    Settings,
    /// The supervisor binary could not spawn.
    Spawn,
    /// A piped stream was unavailable at spawn.
    Pipes,
    /// Waiting on the child failed.
    Wait,
}

/// One finished supervisor run: the raw material `classify_run`
/// maps. `stdout`/`stderr` already respect the stream caps plus one
/// probe byte (so truncation is observable, never silent).
pub(super) struct SrtRun {
    pub(super) status_code: Option<i32>,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) timed_out: bool,
    pub(super) cancelled: bool,
}

/// Whether a finished run names a runtime infrastructure failure:
/// nonzero exit with one of the CLI's own error prefixes at a
/// stderr line start. Child output without the exit, or an exit
/// without the marker, maps to the child status (see the residual
/// note on `SRT_INFRA_MARKERS`).
pub(super) fn is_srt_infra_failure(
    status_code: Option<i32>,
    stderr: &[u8],
) -> bool {
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
pub(super) fn classify_run(
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

/// Run one command under the supervisor: private settings file,
/// closed stdin, allowlist environment, piped bounded streams, and
/// a wait loop that honors cancellation and the wall-clock limit.
/// `argv` is the fixed shell program plus the single command word
/// (never a shell string this side re-splits). `Err` names a spawn
/// fault ([`SrtSpawnFault`]): nothing ran, and the caller maps
/// every variant to [`SandboxError::ExecutionFailed`].
pub(super) fn run_srt(
    binary: &Path,
    settings: &serde_json::Value,
    argv: &[String],
    workdir: &Path,
    limits: &SandboxLimits,
    cancellation: CancellationSignal<'_>,
) -> Result<SrtRun, SrtSpawnFault> {
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
            SrtSpawnFault::Spawn
        })?;
    let stdout_handle = std::thread::spawn({
        let mut pipe = child.stdout.take().ok_or(SrtSpawnFault::Pipes)?;
        let cap = limits.stdout_max_bytes.saturating_add(1);
        move || read_bounded(&mut pipe, cap)
    });
    let stderr_handle = std::thread::spawn({
        let mut pipe = child.stderr.take().ok_or(SrtSpawnFault::Pipes)?;
        let cap = limits.stderr_max_bytes.saturating_add(1);
        move || read_bounded(&mut pipe, cap)
    });
    let deadline = Instant::now() + Duration::from_millis(limits.timeout_ms);
    let mut timed_out = false;
    let mut cancelled = false;
    loop {
        match child.try_wait().map_err(|_| SrtSpawnFault::Wait)? {
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
/// `Err` names [`SrtSpawnFault::Settings`].
fn write_settings_file(
    settings: &serde_json::Value,
) -> Result<PathBuf, SrtSpawnFault> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let bytes =
        serde_json::to_vec(settings).map_err(|_| SrtSpawnFault::Settings)?;
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
                return Err(SrtSpawnFault::Settings);
            }
        }
        Err(_) => return Err(SrtSpawnFault::Settings),
    }
    Ok(path)
}
