//! Entry point of the `siralos-harness` binary.
//!
//! The differential behavioral harness candidate runner (ADR 0033):
//! executes the scenario corpus against the Siralos Rust candidate and
//! emits canonical outcome records for the comparator. Subcommands:
//! - `run --corpus <dir> --root <repo> --out <file>` — run the corpus;
//! - `probe-state-dir` — internal probe subprocess (spawned with a
//!   scrubbed environment by `run`), prints the resolved state dir or
//!   the marker `ERR`.
//!
//! Exit codes: 0 = success, 2 = harness error.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use siralos_cli::harness;

fn usage() -> ExitCode {
    eprintln!(
        "usage: siralos-harness run --corpus <dir> --root <repo> --out <file> [--scenario <id>]\n       \
         siralos-harness probe-state-dir"
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => run_command(&args[1..]),
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
