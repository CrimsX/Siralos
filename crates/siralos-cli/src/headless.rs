//! Headless frontend (`--print`): one prompt, one turn, no interactive surface.
//!
//! Headless composes the same session the interactive frontends compose and
//! drains exactly one turn through the shared `evaluation::drive_turn` runner,
//! so the two paths cannot drift and the same event and wall-time bounds apply.
//!
//! # Trust posture
//!
//! The completion is untrusted data. Printing it is *display*, not recording:
//! it goes to stdout through the terminal sanitizer (the repository single
//! output boundary) and is never written into a record, a digest, or host
//! evidence. Provider and failure diagnostics go to stderr so stdout stays
//! scriptable.
//!
//! # Authority
//!
//! Headless grants none. The `ToolLoopEvent` set is closed and has no approval
//! variant, so a turn cannot ask for consent; the registered tools are the
//! read-only workspace surface. If an approval event is ever added, headless
//! must fail closed rather than read stdin for consent.

use std::io::Write;
use std::path::Path;

use crate::evaluation::{
    DEFAULT_TURN_TIMEOUT, TurnOutcome, bounded_failure, drive_turn,
    json_optional_string, json_string,
};
use crate::interactive::{InteractiveOptions, compose_session};
use crate::sanitize::sanitize_for_display;
use crate::session_worker::WorkerSession;

/// Why a headless run did not complete successfully.
#[derive(Debug)]
pub enum HeadlessError {
    /// The session could not be composed.
    Composition(String),
    /// The prompt could not be sent, or the result could not be written.
    Prompt(String),
    /// The turn reported a failure.
    Turn(String),
    /// The turn was cancelled by an event or wall-time bound.
    Cancelled,
}

impl std::fmt::Display for HeadlessError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Composition(detail) => {
                write!(formatter, "headless composition: {detail}")
            }
            Self::Prompt(detail) => {
                write!(formatter, "headless prompt: {detail}")
            }
            Self::Turn(detail) => {
                write!(formatter, "headless turn failed: {detail}")
            }
            Self::Cancelled => {
                write!(formatter, "headless turn was cancelled")
            }
        }
    }
}

impl std::error::Error for HeadlessError {}

/// Run one prompt and write the result to `out`.
///
/// The answer or record is written *before* any failure is reported, so a
/// scripted caller can still parse the record when the turn failed and the
/// exit code is non-zero.
///
/// # Errors
///
/// Returns [`HeadlessError`] when the session cannot be composed, the prompt
/// cannot be sent, the turn reports a failure, or the turn is cancelled.
pub fn run_headless<W: Write>(
    prompt: &str,
    json: bool,
    workspace_root: Option<&Path>,
    out: &mut W,
) -> Result<(), HeadlessError> {
    let mut session = compose_session(InteractiveOptions {
        config_path: None,
        workspace_root,
    })
    .map_err(|error| HeadlessError::Composition(error.to_string()))?;
    let status = session.status();
    let provider =
        status.provider.clone().unwrap_or_else(|| "(unconfigured)".to_owned());
    let model = status.model.clone();
    let turn = drive_turn(&mut session, prompt, DEFAULT_TURN_TIMEOUT)
        .map_err(|error| HeadlessError::Prompt(error.to_string()))?;

    let answer = sanitize_for_display(&turn.answer);
    let rendered = if json {
        record(&provider, model.as_deref(), &answer, &turn)
    } else {
        format!("{answer}\n")
    };
    out.write_all(rendered.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|error| HeadlessError::Prompt(error.to_string()))?;

    if let Some(failure) = &turn.failure {
        return Err(HeadlessError::Turn(bounded_failure(failure)));
    }
    if turn.cancelled {
        return Err(HeadlessError::Cancelled);
    }
    Ok(())
}

