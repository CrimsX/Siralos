//! The live conformance canary (`canary`): one supervisor
//! execution under the generated offline settings must prove
//! execution, exit passthrough, inside-grant writes,
//! outside-grant denial on the host too, and tree confinement —
//! before any probe may report availability.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use siralos_core::provider::CancellationToken;
use siralos_core::runtime::sandbox::SandboxLimits;

use super::CANARY_SEQUENCE;
use super::build_srt_settings;
use super::run::run_srt;

/// Canary timeout: the runtime must prove itself quickly or not at
/// all.
const CANARY_TIMEOUT_MS: u64 = 30_000;
/// Sleeper delay inside the canary shell: the background marker must
/// fire while the probe still watches.
const CANARY_SLEEP_SECS: u64 = 2;
/// Extra watch time after the canary run ends: covers a slow runtime
/// startup so a late sleeper cannot slip past the window.
const CANARY_SETTLE_SECS: u64 = 3;
/// Poll cadence while watching for the sleeper marker.
const MARKER_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Single-quote one host path for the canary shell string. Paths
/// here are host-constructed (never model input); quoting still
/// closes the only interpolation gap.
pub(super) fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// Run the live conformance canary: one supervisor execution under
/// the generated offline settings must (a) run and exit zero with
/// passthrough output, (b) write inside its grant, (c) fail to
/// write outside its grant on the host too, and (d) reap a
/// background grandchild (the sleeper marker must never appear).
/// `false` on any deviation, including any spawn fault.
pub(super) fn run_canary(binary: &Path) -> bool {
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
    let token = CancellationToken::new();
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
        Err(_) => false,
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
fn cleanup_canary(directory: &Path, escape: &Path) {
    let _ = std::fs::remove_dir_all(directory);
    let _ = std::fs::remove_file(escape);
}
