//! Argument parsing, interactive rendering, and harness logic for the
//! `siralos` binaries.
//!
//! The default binary path composes the read-only R7.4 configuration,
//! deterministic provider, workspace Tools, and R7.3 projection before
//! handing presentation to the CLI-owned interactive session. The
//! projection and permission semantics remain in `siralos-core`.
//!
//! The ADR 0033 differential harness is **not** part of this crate: it
//! lives in the excluded `harness/` workspace, which is also the only
//! component that depends on the external Godot plugin crate. This crate
//! has no feature flags and no external domain dependency, so a bare
//! `git clone` builds the product without a sibling checkout.

pub mod configuration;
pub mod evaluation;
pub mod headless;
pub mod interactive;
pub mod output;
pub mod sanitize;
pub mod session_worker;
pub mod tui;

use std::ffi::OsString;
use std::path::PathBuf;

/// Outcome of parsing the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Start the interactive terminal session (auto: TUI on TTY, stdio otherwise).
    Interactive,
    /// Start the interactive terminal session forced to stdio (`--stdio`).
    Stdio,
    /// Print the version and exit successfully.
    Version,
    /// Print usage and exit successfully.
    Help,
    /// Run one prompt without any interactive frontend (`--print`).
    Headless(HeadlessArgs),
}

/// Arguments of the headless (`--print`) invocation.
///
/// Headless mode grants no authority of its own: it composes the same
/// session the interactive frontends compose and drains exactly one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlessArgs {
    prompt: String,
    json: bool,
    workspace_root: Option<PathBuf>,
}

impl HeadlessArgs {
    /// The prompt to send.
    #[must_use]
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// Emit one structured record instead of the answer text.
    #[must_use]
    pub fn json(&self) -> bool {
        self.json
    }

    /// The workspace root, or `None` for the process working directory.
    #[must_use]
    pub fn workspace_root(&self) -> Option<&std::path::Path> {
        self.workspace_root.as_deref()
    }
}

/// Parse the argument vector, excluding the program name.
///
/// Grammar:
///
/// ```text
/// siralos [--help | --version | --stdio]
/// siralos --print <prompt> [--json] [--cwd <dir>]
/// ```
///
/// `--print` takes the following argument verbatim as its prompt, so a flag
/// written after a completed prompt is still a flag. Repeating `--print`,
/// `--cwd` or `--json` is a usage error rather than last-wins. `--` ends flag
/// parsing; the grammar has no positional arguments, so a token after `--` is
/// a usage error. The inspection flags never combine with headless flags.
///
/// # Errors
///
/// Returns [`UsageError`] when an argument is not valid UTF-8 (the usage
/// boundary is the only place the CLI must convert to text), a flag is
/// unknown, a flag is repeated, a flag that requires a value has none, or two
/// mutually exclusive forms are combined.
pub fn parse_args<I>(args: I) -> Result<Command, UsageError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut inspection: Option<Command> = None;
    let mut prompt: Option<String> = None;
    let mut json = false;
    let mut workspace_root: Option<PathBuf> = None;
    let mut terminated = false;
    let mut args = args.into_iter();

    while let Some(raw) = args.next() {
        let text = raw
            .to_str()
            .ok_or_else(|| UsageError::new("argument is not valid UTF-8"))?;
        if terminated {
            return Err(UsageError::new(format!(
                "unexpected argument `{text}` after `--`"
            )));
        }
        match text {
            "--" => terminated = true,
            "-h" | "--help" => {
                set_inspection(&mut inspection, Command::Help, text)?;
            }
            "-V" | "--version" => {
                set_inspection(&mut inspection, Command::Version, text)?;
            }
            "--stdio" => {
                set_inspection(&mut inspection, Command::Stdio, text)?;
            }
            "-p" | "--print" => {
                if prompt.is_some() {
                    return Err(UsageError::new(
                        "`--print` was given more than once",
                    ));
                }
                prompt = Some(flag_value(&mut args, text)?);
            }
            "--cwd" => {
                if workspace_root.is_some() {
                    return Err(UsageError::new(
                        "`--cwd` was given more than once",
                    ));
                }
                workspace_root =
                    Some(PathBuf::from(flag_value(&mut args, text)?));
            }
            "--json" => {
                if json {
                    return Err(UsageError::new(
                        "`--json` was given more than once",
                    ));
                }
                json = true;
            }
            other => {
                return Err(UsageError::new(format!(
                    "unknown argument `{other}`"
                )));
            }
        }
    }

    if inspection.is_some()
        && (prompt.is_some() || json || workspace_root.is_some())
    {
        return Err(UsageError::new(
            "`--help`, `--version` and `--stdio` cannot be combined with `--print`, `--json` or `--cwd`",
        ));
    }
    if let Some(command) = inspection {
        return Ok(command);
    }
    if let Some(prompt) = prompt {
        return Ok(Command::Headless(HeadlessArgs {
            prompt,
            json,
            workspace_root,
        }));
    }
    if json || workspace_root.is_some() {
        return Err(UsageError::new("`--json` and `--cwd` require `--print`"));
    }
    Ok(Command::Interactive)
}

