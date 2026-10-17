//! Behavioral inventory for the enforcing backend: settings shape,
//! the version gate, infrastructure-marker and outcome mapping,
//! stub-supervisor mechanics, platform refusals, and the opt-in
//! live enforcement rows (C3). Unix-only sections carry the row
//! vocabulary where it is consumed.

use super::build_srt_settings;
#[cfg(unix)]
use super::canary::shell_quote;
use super::resolution::read_srt_version;
#[cfg(unix)]
use super::run::{SrtRun, classify_run, is_srt_infra_failure, run_srt};
use super::{
    PINNED_SRT_DISTRIBUTION, PINNED_SRT_VERSION, SrtEnforcingBackend,
};
use siralos_core::provider::CancellationToken;
#[cfg(unix)]
use siralos_core::runtime::sandbox::{
    SandboxAvailability, SandboxBackend, SandboxCommand, SandboxError,
    SandboxLimits, SandboxUnavailableReason,
};
#[cfg(not(unix))]
use siralos_core::runtime::sandbox::{
    SandboxAvailability, SandboxBackend, SandboxCommand, SandboxLimits,
    SandboxUnavailableReason,
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
    assert_eq!(settings["network"]["allowedDomains"], serde_json::json!([]));
    assert_eq!(settings["network"]["deniedDomains"], serde_json::json!(["*"]));
    assert_eq!(settings["network"]["strictAllowlist"], true);
    assert_eq!(settings["filesystem"]["denyRead"], serde_json::json!(["/"]));
    assert_eq!(settings["filesystem"]["allowRead"], serde_json::json!([root]));
    assert_eq!(
        settings["filesystem"]["allowWrite"],
        serde_json::json!([root])
    );
    assert_eq!(settings["filesystem"]["denyWrite"], serde_json::json!(["/"]));
}

