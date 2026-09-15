//! Multi-model evaluation runs (ticket 135) -- the runner half.
//!
//! One run composes ONE session through the same [`compose_session`] both
//! frontends call (so an evaluation cannot see a session the product does not),
//! drives one fixed, digest-bound task set through it, and records what happened.
//! [`evaluate_targets`] runs several targets and compares them.
//!
//! Three properties are deliberate:
//!
//! - **Informational.** A run records; it never gates, scores or blocks a live
//!   session, and nothing it computes reaches a threshold (the rule decision 94
//!   set for the estimator comparison, applied to runs).
//! - **Headless.** The drain below renders nothing: it reads the same
//!   [`WorkerSession`] seam the frontends read and keeps only what the record
//!   needs. There is no second render path to fork, because there is no render
//!   here at all.
//! - **Report-safe.** A record carries provider and model labels, counts and
//!   bounded, sanitized failure summaries. An endpoint, a credential or a
//!   workspace path has no field to travel in, and the completion text is never
//!   stored: a mismatching case is summarised, not quoted.
//!
//! The offline proof below runs three targets -- the deterministic fake plus two
//! recorded replay stores standing in for two models -- entirely inside
//! `npm run check`. A live run is the owner's, with the owner's budget.

use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use siralos_core::evaluation::{
    ComparisonTable, MAX_FAILURE_BYTES, MAX_RUN_FAILURES, RunOutcome,
    compare_runs, render_comparison, task_set_digest,
};
use siralos_core::evolution::{EvaluationCase, EvaluationCorpus};
use siralos_core::tool::ToolLoopEvent;

use crate::interactive::{InteractiveOptions, compose_session};
use crate::sanitize::sanitize_for_display;
use crate::session_worker::WorkerSession;

/// Stable id of the evaluation task set.
pub const EVALUATION_CORPUS_ID: &str = "siralos-evaluation-smoke";
/// Wall-time budget for ONE prompt turn, when a target does not set its own.
pub const DEFAULT_TURN_TIMEOUT: Duration = Duration::from_secs(120);
/// Event budget for ONE prompt turn.
pub const MAX_TURN_EVENTS: usize = 4096;
/// How long an idle poll waits before checking the turn deadline again.
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// The evaluation task set: three bounded cases with exact expectations.
///
/// Deliberately small and deliberately semantic. The three questions have one
/// correct answer each, so the only scoring rule needed is exact match -- no
/// fuzzy comparison, no judge model, nothing that could drift between runs.
/// The digest-bound identity of this set is
/// [`siralos_core::evaluation::task_set_digest`].
#[must_use]
pub fn evaluation_corpus() -> EvaluationCorpus {
    EvaluationCorpus {
        id: EVALUATION_CORPUS_ID.to_owned(),
        cases: vec![
            EvaluationCase {
                id: "arithmetic".to_owned(),
                prompt: "What is 1 + 1? Answer with the digit only."
                    .to_owned(),
                expected: "2".to_owned(),
            },
            EvaluationCase {
                id: "capital".to_owned(),
                prompt: "What is the capital of France? Answer with one word."
                    .to_owned(),
                expected: "Paris".to_owned(),
            },
            EvaluationCase {
                id: "json-object".to_owned(),
                prompt: "Return the JSON object with a single key ok set to true, and nothing else."
                    .to_owned(),
                expected: "{\"ok\": true}".to_owned(),
            },
        ],
    }
}

/// One evaluation target: the workspace whose `siralos.toml` configures the run.
///
/// The provider, the model and (for a replay target) the store all come from
/// that profile, read exactly as a real session reads it -- the runner applies no
/// identity of its own.
#[derive(Debug, Clone)]
pub struct EvaluationTarget {
    /// Workspace root the session is composed for.
    pub workspace_root: PathBuf,
    /// Wall-time budget for one prompt turn.
    pub turn_timeout: Duration,
}

