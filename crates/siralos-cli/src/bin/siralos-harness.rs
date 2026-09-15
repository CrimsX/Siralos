//! Entry point of the `siralos-harness` binary.
//!
//! The differential behavioral harness candidate runner (ADR 0033):
//! executes the scenario corpus against the Siralos Rust candidate and
//! emits canonical outcome records for the comparator. Subcommands:
//! - `run --corpus <dir> --root <repo> --out <file>` — run the corpus;
//! - `probe-state-dir` — internal probe subprocess (spawned with a
//!   scrubbed environment by `run`), prints the resolved state dir or
//!   the marker `ERR`;
//! - `evaluate --run <label>=<dir> [--run ...] --out <file>` — ticket
//!   135's OWNER-RUN live multi-model evaluation: one session per named
//!   workspace (each configured by its own `siralos.toml`), one fixed
//!   task set, one INFORMATIONAL comparison. It spends the profiles' own
//!   provider budget, so it is never part of `npm run check`; the offline
//!   proof of the same machinery is a lib test.
//!
//! Exit codes: 0 = success, 2 = harness error.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use siralos_cli::harness;

fn usage() -> ExitCode {
    eprintln!(
        "usage: siralos-harness run --corpus <dir> --root <repo> --out <file> [--scenario <id>]\n       \
         siralos-harness probe-state-dir\n       \
         siralos-harness evaluate --run <label>=<workspace> [--run ...] --out <file> [--turn-timeout-ms <n>]"
    );
    report_error(
        "HARNESS_INVOCATION_FAILURE",
        "INVALID_ARGUMENTS",
        "runner arguments do not match the versioned invocation protocol",
    );
    ExitCode::from(2)
}

fn report_error(category: &str, code: &str, message: &str) {
    let diagnostic = serde_json::json!({
        "category": category,
        "code": code,
        "message": message,
    });
    eprintln!("SIRALOS_HARNESS_ERROR {diagnostic}");
}