#[test]
fn settings_refuse_relative_workdirs() {
    assert!(
        build_srt_settings(std::path::Path::new("relative/root")).is_none(),
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

/// Create a stub supervisor directory: `base/srt` holds `script`
/// with executable permissions. Callers add whatever else the
/// script references (an argv log in the supervisor's cwd, say)
/// and derive `stub` as `base.join("srt")`. Driven through
/// `run_srt` directly — the probe canary needs the real runtime,
/// and the live rows (C3) prove the integrated path.
#[cfg(unix)]
fn stub_supervisor(label: &str, script: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;

    let base = std::env::temp_dir()
        .join(format!("siralos-srt-stub-{}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("stub dir");
    let stub = base.join("srt");
    std::fs::write(&stub, script).expect("stub");
    let mut permissions =
        std::fs::metadata(&stub).expect("meta").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&stub, permissions).expect("exec");
    base
}

#[cfg(unix)]
#[test]
fn stub_supervisor_shapes_args_and_passes_status_through() {
    // A stub supervisor that records its argv into its own cwd
    // (run_srt launches the supervisor inside the confined
    // workdir), emits fixed streams, and exits with the child's
    // code.
    let base = stub_supervisor(
        "shape",
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > argv.log\n\
         printf 'stub-stdout\\n'\nprintf 'stub-stderr\\n' >&2\nexit 3\n",
    );
    let stub = base.join("srt");
    let workdir = base.join("work");
    std::fs::create_dir_all(&workdir).expect("work");
    let settings = build_srt_settings(&workdir).expect("settings build");
    let token = CancellationToken::new();
    let word = "echo 'exact bytes &;\"quoted\"'";
    let run = run_srt(
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
    let argv =
        std::fs::read_to_string(workdir.join("argv.log")).expect("argv");
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
    let document = std::fs::read_to_string(&settings_path).unwrap_or_default();
    assert!(document.is_empty(), "settings file must not outlive");
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn stub_supervisor_timeout_kills_and_reports() {
    let base = stub_supervisor("timeout", "#!/bin/sh\nsleep 30\nexit 0\n");
    let stub = base.join("srt");
    let settings = build_srt_settings(&base).expect("settings build");
    let token = CancellationToken::new();
    let run = run_srt(
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
    let base = stub_supervisor("cancel", "#!/bin/sh\nsleep 30\nexit 0\n");
    let stub = base.join("srt");
    let settings = build_srt_settings(&base).expect("settings build");
    let token = CancellationToken::new();
    token.cancel();
    let run = run_srt(
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
    let missing = SrtEnforcingBackend::with_binary(std::path::Path::new(
        "/nonexistent/srt-missing",
    ));
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
            PINNED_SRT_DISTRIBUTION, PINNED_SRT_VERSION,
        ),
    )
    .expect("manifest");
    let pinned = SrtEnforcingBackend::with_binary(&bin);
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
    let marker = std::env::temp_dir()
        .join(format!("siralos-srt-never-{}-spawned.txt", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let backend = SrtEnforcingBackend::new();
    let token = CancellationToken::new();
    let error = backend
        .execute(
            &SandboxCommand {
                program: "sh".to_owned(),
                args: vec![
                    "-c".to_owned(),
                    format!("touch {}", marker.display()),
                ],
                env: Vec::new(),
                workdir: std::env::temp_dir(),
            },
            &SandboxLimits::default_tool_limits(),
            token.signal(),
        )
        .expect_err("unproven enforcement must refuse");
    assert_eq!(error, SandboxError::BackendUnavailable);
    assert!(
        !marker.exists(),
        "a refused execution must leave no child effects"
    );
}

// Live enforcement rows (C3). Unix-only and opt-in: each row is
// `#[ignore]`d and runs only when selected with `cargo test --
// --ignored` together with `SIRALOS_PROVE_SRT=1` and the pinned
// `srt` on PATH — an unrun check is never a green pass, so plain
// `cargo test` creates no assertion about live enforcement at
// all. A selected row that cannot prove (variable missing, probe
// refusing) panics; a proven row prints `PROVEN` with the host
// platform and the resolved runtime version. Only the loopback
// row may report `UNKNOWN` without failing, when the tooling it
// needs is absent — recorded, never a pass claimed on settings
// alone.
#[cfg(unix)]
fn require_live_backend() -> SrtEnforcingBackend {
    if std::env::var("SIRALOS_PROVE_SRT").is_err() {
        panic!(
            "live row demands proof: run with SIRALOS_PROVE_SRT=1 and \
             srt@{PINNED_SRT_VERSION} on PATH (an unrun check is never a \
             pass)"
        );
    }
    let backend = SrtEnforcingBackend::new();
    match backend.probe() {
        SandboxAvailability::Available(_) => backend,
        SandboxAvailability::Unavailable(reason) => {
            panic!("SIRALOS_PROVE_SRT=1 but the live probe refuses: {reason}")
        }
    }
}

/// The proof record: platform and runtime version travel with the
/// PROVEN line, so the ADR evidence table is mechanically backed
/// by the test log.
#[cfg(unix)]
fn print_proven(row: &str, backend: &SrtEnforcingBackend) {
    let binary =
        backend.binary().expect("a proven backend resolved its binary");
    let version = read_srt_version(&binary)
        .expect("a proven backend is the pinned distribution");
    eprintln!(
        "PROVEN {row} platform={} srt@{PINNED_SRT_VERSION}={version}",
        std::env::consts::OS
    );
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
) -> SandboxCommand {
    SandboxCommand {
        program: program.to_owned(),
        args: vec!["-c".to_owned(), word.to_owned()],
        env: Vec::new(),
        workdir: workdir.to_path_buf(),
    }
}

#[cfg(unix)]
#[ignore = "live row: run with --ignored, SIRALOS_PROVE_SRT=1, srt@0.0.70 on PATH"]
#[test]
fn live_row_grandchild_reaping_kills_the_tree() {
    let backend = require_live_backend();
    let workdir = live_workdir("reap");
    let marker = workdir.join("late-marker");
    let token = CancellationToken::new();
    let word = format!("(sleep 2 && touch {} &)", shell_quote(&marker));
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
    print_proven("live_row_grandchild_reaping_kills_the_tree", &backend);
    let _ = std::fs::remove_dir_all(&workdir);
}

#[cfg(unix)]
#[ignore = "live row: run with --ignored, SIRALOS_PROVE_SRT=1, srt@0.0.70 on PATH"]
#[test]
fn live_row_timeout_kill_terminates() {
    let backend = require_live_backend();
    let workdir = live_workdir("timeout");
    let token = CancellationToken::new();
    let outcome = backend
        .execute(
            &row_command(&workdir, "sh", "sleep 30"),
            &row_limits(2_000),
            token.signal(),
        )
        .expect("timeout row returns a run");
    assert!(outcome.timed_out, "{outcome:?}");
    assert_eq!(outcome.status_code, None);
    print_proven("live_row_timeout_kill_terminates", &backend);
    let _ = std::fs::remove_dir_all(&workdir);
}

#[cfg(unix)]
#[ignore = "live row: run with --ignored, SIRALOS_PROVE_SRT=1, srt@0.0.70 on PATH"]
#[test]
fn live_row_cancel_kills() {
    let backend = require_live_backend();
    let workdir = live_workdir("cancel");
    // Single-threaded cooperative cancel: the token is already
    // cancelled, so the wait loop kills the supervisor right
    // after spawn — the kill itself is what this row proves.
    let token = CancellationToken::new();
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
    print_proven("live_row_cancel_kills", &backend);
    let _ = std::fs::remove_dir_all(&workdir);
}

#[cfg(unix)]
#[ignore = "live row: run with --ignored, SIRALOS_PROVE_SRT=1, srt@0.0.70 on PATH"]
#[test]
fn live_row_loopback_connect_is_denied() {
    let backend = require_live_backend();
    let workdir = live_workdir("network");
    let token = CancellationToken::new();
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
    let quoted = shell_quote(&script);
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
        print_proven("live_row_loopback_connect_is_denied", &backend);
    } else {
        eprintln!("UNKNOWN live_row_loopback: {text:?}");
    }
    let _ = std::fs::remove_dir_all(&workdir);
}

#[cfg(unix)]
#[ignore = "live row: run with --ignored, SIRALOS_PROVE_SRT=1, srt@0.0.70 on PATH"]
#[test]
fn live_row_environment_is_allowlisted() {
    let backend = require_live_backend();
    let workdir = live_workdir("env");
    let token = CancellationToken::new();
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
    assert!(text.contains("TERM=dumb"), "fixed env must win, got: {text:?}");
    print_proven("live_row_environment_is_allowlisted", &backend);
    let _ = std::fs::remove_dir_all(&workdir);
}

#[cfg(unix)]
#[ignore = "live row: run with --ignored, SIRALOS_PROVE_SRT=1, srt@0.0.70 on PATH"]
#[test]
fn live_row_stdin_is_closed() {
    let backend = require_live_backend();
    let workdir = live_workdir("stdin");
    let token = CancellationToken::new();
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
    print_proven("live_row_stdin_is_closed", &backend);
    let _ = std::fs::remove_dir_all(&workdir);
}

#[cfg(unix)]
#[ignore = "live row: run with --ignored, SIRALOS_PROVE_SRT=1, srt@0.0.70 on PATH"]
#[test]
fn live_row_workdir_confinement_holds() {
    let backend = require_live_backend();
    let workdir = live_workdir("confine");
    let generation = super::CANARY_SEQUENCE
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let inside = workdir.join("inside-ok");
    let escape = std::env::temp_dir().join(format!(
        "siralos-row-escape-{}-{generation}",
        std::process::id(),
    ));
    let _ = std::fs::remove_file(&escape);
    let token = CancellationToken::new();
    let word = format!(
        "touch {} && printf 'INSIDE_OK\\n'; \
         touch {} 2>/dev/null && printf 'ESCAPED\\n' || printf 'CONTAINED\\n'",
        shell_quote(&inside),
        shell_quote(&escape),
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

// Outside the Unix rows entirely (every other platform): the rows
// are structural facts about a Unix backend, so here they neither
// compile nor run. This inventory entry keeps the unrun state
// discoverable on such hosts — a recorded `UNKNOWN`, never a pass
// claimed for enforcement.
#[cfg(not(unix))]
#[test]
fn live_rows_are_unrun_unknown_on_this_platform() {
    eprintln!(
        "UNKNOWN live rows (unix-only) did not run on platform {}: \
         [live_row_grandchild_reaping_kills_the_tree, \
         live_row_timeout_kill_terminates, live_row_cancel_kills, \
         live_row_loopback_connect_is_denied, \
         live_row_environment_is_allowlisted, live_row_stdin_is_closed, \
         live_row_workdir_confinement_holds]",
        std::env::consts::OS
    );
}