impl EvaluationTarget {
    /// A target with the default turn budget.
    #[must_use]
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            turn_timeout: DEFAULT_TURN_TIMEOUT,
        }
    }
}

/// Why an evaluation could not be completed.
///
/// A case that merely mismatched is NOT an error: it is recorded, because that
/// record is the evidence. These variants mean the run itself could not happen.
#[derive(Debug)]
pub enum EvaluationRunError {
    /// The task set is malformed.
    Corpus(String),
    /// The session for one target could not be composed.
    Composition(String),
    /// A prompt turn could not be sent.
    Prompt(String),
    /// The runs could not be compared.
    Comparison(String),
}

impl fmt::Display for EvaluationRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corpus(message) => {
                write!(formatter, "evaluation task set: {message}")
            }
            Self::Composition(message) => {
                write!(formatter, "evaluation composition: {message}")
            }
            Self::Prompt(message) => {
                write!(formatter, "evaluation prompt: {message}")
            }
            Self::Comparison(message) => {
                write!(formatter, "evaluation comparison: {message}")
            }
        }
    }
}

impl std::error::Error for EvaluationRunError {}

/// The records of an evaluation plus the comparison over them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationReport {
    /// One record per target, in target order.
    pub runs: Vec<RunOutcome>,
    /// The INFORMATIONAL comparison over those records.
    pub table: ComparisonTable,
    /// The comparison as terminal-safe text, ready to print.
    pub rendered: String,
}

/// Run the task set against one target and record the outcome.
///
/// # Errors
///
/// Returns [`EvaluationRunError`] when the task set is malformed, the session
/// cannot be composed, or a prompt cannot be sent.
pub fn run_evaluation(
    corpus: &EvaluationCorpus,
    target: &EvaluationTarget,
) -> Result<RunOutcome, EvaluationRunError> {
    let corpus_digest = task_set_digest(corpus)
        .map_err(|error| EvaluationRunError::Corpus(error.message))?;
    let started = Instant::now();
    let mut session = compose_session(InteractiveOptions {
        config_path: None,
        workspace_root: Some(target.workspace_root.as_path()),
    })
    .map_err(|error| EvaluationRunError::Composition(error.to_string()))?;
    let status = session.status();
    let mut outcome = RunOutcome {
        provider: status
            .provider
            .clone()
            .unwrap_or_else(|| "(unconfigured)".to_owned()),
        model: status.model.clone(),
        corpus_id: corpus.id.clone(),
        corpus_digest,
        cases_run: 0,
        cases_passed: 0,
        turns: 0,
        tool_rounds: 0,
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
        failure_count: 0,
        failures: Vec::new(),
        cancelled: false,
        wall_ms: 0,
    };
    let mut cases = corpus.cases.clone();
    cases.sort_by(|left, right| left.id.cmp(&right.id));
    for case in &cases {
        let turn =
            drive_turn(&mut session, &case.prompt, target.turn_timeout)?;
        outcome.cases_run += 1;
        outcome.turns += 1;
        outcome.tool_rounds += turn.tool_rounds;
        if turn.cancelled {
            outcome.cancelled = true;
        }
        if let Some(failure) = &turn.failure {
            record_failure(&mut outcome, failure);
        }
        if turn.answer.trim_end() == case.expected {
            outcome.cases_passed += 1;
        } else {
            // The answer is NOT recorded: the completion is untrusted data and
            // can be arbitrarily long. The mismatch is the evidence.
            record_failure(
                &mut outcome,
                &format!("case {} did not match the expectation", case.id),
            );
        }
    }
    // Usage is whatever the provider reported through the recording seam, and
    // nothing when it reported none (absent stays absent, never a zero).
    if let Some(usage) = session.usage_totals() {
        outcome.input_tokens = usage.input_tokens;
        outcome.output_tokens = usage.output_tokens;
        outcome.cached_tokens = usage.cached_tokens;
    }
    // The single-owner flush (decision 78): a record-replay profile persists
    // its store here, and a session without a recorder flushes nothing.
    session.flush();
    outcome.wall_ms =
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(outcome)
}

