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

use std::path::PathBuf;
use std::process::ExitCode;

use siralos_cli::harness;

fn usage() -> ExitCode {
    eprintln!(
        "usage: siralos-harness run --corpus <dir> --root <repo> --out <file>\n       \
         siralos-harness probe-state-dir"
    );
    ExitCode::from(2)
}

fn option_value(args: &[String], name: &str) -> Option<String> {
    let index = args.iter().position(|arg| arg == name)?;
    args.get(index + 1).cloned()
}

fn run_command(args: &[String]) -> ExitCode {
    let corpus = option_value(args, "--corpus");
    let root = option_value(args, "--root");
    let out = option_value(args, "--out");
    let (Some(corpus), Some(root), Some(out)) = (corpus, root, out) else {
        return usage();
    };
    let result =
        harness::run_corpus(&PathBuf::from(corpus), &PathBuf::from(root));
    match result {
        Ok(records) => {
            if let Some(parent) = PathBuf::from(&out).parent() {
                if let Err(error) = std::fs::create_dir_all(parent) {
                    eprintln!(
                        "siralos-harness: cannot create output directory: {error}"
                    );
                    return ExitCode::from(2);
                }
            }
            if let Err(error) = std::fs::write(&out, records) {
                eprintln!("siralos-harness: cannot write {out}: {error}");
                return ExitCode::from(2);
            }
            println!("candidate: wrote {out}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("siralos-harness: {error}");
            ExitCode::from(2)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => run_command(&args[1..]),
        Some("probe-state-dir") => {
            print!("{}", harness::probe_state_dir());
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}
