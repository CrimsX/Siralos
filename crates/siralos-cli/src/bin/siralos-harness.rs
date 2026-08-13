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
        "usage: siralos-harness run --corpus <dir> --root <repo> --out <file>\n       \
         siralos-harness probe-state-dir"
    );
    ExitCode::from(2)
}

fn run_command(args: &[String]) -> ExitCode {
    let [corpus_flag, corpus, root_flag, root, out_flag, out] = args else {
        return usage();
    };
    if corpus_flag != "--corpus"
        || root_flag != "--root"
        || out_flag != "--out"
    {
        return usage();
    }
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
            if let Err(error) = std::fs::write(out, records) {
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
                    ExitCode::from(2)
                }
            }
        }
        _ => usage(),
    }
}