/// Run the task set against every target and compare the records.
///
/// # Errors
///
/// Returns [`EvaluationRunError`] when any run fails or the comparison cannot
/// be built (an empty target list is a comparison error).
pub fn evaluate_targets(
    corpus: &EvaluationCorpus,
    targets: &[EvaluationTarget],
) -> Result<EvaluationReport, EvaluationRunError> {
    let mut runs = Vec::with_capacity(targets.len());
    for target in targets {
        runs.push(run_evaluation(corpus, target)?);
    }
    let table = compare_runs(&runs)
        .map_err(|error| EvaluationRunError::Comparison(error.message))?;
    let rendered = sanitize_for_display(&render_comparison(&table));
    Ok(EvaluationReport { runs, table, rendered })
}

/// Render the records as deterministic JSON (the `--out` artifact).
///
/// Hand-written on purpose: the CLI's JSON dependency is a harness-feature
/// dependency, and this report must exist in every build. Escaping is the
/// minimum JSON requires (quote, backslash, control characters); every value is
/// already bounded by the record it came from.
#[must_use]
pub fn render_records_json(report: &EvaluationReport) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"informational\": true,\n");
    out.push_str(&format!(
        "  \"corpusId\": {},\n",
        json_string(&report.table.corpus_id)
    ));
    out.push_str(&format!(
        "  \"corpusDigest\": {},\n",
        json_string(&report.table.corpus_digest)
    ));
    out.push_str(&format!("  \"comparable\": {},\n", report.table.comparable));
    out.push_str("  \"runs\": [\n");
    for (index, run) in report.runs.iter().enumerate() {
        let comma = if index + 1 == report.runs.len() { "" } else { "," };
        out.push_str("    {\n");
        out.push_str(&format!(
            "      \"provider\": {},\n",
            json_string(&run.provider)
        ));
        out.push_str(&format!(
            "      \"model\": {},\n",
            json_optional_string(run.model.as_deref())
        ));
        out.push_str(&format!(
            "      \"taskSetDigest\": {},\n",
            json_string(&run.corpus_digest)
        ));
        out.push_str(&format!("      \"casesRun\": {},\n", run.cases_run));
        out.push_str(&format!(
            "      \"casesPassed\": {},\n",
            run.cases_passed
        ));
        out.push_str(&format!("      \"turns\": {},\n", run.turns));
        out.push_str(&format!("      \"toolRounds\": {},\n", run.tool_rounds));
        out.push_str(&format!(
            "      \"inputTokens\": {},\n",
            json_optional_u64(run.input_tokens)
        ));
        out.push_str(&format!(
            "      \"outputTokens\": {},\n",
            json_optional_u64(run.output_tokens)
        ));
        out.push_str(&format!(
            "      \"cachedTokens\": {},\n",
            json_optional_u64(run.cached_tokens)
        ));
        out.push_str(&format!(
            "      \"failureCount\": {},\n",
            run.failure_count
        ));
        out.push_str(&format!("      \"cancelled\": {},\n", run.cancelled));
        out.push_str(&format!("      \"wallMs\": {},\n", run.wall_ms));
        out.push_str("      \"failures\": [");
        for (failure_index, failure) in run.failures.iter().enumerate() {
            if failure_index > 0 {
                out.push_str(", ");
            }
            out.push_str(&json_string(failure));
        }
        out.push_str("]\n");
        out.push_str(&format!("    }}{comma}\n"));
    }
    out.push_str("  ],\n");
    out.push_str(&format!(
        "  \"comparison\": {}\n",
        json_string(&report.rendered)
    ));
    out.push_str("}\n");
    out
}

/// What one prompt turn produced.
#[derive(Debug, Default)]
struct TurnOutcome {
    /// Completion text, kept for scoring only and never stored.
    answer: String,
    /// Tool rounds observed in the turn.
    tool_rounds: usize,
    /// The turn was cancelled.
    cancelled: bool,
    /// A failure the turn reported, or one the runner detected.
    failure: Option<String>,
}