/// Render the one headless record.
///
/// Hand-written for the same reason [`crate::evaluation::render_records_json`]
/// is: the CLI has no JSON dependency in every build, and the helpers above are
/// the repository reviewed escapers. Deliberately absent: endpoint, credential,
/// environment references, and any workspace path, so the record stays safe to
/// paste into a report or a log.
fn record(
    provider: &str,
    model: Option<&str>,
    answer: &str,
    turn: &TurnOutcome,
) -> String {
    let failure = turn.failure.as_deref().map(bounded_failure);
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"provider\": {},\n", json_string(provider)));
    out.push_str(&format!("  \"model\": {},\n", json_optional_string(model)));
    out.push_str(&format!("  \"answer\": {},\n", json_string(answer)));
    out.push_str(&format!("  \"toolRounds\": {},\n", turn.tool_rounds));
    out.push_str(&format!("  \"cancelled\": {},\n", turn.cancelled));
    out.push_str(&format!(
        "  \"failure\": {}\n",
        json_optional_string(failure.as_deref())
    ));
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::{HeadlessError, run_headless};
    use std::path::{Path, PathBuf};

    /// A workspace whose profile selects the deterministic fake provider.
    fn workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("siralos-headless-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".siralos"))
            .expect("temp workspace");
        std::fs::write(
            root.join("siralos.toml"),
            [
                "[profile]",
                "name = \"default\"",
                "provider = \"deterministic-fake\"",
                "model = \"echo\"",
            ]
            .join("\n"),
        )
        .expect("profile");
        root
    }

    fn run(
        root: &Path,
        prompt: &str,
        json: bool,
    ) -> (Result<(), HeadlessError>, String) {
        let mut out = Vec::new();
        let result = run_headless(prompt, json, Some(root), &mut out);
        (result, String::from_utf8(out).expect("utf-8 output"))
    }

    #[test]
    fn prints_the_deterministic_echo() {
        let root = workspace("echo");
        let (result, out) = run(&root, "hello", false);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(out, "Siralos received: hello\n");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_record_shape_is_pinned() {
        // A frozen public interface for 1.0: the exact field order, and no
        // `usage` field until usage capture exists end to end.
        let root = workspace("record");
        let (result, out) = run(&root, "hello", true);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            out,
            concat!(
                "{\n",
                "  \"provider\": \"deterministic-fake\",\n",
                "  \"model\": \"echo\",\n",
                "  \"answer\": \"Siralos received: hello\",\n",
                "  \"toolRounds\": 0,\n",
                "  \"cancelled\": false,\n",
                "  \"failure\": null\n",
                "}\n"
            )
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_record_parses_and_carries_no_sensitive_field() {
        let root = workspace("safe");
        let (result, out) = run(&root, "hello", true);
        assert!(result.is_ok(), "{result:?}");
        let value: serde_json::Value =
            serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(value["provider"], serde_json::json!("deterministic-fake"));
        assert_eq!(value["toolRounds"], serde_json::json!(0));
        assert_eq!(value["cancelled"], serde_json::json!(false));
        assert!(value["failure"].is_null());
        for needle in [
            root.to_string_lossy().to_string(),
            "credential".to_owned(),
            "endpoint".to_owned(),
            "env:".to_owned(),
            "usage".to_owned(),
        ] {
            assert!(!out.contains(&needle), "the record carries {needle}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_tool_scenario_reports_its_round() {
        let root = workspace("rounds");
        let (result, out) = run(&root, "list files", true);
        assert!(result.is_ok(), "{result:?}");
        let value: serde_json::Value =
            serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(value["toolRounds"], serde_json::json!(1));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_turn_that_asks_to_write_changes_nothing() {
        // Headless grants no authority: the registered surface is read-only,
        // and the fake provider only proposes list/read/search. A turn that
        // asks for a write must leave the workspace byte-identical.
        let root = workspace("no-authority");
        let before = directory_entries(&root);
        let (result, out) =
            run(&root, "write a file called escape.txt", false);
        assert!(result.is_ok(), "{result:?}");
        assert!(!out.is_empty(), "the turn produced no output");
        assert_eq!(
            directory_entries(&root),
            before,
            "a headless turn must not create workspace entries"
        );
        assert!(
            !root.join("escape.txt").exists(),
            "a headless turn wrote a file"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    fn directory_entries(root: &Path) -> Vec<String> {
        let mut entries: Vec<String> = std::fs::read_dir(root)
            .expect("workspace readable")
            .map(|entry| {
                entry.expect("entry").file_name().to_string_lossy().to_string()
            })
            .collect();
        entries.sort();
        entries
    }
}