/// Record an inspection flag, refusing a second one.
fn set_inspection(
    slot: &mut Option<Command>,
    command: Command,
    flag: &str,
) -> Result<(), UsageError> {
    if slot.is_some() {
        return Err(UsageError::new(format!(
            "`{flag}` cannot be combined with another inspection flag"
        )));
    }
    *slot = Some(command);
    Ok(())
}

/// The verbatim value of a flag that requires one.
fn flag_value<I>(args: &mut I, flag: &str) -> Result<String, UsageError>
where
    I: Iterator<Item = OsString>,
{
    let raw = args.next().ok_or_else(|| {
        UsageError::new(format!("`{flag}` requires a value"))
    })?;
    raw.to_str().map(str::to_owned).ok_or_else(|| {
        UsageError::new(format!("the value of `{flag}` is not valid UTF-8"))
    })
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
    fn no_arguments_starts_interactive_session() {
        assert_eq!(
            parse_args(args(&[])).expect("valid"),
            Command::Interactive
        );
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
    fn stdio_flag_forces_the_stdio_frontend() {
        // Decision 105 A2: `--stdio` forces stdio regardless of TTY.
        assert_eq!(
            parse_args(args(&["--stdio"])).expect("valid"),
            Command::Stdio
        );
    }

    #[test]
    fn removed_tui_flag_is_rejected() {
        // Decision 105 A2: the one-commit `--tui` flag is removed.
        let error = parse_args(args(&["--tui"])).expect_err("must fail");
        assert!(error.detail().contains("--tui"));
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

    #[test]
    fn print_takes_the_next_argument_as_the_prompt() {
        match parse_args(args(&["--print", "hello"])).expect("valid") {
            Command::Headless(headless) => {
                assert_eq!(headless.prompt(), "hello");
                assert!(!headless.json());
                assert!(headless.workspace_root().is_none());
            }
            other => panic!("expected headless, got {other:?}"),
        }
    }

    #[test]
    fn short_print_flag_is_equivalent() {
        assert_eq!(
            parse_args(args(&["-p", "x"])).expect("valid"),
            parse_args(args(&["--print", "x"])).expect("valid")
        );
    }

    #[test]
    fn json_and_cwd_modify_the_headless_invocation() {
        let command =
            parse_args(args(&["--json", "--cwd", "/tmp/ws", "-p", "x"]))
                .expect("valid");
        match command {
            Command::Headless(headless) => {
                assert!(headless.json());
                assert_eq!(
                    headless.workspace_root(),
                    Some(std::path::Path::new("/tmp/ws"))
                );
            }
            other => panic!("expected headless, got {other:?}"),
        }
    }

    #[test]
    fn a_flag_after_a_completed_prompt_is_still_a_flag() {
        // `--print` consumes exactly one value, so a trailing flag is parsed.
        match parse_args(args(&["-p", "hello", "--json"])).expect("valid") {
            Command::Headless(headless) => assert!(headless.json()),
            other => panic!("expected headless, got {other:?}"),
        }
    }

    #[test]
    fn repeated_headless_flags_are_rejected() {
        for input in [
            vec!["-p", "a", "-p", "b"],
            vec!["-p", "a", "--cwd", "x", "--cwd", "y"],
            vec!["-p", "a", "--json", "--json"],
        ] {
            let error = parse_args(args(&input)).expect_err("must fail");
            assert!(
                error.detail().contains("more than once"),
                "{}",
                error.detail()
            );
        }
    }

    #[test]
    fn a_flag_without_its_value_is_rejected() {
        for input in [vec!["-p"], vec!["--cwd"]] {
            let error = parse_args(args(&input)).expect_err("must fail");
            assert!(
                error.detail().contains("requires a value"),
                "{}",
                error.detail()
            );
        }
    }

    #[test]
    fn headless_output_flags_require_print() {
        for input in [vec!["--json"], vec!["--cwd", "x"]] {
            let error = parse_args(args(&input)).expect_err("must fail");
            assert!(
                error.detail().contains("require `--print`"),
                "{}",
                error.detail()
            );
        }
    }

    #[test]
    fn inspection_flags_do_not_combine_with_headless_flags() {
        for input in [
            vec!["-p", "x", "--help"],
            vec!["--version", "-p", "x"],
            vec!["--stdio", "--json", "-p", "x"],
        ] {
            let error = parse_args(args(&input)).expect_err("must fail");
            assert!(
                error.detail().contains("cannot be combined"),
                "{}",
                error.detail()
            );
        }
    }

    #[test]
    fn a_token_after_the_double_dash_is_rejected() {
        // The grammar has no positional arguments, so `--` cannot introduce one.
        assert_eq!(
            parse_args(args(&["--"])).expect("valid"),
            Command::Interactive
        );
        let error = parse_args(args(&["--", "stray"])).expect_err("must fail");
        assert!(error.detail().contains("after `--`"), "{}", error.detail());
    }
}