/// Send one prompt and drain the turn to its end.
///
/// The turn is bounded twice: by event count (a runaway stream) and by wall time
/// (a provider that never settles). Exceeding either cancels the turn and records
/// a failure, so an evaluation can never hang the gate. The wall-time check
/// happens between events, so one blocking provider call is not interrupted.
fn drive_turn<S: WorkerSession>(
    session: &mut S,
    prompt: &str,
    timeout: Duration,
) -> Result<TurnOutcome, EvaluationRunError> {
    session.send_prompt(prompt).map_err(EvaluationRunError::Prompt)?;
    let mut outcome = TurnOutcome::default();
    let deadline = Instant::now() + timeout;
    let mut events = 0usize;
    while session.is_responding() {
        if let Some(event) = session.poll_event() {
            events += 1;
            if events > MAX_TURN_EVENTS {
                session.cancel();
                outcome.cancelled = true;
                outcome.failure = Some(format!(
                    "turn exceeded {MAX_TURN_EVENTS} events and was cancelled"
                ));
                break;
            }
            match event {
                ToolLoopEvent::TextDelta { text } => {
                    outcome.answer.push_str(&text);
                }
                ToolLoopEvent::ToolStarted { .. } => {
                    // R7.2 pairs one call with one result per round, so one
                    // started call is one round.
                    outcome.tool_rounds += 1;
                }
                ToolLoopEvent::ResponseFailed { message }
                | ToolLoopEvent::ToolFailed { message, .. } => {
                    outcome.failure = Some(message);
                }
                ToolLoopEvent::ResponseCancelled => {
                    outcome.cancelled = true;
                }
                ToolLoopEvent::ReasoningDelta { .. }
                | ToolLoopEvent::ResponseStarted
                | ToolLoopEvent::ResponseCompleted
                | ToolLoopEvent::ToolCompleted { .. }
                | ToolLoopEvent::ToolCancelled { .. }
                | ToolLoopEvent::ProviderPending
                | ToolLoopEvent::ContextPressure { .. } => {}
            }
        } else if Instant::now() >= deadline {
            session.cancel();
            outcome.cancelled = true;
            outcome.failure = Some(format!(
                "turn did not settle within {} ms and was cancelled",
                timeout.as_millis()
            ));
            break;
        } else {
            std::thread::sleep(IDLE_POLL_INTERVAL);
        }
    }
    // The demand loop runs where the frontends run it: after the turn's events.
    session.turn_settled();
    Ok(outcome)
}

/// Record one failure: always counted, sampled to [`MAX_RUN_FAILURES`].
fn record_failure(outcome: &mut RunOutcome, detail: &str) {
    outcome.failure_count += 1;
    if outcome.failures.len() < MAX_RUN_FAILURES {
        outcome.failures.push(bounded_failure(detail));
    }
}

/// Sanitize a failure summary and bound it to [`MAX_FAILURE_BYTES`].
fn bounded_failure(detail: &str) -> String {
    let safe = sanitize_for_display(detail);
    if safe.len() <= MAX_FAILURE_BYTES {
        return safe;
    }
    let budget = MAX_FAILURE_BYTES.saturating_sub(3);
    let mut bounded = String::new();
    for character in safe.chars() {
        if bounded.len() + character.len_utf8() > budget {
            break;
        }
        bounded.push(character);
    }
    bounded.push_str("...");
    bounded
}

/// Escape the minimum JSON requires.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control < ' ' => {
                out.push_str(&format!("\\u{:04x}", u32::from(control)));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn json_optional_string(value: Option<&str>) -> String {
    match value {
        Some(value) => json_string(value),
        None => "null".to_owned(),
    }
}

fn json_optional_u64(value: Option<u64>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "null".to_owned(),
    }
}

