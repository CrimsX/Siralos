//! Argument parsing and harness logic for the `siralos` binaries.
//!
//! R1 exposes only the version and usage surfaces needed to prove the
//! binary identity and the workspace dependency direction. The
//! interactive terminal session of the TypeScript reference
//! implementation is not ported yet.
//!
//! With the internal `differential-harness` feature enabled, the ADR 0033
//! candidate runner lives in the `harness` module and is exercised by the
//! `siralos-harness` binary. Neither surface is part of the default product
//! build.

pub mod configuration;

#[cfg(feature = "differential-harness")]
pub mod harness;

use std::ffi::OsString;

/// Outcome of parsing the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Print the version and exit successfully.
    Version,
    /// Print usage and exit successfully.
    Help,
}

/// Parse the argument vector, excluding the program name.
///
/// # Errors
///
/// Returns [`UsageError`] when more than one argument is given, an
/// argument is not valid UTF-8 (the usage boundary is the only place the
/// CLI must convert to text), or the argument is not a supported flag.
pub fn parse_args<I>(args: I) -> Result<Command, UsageError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Command::Help);
    };
    if args.next().is_some() {
        return Err(UsageError::new("expected at most one argument"));
    }
    match first.to_str() {
        Some("--help") | Some("-h") => Ok(Command::Help),
        Some("--version") | Some("-V") => Ok(Command::Version),
        Some(other) => {
            Err(UsageError::new(format!("unknown argument `{other}`")))
        }
        None => Err(UsageError::new("argument is not valid UTF-8")),
    }
}

/// Invalid command-line invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    detail: String,
}

impl UsageError {
    /// A human-readable explanation of the invocation error.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    fn new(detail: impl Into<String>) -> Self {
        Self { detail: detail.into() }
    }
}

impl std::fmt::Display for UsageError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for UsageError {}

#[cfg(test)]
mod tests {
    use super::{Command, UsageError, parse_args};
    use std::ffi::OsString;

    fn args(input: &[&str]) -> Vec<OsString> {
        input.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_arguments_prints_help() {
        assert_eq!(parse_args(args(&[])).expect("valid"), Command::Help);
    }

    #[test]
    fn help_flags_are_accepted() {
        assert_eq!(
            parse_args(args(&["--help"])).expect("valid"),
            Command::Help
        );
        assert_eq!(parse_args(args(&["-h"])).expect("valid"), Command::Help);
    }

    #[test]
    fn version_flags_are_accepted() {
        assert_eq!(
            parse_args(args(&["--version"])).expect("valid"),
            Command::Version
        );
        assert_eq!(
            parse_args(args(&["-V"])).expect("valid"),
            Command::Version
        );
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        let error = parse_args(args(&["--unknown"])).expect_err("must fail");
        assert!(error.detail().contains("--unknown"));
        assert_eq!(error.to_string(), error.detail());
    }

    #[test]
    fn more_than_one_argument_is_rejected() {
        assert!(matches!(
            parse_args(args(&["--help", "--version"])),
            Err(UsageError { .. })
        ));
    }

    #[test]
    fn non_utf8_arguments_are_rejected() {
        #[cfg(not(windows))]
        use std::os::unix::ffi::OsStringExt;
        #[cfg(windows)]
        use std::os::windows::ffi::OsStringExt;

        #[cfg(not(windows))]
        let invalid = OsString::from_vec(vec![b'-', 0xFF]);
        #[cfg(windows)]
        let invalid = OsString::from_wide(&[0xD800u16, 0x00]);
        assert!(matches!(parse_args([invalid]), Err(UsageError { .. })));
    }
}