fn run_command(args: &[String]) -> ExitCode {
    let (required, scenario_id) = match args {
        [corpus_flag, corpus, root_flag, root, out_flag, out] => {
            ([corpus_flag, corpus, root_flag, root, out_flag, out], None)
        }
        [
            corpus_flag,
            corpus,
            root_flag,
            root,
            out_flag,
            out,
            scenario_flag,
            scenario_id,
        ] if scenario_flag == "--scenario" => (
            [corpus_flag, corpus, root_flag, root, out_flag, out],
            Some(scenario_id.as_str()),
        ),
        _ => return usage(),
    };
    let [corpus_flag, corpus, root_flag, root, out_flag, out] = required;
    if corpus_flag != "--corpus"
        || root_flag != "--root"
        || out_flag != "--out"
    {
        return usage();
    }
    let result = harness::run_corpus(
        &PathBuf::from(corpus),
        &PathBuf::from(root),
        scenario_id,
    );
    match result {
        Ok(records) => {
            if let Some(parent) = PathBuf::from(&out).parent() {
                if let Err(error) = std::fs::create_dir_all(parent) {
                    eprintln!(
                        "siralos-harness: cannot create output directory: {error}"
                    );
                    report_error(
                        "HARNESS_INTERNAL_FAILURE",
                        "OUTPUT_CREATE_FAILURE",
                        "candidate runner could not create its output directory",
                    );
                    return ExitCode::from(2);
                }
            }
            if let Err(error) = std::fs::write(out, records) {
                eprintln!("siralos-harness: cannot write {out}: {error}");
                report_error(
                    "HARNESS_INTERNAL_FAILURE",
                    "OUTPUT_WRITE_FAILURE",
                    "candidate runner could not write its protocol document",
                );
                return ExitCode::from(2);
            }
            println!("candidate: wrote {out}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("siralos-harness: {error}");
            report_error(error.category(), error.code(), &error.to_string());
            ExitCode::from(2)
        }
    }
}

/// The owner-run live evaluation (ticket 135), never part of `npm run check`.
///
/// Each `--run` names a workspace whose `siralos.toml` configures the run, so
/// the provider, the model and the credential resolution are the profile's own
/// -- exactly what an interactive session would compose. The label is printed
/// for the owner to map runs to workspaces; it is never part of a record.
fn evaluate_command(args: &[String]) -> ExitCode {
    let mut runs: Vec<(String, PathBuf)> = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut turn_timeout = siralos_cli::evaluation::DEFAULT_TURN_TIMEOUT;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--run" => {
                let Some(value) = args.get(index + 1) else {
                    return usage();
                };
                let Some((label, path)) = value.split_once('=') else {
                    return usage();
                };
                if label.is_empty() || path.is_empty() {
                    return usage();
                }
                runs.push((label.to_owned(), PathBuf::from(path)));
                index += 2;
            }
            "--out" => {
                let Some(value) = args.get(index + 1) else {
                    return usage();
                };
                out = Some(PathBuf::from(value));
                index += 2;
            }
            "--turn-timeout-ms" => {
                let Some(millis) = args
                    .get(index + 1)
                    .and_then(|value| value.parse::<u64>().ok())
                else {
                    return usage();
                };
                turn_timeout = std::time::Duration::from_millis(millis);
                index += 2;
            }
            _ => return usage(),
        }
    }
    let Some(out) = out else {
        return usage();
    };
    if runs.is_empty() {
        return usage();
    }
    let corpus = siralos_cli::evaluation::evaluation_corpus();
    println!(
        "evaluate: task set {} against {} run(s), turn timeout {} ms",
        corpus.id,
        runs.len(),
        turn_timeout.as_millis()
    );
    let targets: Vec<siralos_cli::evaluation::EvaluationTarget> = runs
        .iter()
        .map(|(label, path)| {
            println!("evaluate: {label} = {}", path.display());
            let mut target =
                siralos_cli::evaluation::EvaluationTarget::new(path);
            target.turn_timeout = turn_timeout;
            target
        })
        .collect();
    let report =
        match siralos_cli::evaluation::evaluate_targets(&corpus, &targets) {
            Ok(report) => report,
            Err(error) => {
                eprintln!("siralos-harness: {error}");
                report_error(
                    "EVALUATION_FAILURE",
                    "RUN_FAILED",
                    "the evaluation could not be completed",
                );
                return ExitCode::from(2);
            }
        };
    print!("{}", report.rendered);
    if let Some(parent) = out.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            eprintln!(
                "siralos-harness: cannot create output directory: {error}"
            );
            report_error(
                "HARNESS_INTERNAL_FAILURE",
                "OUTPUT_CREATE_FAILURE",
                "candidate runner could not create its output directory",
            );
            return ExitCode::from(2);
        }
    }
    if let Err(error) = std::fs::write(
        &out,
        siralos_cli::evaluation::render_records_json(&report),
    ) {
        eprintln!("siralos-harness: cannot write {}: {error}", out.display());
        report_error(
            "HARNESS_INTERNAL_FAILURE",
            "OUTPUT_WRITE_FAILURE",
            "candidate runner could not write its evaluation record",
        );
        return ExitCode::from(2);
    }
    println!("evaluate: wrote {}", out.display());
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => run_command(&args[1..]),
        Some("evaluate") => evaluate_command(&args[1..]),
        Some("probe-state-dir") if args.len() == 1 => {
            match harness::probe_state_dir_bytes() {
                Ok(bytes) => {
                    if let Err(error) = std::io::stdout().write_all(&bytes) {
                        eprintln!(
                            "siralos-harness: cannot write probe outcome: {error}"
                        );
                        return ExitCode::from(2);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("siralos-harness: {error}");
                    report_error(
                        error.category(),
                        error.code(),
                        &error.to_string(),
                    );
                    ExitCode::from(2)
                }
            }
        }
        _ => usage(),
    }
}