/// True when the report's records all share one task set (the comparison's
/// precondition, checked rather than assumed).
#[must_use]
pub fn records_share_one_task_set(report: &EvaluationReport) -> bool {
    let mut digests = report.runs.iter().map(|run| run.corpus_digest.as_str());
    match digests.next() {
        None => false,
        Some(first) => digests.all(|digest| digest == first),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use siralos_adapters::replay_store::write_replay_store;
    use siralos_core::determinism::{
        ProviderResponseIdentity, ReplayRecording,
    };
    use siralos_core::identity::sha256_hex;

    /// A recorded response body in the OpenAI shape the replay path parses.
    fn recorded_body(answer: &str) -> String {
        serde_json::json!({
            "choices": [{ "message": { "content": answer } }]
        })
        .to_string()
    }

    fn recording(model: &str, answer: &str) -> ReplayRecording {
        let body = recorded_body(answer);
        ReplayRecording {
            identity: ProviderResponseIdentity {
                provider_id: "deterministic-fake".to_owned(),
                model: model.to_owned(),
                status: Some(200),
                body_sha256: sha256_hex(body.as_bytes()),
                body_bytes: u64::try_from(body.len()).unwrap_or(u64::MAX),
                observed_at_ms: None,
                input_tokens: Some(64),
                output_tokens: Some(4),
                cached_tokens: None,
            },
            body,
        }
    }

    fn workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "siralos-evaluation-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".siralos"))
            .expect("temp workspace");
        root
    }

    fn profile(root: &std::path::Path, lines: &[&str]) {
        std::fs::write(root.join("siralos.toml"), lines.join("\n"))
            .expect("profile");
    }

    fn recorded_target(
        label: &str,
        model: &str,
        answers: [&str; 3],
    ) -> PathBuf {
        let root = workspace(label);
        profile(
            &root,
            &[
                "[profile]",
                "name = \"default\"",
                "provider = \"deterministic-fake\"",
                &format!("model = \"{model}\""),
                "replay = true",
            ],
        );
        let recordings: Vec<ReplayRecording> =
            answers.iter().map(|answer| recording(model, answer)).collect();
        write_replay_store(
            &root.join(".siralos").join("replay-store.json"),
            &recordings,
        )
        .expect("replay store");
        root
    }

    #[test]
    fn two_recorded_models_and_the_echo_fake_produce_records_and_one_table() {
        let corpus = evaluation_corpus();
        assert!(corpus.validate().is_ok());

        // The deterministic fake: an echo, not a model.
        let fake_root = workspace("fake");
        profile(
            &fake_root,
            &[
                "[profile]",
                "name = \"default\"",
                "provider = \"deterministic-fake\"",
                "model = \"echo\"",
            ],
        );
        // Two recorded models standing in for two providers, served by the
        // replay path out of a digest-bound store.
        let good_root = recorded_target(
            "good",
            "recorded-good",
            ["2", "Paris", "{\"ok\": true}"],
        );
        let weak_root = recorded_target(
            "weak",
            "recorded-weak",
            ["2", "Lyon", "{\"ok\": true}"],
        );

        let report = evaluate_targets(
            &corpus,
            &[
                EvaluationTarget::new(&fake_root),
                EvaluationTarget::new(&good_root),
                EvaluationTarget::new(&weak_root),
            ],
        )
        .expect("the evaluation runs");

        assert_eq!(report.runs.len(), 3);
        assert!(report.table.informational, "evidence, never a gate");
        assert!(report.table.comparable, "three runs are a comparison");
        assert!(
            records_share_one_task_set(&report),
            "one task set, three runs"
        );
        for run in &report.runs {
            assert_eq!(run.corpus_id, EVALUATION_CORPUS_ID);
            assert_eq!(run.corpus_digest.len(), 64);
            assert_eq!(run.cases_run, 3);
            assert_eq!(run.turns, 3);
            assert_eq!(run.tool_rounds, 0, "these prompts request no tool");
            assert!(!run.cancelled);
        }

        let by_model = |name: &str| {
            report
                .runs
                .iter()
                .find(|run| run.model.as_deref() == Some(name))
                .expect("a run per model")
        };
        // The fake echoes and the task set is semantic, so it misses every case.
        // The record says so rather than being tuned to look good.
        let echo = by_model("echo");
        assert_eq!(echo.cases_passed, 0);
        assert_eq!(echo.failure_count, 3);
        assert_eq!(echo.failures.len(), 3);
        assert!(echo.input_tokens.is_none(), "the fake reports no usage");
        // The recorded models answer the SAME task set differently.
        let good = by_model("recorded-good");
        assert_eq!((good.cases_passed, good.failure_count), (3, 0));
        let weak = by_model("recorded-weak");
        assert_eq!((weak.cases_passed, weak.failure_count), (2, 1));
        assert!(
            weak.failures.iter().any(|failure| failure.contains("capital")),
            "a mismatch names its case: {:?}",
            weak.failures
        );
        assert!(
            weak.input_tokens.is_none(),
            "playback serves the body and does not re-report recorded usage"
        );

        let models: Vec<Option<&str>> =
            report.table.rows.iter().map(|row| row.model.as_deref()).collect();
        assert_eq!(
            models,
            vec![Some("echo"), Some("recorded-good"), Some("recorded-weak")]
        );
        assert!(report.rendered.contains("INFORMATIONAL"));
        assert!(report.rendered.contains(EVALUATION_CORPUS_ID));
        assert!(
            report.rendered.contains("-/-/-"),
            "no provider in this proof reports usage: {}",
            report.rendered
        );
        assert_eq!(
            report.rendered,
            sanitize_for_display(&render_comparison(&report.table)),
            "the rendered table is the sanitized core render, byte for byte"
        );

        // The JSON artifact must parse, and must carry no host-shaped detail.
        let json = render_records_json(&report);
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["informational"], serde_json::json!(true));
        assert_eq!(value["runs"].as_array().map(Vec::len), Some(3));
        assert_eq!(value["runs"][1]["casesPassed"], serde_json::json!(3));
        assert_eq!(
            value["corpusDigest"],
            serde_json::json!(report.table.corpus_digest)
        );
        for needle in [
            fake_root.to_string_lossy().to_string(),
            good_root.to_string_lossy().to_string(),
            "credential".to_owned(),
            "endpoint".to_owned(),
            "env:".to_owned(),
        ] {
            assert!(!json.contains(&needle), "the record carries {needle}");
        }
    }

    #[test]
    fn an_unconfigured_workspace_is_labelled_rather_than_accused() {
        let root = workspace("bare");
        let run = run_evaluation(
            &evaluation_corpus(),
            &EvaluationTarget::new(&root),
        )
        .expect("the run happens");
        assert_eq!(run.provider, "(unconfigured)");
        assert_eq!(run.model, None);
        assert_eq!(run.cases_run, 3);
        assert_eq!(
            run.cases_passed, 0,
            "the default fake echoes; the cases are semantic"
        );
    }

    #[test]
    fn failures_are_counted_in_full_and_sampled_to_the_bound() {
        let mut outcome = RunOutcome {
            provider: "p".to_owned(),
            model: None,
            corpus_id: "c".to_owned(),
            corpus_digest: "d".to_owned(),
            cases_run: 0,
            cases_passed: 0,
            turns: 0,
            tool_rounds: 0,
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            failure_count: 0,
            failures: Vec::new(),
            cancelled: false,
            wall_ms: 0,
        };
        for index in 0..(MAX_RUN_FAILURES + 4) {
            record_failure(&mut outcome, &format!("case {index} missed"));
        }
        assert_eq!(outcome.failure_count, MAX_RUN_FAILURES + 4);
        assert_eq!(outcome.failures.len(), MAX_RUN_FAILURES);
        let long = bounded_failure(&"a".repeat(MAX_FAILURE_BYTES * 2));
        assert_eq!(long.len(), MAX_FAILURE_BYTES);
        assert!(long.ends_with("..."));
        assert!(!bounded_failure("\u{1b}[31mred").contains('\u{1b}'));
    }

    /// A session that never settles: silent (the wall-time bound) or flooding
    /// (the event bound). No provider in the offline proof can produce either
    /// state, so this double is the only way to prove the runner cannot hang or
    /// spin.
    struct StubbornSession {
        responding: bool,
        flood: bool,
        cancelled: bool,
    }

    impl WorkerSession for StubbornSession {
        fn send_prompt(&mut self, _prompt: &str) -> Result<(), String> {
            self.responding = true;
            Ok(())
        }
        fn poll_event(&mut self) -> Option<ToolLoopEvent> {
            if self.flood {
                Some(ToolLoopEvent::TextDelta { text: "x".to_owned() })
            } else {
                None
            }
        }
        fn is_responding(&self) -> bool {
            self.responding
        }
        fn pane(&self) -> Option<crate::tui::ContextPaneData> {
            None
        }
        fn context_report(&self) -> String {
            String::new()
        }
        fn tools_report(&self) -> String {
            String::new()
        }
        fn set_model(&mut self, _model: &str) -> Result<(), String> {
            Ok(())
        }
        fn reload(&mut self) -> Result<String, String> {
            Ok(String::new())
        }
        fn turn_settled(&mut self) {}
        fn fetch_models(&mut self) -> Result<Vec<String>, String> {
            Ok(Vec::new())
        }
        fn domains_add(&mut self, _folder: &str) -> Result<String, String> {
            Ok(String::new())
        }
        fn domains_enable(&mut self, _id: &str) -> Result<String, String> {
            Ok(String::new())
        }
        fn domains_activate(&mut self, _id: &str) -> Result<String, String> {
            Ok(String::new())
        }
        fn status(&self) -> crate::session_worker::SessionStatus {
            crate::session_worker::SessionStatus {
                status: String::new(),
                provider: None,
                model: None,
                endpoint: None,
                protocol: String::new(),
                credential_display: None,
                credential_resolved: false,
                context_suffix: String::new(),
            }
        }
        fn cancel(&mut self) {
            self.cancelled = true;
            self.responding = false;
        }
        fn flush(&mut self) {}
        fn enable_progress_ticks(&mut self) {}
    }

    fn stubborn(flood: bool) -> StubbornSession {
        StubbornSession { responding: false, flood, cancelled: false }
    }

    #[test]
    fn a_turn_that_never_settles_is_cancelled_at_its_bound() {
        // The wall-time bound: a silent session cannot hold the run open.
        let mut stalled = stubborn(false);
        let started = Instant::now();
        let turn =
            drive_turn(&mut stalled, "prompt", Duration::from_millis(20))
                .expect("the turn is driven");
        assert!(turn.cancelled, "the stall is a cancellation");
        assert!(stalled.cancelled, "the runner asked the session to cancel");
        let failure = turn.failure.expect("a failure is recorded");
        assert!(failure.contains("did not settle"), "{failure}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the bound is the deadline, not a spin"
        );

        // The event bound: a flooding session cannot grow the record without
        // limit. The event that trips the bound is counted and dropped.
        let mut flooding = stubborn(true);
        let turn = drive_turn(&mut flooding, "prompt", DEFAULT_TURN_TIMEOUT)
            .expect("the turn is driven");
        assert!(flooding.cancelled);
        assert_eq!(turn.answer.len(), MAX_TURN_EVENTS);
        let failure = turn.failure.expect("a failure is recorded");
        assert!(failure.contains("exceeded 4096 events"), "{failure}");
    }

    #[test]
    fn an_empty_target_list_is_a_typed_refusal() {
        let error = evaluate_targets(&evaluation_corpus(), &[])
            .expect_err("nothing to compare");
        assert!(matches!(error, EvaluationRunError::Comparison(_)), "{error}");
        assert!(error.to_string().contains("at least one"));
    }
}
