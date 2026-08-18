//! Entry point of the `siralos` binary.
//!
//! The default invocation enters the synchronous R7.5 interactive session;
//! fixed flags remain deterministic, non-interactive inspection surfaces.

use std::env;
use std::process::ExitCode;

use siralos_cli::{Command, parse_args};

fn main() -> ExitCode {
    let args = env::args_os().skip(1);
    match parse_args(args) {
        Ok(Command::Interactive) => {
            match siralos_cli::interactive::run_interactive_stdio() {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("siralos: {error}");
                    ExitCode::from(1)
                }
            }
        }
        Ok(Command::Version) => print_version(),
        Ok(Command::Help) => {
            print_usage();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("siralos: {error}");
            eprintln!("Try `siralos --help` for usage.");
            ExitCode::from(2)
        }
    }
}

/// Print the product version and exit successfully.
fn print_version() -> ExitCode {
    // The package version is declared in Cargo.toml and is always a
    // strict three-part numeric version; a violation is a build-time
    // packaging error, not a runtime failure.
    let version = siralos_core::version::Version::parse(env!(
        "CARGO_PKG_VERSION"
    ))
    .expect(
        "the siralos-cli package version is always a valid numeric version",
    );
    println!("siralos {version}");
    ExitCode::SUCCESS
}

/// Print usage text and exit successfully.
fn print_usage() -> ExitCode {
    let state_dir = match siralos_adapters::paths::state_dir() {
        Ok(path) => path.display().to_string(),
        Err(error) => format!("<unavailable: {error}>"),
    };
    println!(
        "siralos: a deterministic, security-first software-development and QA harness\n\
         \n\
         USAGE:\n\
         \x20   siralos [--help | --version]\n\
         \n\
         OPTIONS:\n\
         \x20   -h, --help      Print this usage information\n\
         \x20   -V, --version   Print the version\n\
         \n\
         With no arguments, start the interactive terminal session.\n\
         \n\
         User state directory: {state_dir}"
    );
    ExitCode::SUCCESS
}
