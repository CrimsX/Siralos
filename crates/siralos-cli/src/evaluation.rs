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
use crate::session_worker::{FlushOutcome, WorkerSession};

/// Stable id of the evaluation task set.
pub const EVALUATION_CORPUS_ID: &str = "siralos-evaluation-smoke";
/// Wall-time budget for accepting events from one prompt turn when a target
/// does not set its own; a separate bounded quiesce grace may follow.
pub const DEFAULT_TURN_TIMEOUT: Duration = Duration::from_secs(120);
/// Ceiling for one prompt turn's wall-time budget, whatever a target sets. An
/// evaluation runs unattended, so an unbounded or absurd target timeout is a
/// resource-exhaustion lever, not a preference.
pub const MAX_TURN_TIMEOUT: Duration = Duration::from_secs(600);
/// Ceiling on how many targets one evaluation compares.
pub const MAX_EVALUATION_TARGETS: usize = 64;
/// Event budget for ONE prompt turn.
pub const MAX_TURN_EVENTS: usize = 4096;
/// Aggregate UTF-8 byte budget for the answer collected from one turn.
pub const MAX_ANSWER_BYTES: usize = 64 * 1024;
/// Marker appended when the aggregate answer budget is exceeded. It is part
/// of the byte budget, so the resulting string is always within the cap.
const ANSWER_TRUNCATION_MARKER: &str = "...[truncated]";
/// How long an idle poll waits before checking the turn deadline again.
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(1);
/// Bounded cooperative drain budget after cancellation or a terminal event.
const QUIESCE_TIMEOUT: Duration = Duration::from_millis(250);
/// Upper bound on discarded events while quiescing a cancelled response.
const MAX_QUIESCE_EVENTS: usize = 256;

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
#[derive(Clone)]
pub struct EvaluationTarget {
    /// Workspace root the session is composed for.
    pub workspace_root: PathBuf,
    /// Optional trusted user configuration path used for composition.
    pub config_path: Option<PathBuf>,
    /// Wall-time budget for accepting events from one prompt turn. A separate,
    /// bounded quiesce grace may follow after cancellation.
    pub turn_timeout: Duration,
}

/// Absolute target paths are host-only; diagnostics and reports use the
/// workspace-relative projection instead.
impl fmt::Debug for EvaluationTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EvaluationTarget")
            .field("workspace_root", &"[ABSOLUTE]")
            .field(
                "config_path",
                &self.config_path.as_ref().map(|_| "[ABSOLUTE]"),
            )
            .field("turn_timeout", &self.turn_timeout)
            .finish()
    }
}

impl EvaluationTarget {
    /// A target with the default turn budget.
    #[must_use]
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            config_path: None,
            turn_timeout: DEFAULT_TURN_TIMEOUT,
        }
    }

    /// Construct a target with an explicit trusted user-config path. The
    /// workspace is never searched for this file: a workspace-controlled
    /// approval would defeat the profile trust boundary.
    #[must_use]
    pub fn with_config(
        workspace_root: impl Into<PathBuf>,
        config_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            config_path: Some(config_path.into()),
            turn_timeout: DEFAULT_TURN_TIMEOUT,
        }
    }

    /// The effective turn budget, clamped to [`MAX_TURN_TIMEOUT`].
    #[must_use]
    pub fn effective_turn_timeout(&self) -> Duration {
        self.turn_timeout.min(MAX_TURN_TIMEOUT)
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
    /// A cancelled turn did not reach a quiescent terminal state.
    Quiesce(String),
    /// The single replay-store flush did not complete successfully.
    Flush(String),
    /// A turn failure and a replay-flush failure both occurred; both causes
    /// are retained for the finalizer.
    Finalization {
        /// The typed primary turn failure.
        turn: Box<EvaluationRunError>,
        /// The typed persistence failure.
        flush: Box<EvaluationRunError>,
    },
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
            Self::Quiesce(message) => {
                write!(formatter, "evaluation turn quiesce: {message}")
            }
            Self::Flush(message) => {
                write!(formatter, "evaluation replay flush: {message}")
            }
            Self::Finalization { turn, flush } => {
                write!(
                    formatter,
                    "evaluation turn and replay flush failed: {turn}; {flush}"
                )
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
/// cannot be composed, a prompt cannot be sent, a cancelled turn cannot
/// quiesce, or the replay store cannot be flushed. If both the turn and the
/// finalizer fail, the returned [`EvaluationRunError::Finalization`] retains
/// both typed causes. The flush is attempted even after a prompt or quiesce
/// failure, so persistence evidence is never lost on an early return.
pub fn run_evaluation(
    corpus: &EvaluationCorpus,
    target: &EvaluationTarget,
) -> Result<RunOutcome, EvaluationRunError> {
    let corpus_digest = task_set_digest(corpus)
        .map_err(|error| EvaluationRunError::Corpus(error.message))?;
    let started = Instant::now();
    let mut session = compose_session(InteractiveOptions {
        config_path: target.config_path.as_deref(),
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
    // Keep the task loop separate from the finalizer. In particular, an early
    // prompt failure must not skip the one replay flush that owns the
    // persistence evidence.
    let run_result = (|| -> Result<(), EvaluationRunError> {
        for case in &cases {
            let turn = drive_turn(
                &mut session,
                &case.prompt,
                target.effective_turn_timeout(),
            )?;
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
                // The answer is NOT recorded: the completion is untrusted data
                // and can be arbitrarily long. The mismatch is the evidence.
                record_failure(
                    &mut outcome,
                    &format!("case {} did not match the expectation", case.id),
                );
            }
        }
        Ok(())
    })();
    // Usage is whatever the provider reported through the recording seam, and
    // nothing when it reported none (absent stays absent, never a zero).
    if let Some(usage) = session.usage_totals() {
        outcome.input_tokens = usage.input_tokens;
        outcome.output_tokens = usage.output_tokens;
        outcome.cached_tokens = usage.cached_tokens;
    }
    // The single-owner flush (decision 78): a record-replay profile persists
    // its store here, and a session without a recorder reports that explicitly.
    // The typed seam is mandatory here: a log line is not a successful
    // evaluation when the evidence could not be persisted.
    let flush_result = session.flush_result();
    outcome.wall_ms =
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match (run_result, flush_result) {
        (
            Ok(()),
            Ok(FlushOutcome::NoRecorder | FlushOutcome::Persisted { .. }),
        ) => Ok(outcome),
        (Ok(()), Ok(FlushOutcome::Legacy)) => {
            Err(EvaluationRunError::Flush(bounded_failure(
                "replay flush outcome is unavailable; typed persistence evidence is missing",
            )))
        }
        (Ok(()), Err(error)) => {
            Err(EvaluationRunError::Flush(bounded_failure(&error.to_string())))
        }
        (Err(error), Ok(FlushOutcome::Legacy)) => {
            let flush = EvaluationRunError::Flush(bounded_failure(
                "replay flush outcome is unavailable; typed persistence evidence is missing",
            ));
            Err(EvaluationRunError::Finalization {
                turn: Box::new(error),
                flush: Box::new(flush),
            })
        }
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(flush_error)) => {
            let flush = EvaluationRunError::Flush(bounded_failure(&format!(
                "replay flush failed: {flush_error}"
            )));
            Err(EvaluationRunError::Finalization {
                turn: Box::new(error),
                flush: Box::new(flush),
            })
        }
    }
}

/// Run the task set against every target and compare the records.
///
/// # Errors
///
/// Returns [`EvaluationRunError`] when any run fails or the comparison cannot
/// be built (an empty or over-long target list is a comparison error).
pub fn evaluate_targets(
    corpus: &EvaluationCorpus,
    targets: &[EvaluationTarget],
) -> Result<EvaluationReport, EvaluationRunError> {
    if targets.len() > MAX_EVALUATION_TARGETS {
        return Err(EvaluationRunError::Comparison(format!(
            "at most {MAX_EVALUATION_TARGETS} targets are compared"
        )));
    }
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
///
/// Shared with the headless frontend, which prints the same completion the
/// interactive frontends display. The completion remains untrusted display
/// data: it is never written into a record, a digest, or host evidence.
#[derive(Debug, Default)]
pub(crate) struct TurnOutcome {
    /// Completion text, kept for scoring only and never stored.
    pub(crate) answer: String,
    /// Tool rounds observed in the turn.
    pub(crate) tool_rounds: usize,
    /// The turn was cancelled.
    pub(crate) cancelled: bool,
    /// The answer was explicitly shortened at [`MAX_ANSWER_BYTES`].
    pub(crate) answer_truncated: bool,
    /// A failure the turn reported, or one the runner detected.
    pub(crate) failure: Option<String>,
}

/// Truncate to a byte limit without splitting a UTF-8 scalar.
fn truncate_utf8_prefix(value: &mut String, limit: usize) {
    let mut end = limit.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

/// Append a provider text delta under the aggregate answer bound.
///
/// Returns `true` when the delta crossed the bound. The marker is included in
/// the bound and the function never accepts more bytes than
/// [`MAX_ANSWER_BYTES`], including when the incoming delta is a single huge
/// string.
fn append_bounded_answer(answer: &mut String, text: &str) -> bool {
    if text.len() <= MAX_ANSWER_BYTES.saturating_sub(answer.len()) {
        answer.push_str(text);
        return false;
    }

    let prefix_budget =
        MAX_ANSWER_BYTES.saturating_sub(ANSWER_TRUNCATION_MARKER.len());
    if answer.len() > prefix_budget {
        truncate_utf8_prefix(answer, prefix_budget);
    }
    let remaining = prefix_budget.saturating_sub(answer.len());
    let mut end = 0;
    for (offset, character) in text.char_indices() {
        let next = offset.saturating_add(character.len_utf8());
        if next > remaining {
            break;
        }
        end = next;
    }
    answer.push_str(&text[..end]);
    answer.push_str(ANSWER_TRUNCATION_MARKER);
    true
}

fn mark_deadline<S: WorkerSession>(
    session: &mut S,
    outcome: &mut TurnOutcome,
    timeout: Duration,
) {
    session.cancel();
    outcome.cancelled = true;
    if outcome.failure.is_none() {
        outcome.failure = Some(format!(
            "turn did not settle within {} ms and was cancelled",
            timeout.as_millis()
        ));
    }
}

fn drain_until_settled<S: WorkerSession>(
    session: &mut S,
    timeout: Duration,
) -> bool {
    let Some(deadline) = Instant::now().checked_add(timeout) else {
        return false;
    };
    let mut drained = 0usize;
    while session.is_responding() {
        if drained >= MAX_QUIESCE_EVENTS || Instant::now() >= deadline {
            return false;
        }
        match session.poll_event() {
            Some(_) => drained += 1,
            None => {
                if !session.is_responding() {
                    return true;
                }
                std::thread::sleep(IDLE_POLL_INTERVAL);
            }
        }
    }
    true
}

/// Send one prompt and drain the turn to its end.
///
/// The response is bounded by event count, aggregate answer bytes, and wall
/// time. The wall deadline is checked before polling and again before accepting
/// the polled event, so an event that arrives just after the deadline cannot be
/// mistaken for a live answer. Once a bound is exceeded, the bounded
/// [`QUIESCE_TIMEOUT`] cleanup grace drains cancellation evidence but never
/// accepts new answer data. A blocking provider call itself remains cooperative
/// because the synchronous `poll_event` seam has no cancellation channel.
pub(crate) fn drive_turn<S: WorkerSession>(
    session: &mut S,
    prompt: &str,
    timeout: Duration,
) -> Result<TurnOutcome, EvaluationRunError> {
    let mut outcome = TurnOutcome::default();
    // Start the clock before the fallible/blocking start operation. A transport
    // that returns after the budget is still handled as a deadline violation;
    // making the provider read itself abortable is a separate transport seam.
    // `checked_add` keeps an absurd caller-supplied duration from panicking.
    // A clock that cannot represent the requested deadline fails closed as
    // already expired; normal finite budgets get the exact wall bound.
    let deadline = Instant::now().checked_add(timeout);
    let deadline_reached = || match deadline {
        Some(limit) => Instant::now() >= limit,
        None => true,
    };
    if let Err(error) = session.send_prompt(prompt) {
        // A provider may have started partially before reporting the refusal;
        // request cancellation and restore the state before the caller's final
        // replay flush.
        if session.is_responding() {
            session.cancel();
            if !drain_until_settled(session, QUIESCE_TIMEOUT) {
                session.cancel();
                return Err(EvaluationRunError::Quiesce(bounded_failure(
                    &format!(
                        "{error}; cancelled turn did not quiesce within {} ms",
                        QUIESCE_TIMEOUT.as_millis()
                    ),
                )));
            }
            session.turn_settled();
        }
        return Err(EvaluationRunError::Prompt(error));
    }
    let mut events = 0usize;
    let mut quiesce_required = false;
    let mut terminal_seen = false;
    if deadline_reached() {
        mark_deadline(session, &mut outcome, timeout);
        quiesce_required = true;
    }
    while session.is_responding() {
        // Check before the blocking poll as well as after it. In particular, a
        // zero timeout must not accept a delta that was already queued.
        if deadline_reached() {
            mark_deadline(session, &mut outcome, timeout);
            quiesce_required = true;
            break;
        }
        let event = session.poll_event();
        // Do not process any event, or treat an empty poll as completion, after
        // the wall deadline. The check is deliberately after `poll_event` as
        // well as before it: a provider can return a queued event just late.
        if deadline_reached() {
            mark_deadline(session, &mut outcome, timeout);
            quiesce_required = true;
            break;
        }
        if let Some(event) = event {
            events += 1;
            if events > MAX_TURN_EVENTS {
                session.cancel();
                outcome.cancelled = true;
                if outcome.failure.is_none() {
                    outcome.failure = Some(format!(
                        "turn exceeded {MAX_TURN_EVENTS} events and was cancelled"
                    ));
                }
                quiesce_required = true;
                break;
            }
            match event {
                ToolLoopEvent::TextDelta { text } => {
                    if append_bounded_answer(&mut outcome.answer, &text) {
                        outcome.answer_truncated = true;
                        outcome.cancelled = true;
                        let truncation_failure = format!(
                            "turn answer exceeded the {MAX_ANSWER_BYTES}-byte aggregate cap; answer was truncated and the turn was cancelled"
                        );
                        outcome.failure = Some(match outcome.failure.take() {
                            Some(previous) => {
                                format!("{previous}; {truncation_failure}")
                            }
                            None => truncation_failure,
                        });
                        session.cancel();
                        quiesce_required = true;
                        break;
                    }
                }
                ToolLoopEvent::ToolStarted { .. } => {
                    // R7.2 pairs one call with one result per round, so one
                    // started call is one round.
                    outcome.tool_rounds += 1;
                }
                ToolLoopEvent::ResponseFailed { message } => {
                    outcome.failure = Some(message);
                    terminal_seen = true;
                    break;
                }
                ToolLoopEvent::ToolFailed { message, .. } => {
                    outcome.failure = Some(message);
                }
                ToolLoopEvent::ResponseCancelled => {
                    outcome.cancelled = true;
                    terminal_seen = true;
                    break;
                }
                ToolLoopEvent::ResponseCompleted => {
                    terminal_seen = true;
                    break;
                }
                ToolLoopEvent::ReasoningDelta { .. }
                | ToolLoopEvent::ResponseStarted
                | ToolLoopEvent::ToolCompleted { .. }
                | ToolLoopEvent::ToolCancelled { .. }
                | ToolLoopEvent::ProviderPending
                | ToolLoopEvent::ContextPressure { .. } => {}
            }
        } else {
            std::thread::sleep(IDLE_POLL_INTERVAL);
        }
    }
    if (quiesce_required || terminal_seen)
        && !drain_until_settled(session, QUIESCE_TIMEOUT)
    {
        // Make one final cancellation request before reporting the
        // cleanup failure; some providers expose cancellation only at
        // their next poll boundary.
        session.cancel();
        let detail = match outcome.failure.take() {
            Some(previous) => format!(
                "{previous}; cancelled turn did not quiesce within {} ms",
                QUIESCE_TIMEOUT.as_millis()
            ),
            None => format!(
                "cancelled turn did not quiesce within {} ms",
                QUIESCE_TIMEOUT.as_millis()
            ),
        };
        return Err(EvaluationRunError::Quiesce(bounded_failure(&detail)));
    }
    // The demand loop runs where the frontends run it: after the turn's events
    // and after the terminal sentinel has restored the application state.
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
pub(crate) fn bounded_failure(detail: &str) -> String {
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
pub(crate) fn json_string(value: &str) -> String {
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

pub(crate) fn json_optional_string(value: Option<&str>) -> String {
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
            request_sha256: None,
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
        let profile = lines.join("\n");
        std::fs::write(root.join("siralos.toml"), &profile).expect("profile");
        let digest = siralos_core::identity::sha256_hex(profile.as_bytes());
        std::fs::write(
            root.join(".test-user-config.json"),
            format!("{{\"profileApproval\":\"{digest}\"}}"),
        )
        .expect("test approval config");
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
                EvaluationTarget::with_config(
                    &fake_root,
                    fake_root.join(".test-user-config.json"),
                ),
                EvaluationTarget::with_config(
                    &good_root,
                    good_root.join(".test-user-config.json"),
                ),
                EvaluationTarget::with_config(
                    &weak_root,
                    weak_root.join(".test-user-config.json"),
                ),
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
        stays_responding_after_cancel: bool,
        settled: bool,
        terminal_sentinel_delay: Option<Duration>,
        saw_terminal: bool,
        events: std::collections::VecDeque<ToolLoopEvent>,
        poll_delay: Duration,
    }

    impl WorkerSession for StubbornSession {
        fn send_prompt(&mut self, _prompt: &str) -> Result<(), String> {
            self.responding = true;
            Ok(())
        }
        fn poll_event(&mut self) -> Option<ToolLoopEvent> {
            if self.saw_terminal {
                if let Some(delay) = self.terminal_sentinel_delay.take() {
                    std::thread::sleep(delay);
                }
            }
            if !self.poll_delay.is_zero() {
                std::thread::sleep(self.poll_delay);
            }
            let next = self.events.pop_front();
            if matches!(
                &next,
                Some(
                    ToolLoopEvent::ResponseCompleted
                        | ToolLoopEvent::ResponseCancelled
                        | ToolLoopEvent::ResponseFailed { .. }
                )
            ) {
                self.saw_terminal = true;
            }
            if next.is_none()
                && self.saw_terminal
                && !self.stays_responding_after_cancel
            {
                self.responding = false;
            }
            if let Some(event) = next {
                return Some(event);
            }
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
        fn turn_settled(&mut self) {
            self.settled = true;
        }
        fn fetch_models(
            &mut self,
            _cancellation: &crate::session_worker::CancelFlag,
        ) -> Result<Vec<String>, String> {
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
                live_model_switchable: true,
                context_suffix: String::new(),
            }
        }
        fn cancel(&mut self) {
            self.cancelled = true;
            if !self.stays_responding_after_cancel {
                self.responding = false;
            }
        }
        fn flush(&mut self) {}
        fn enable_progress_ticks(&mut self) {}
    }

    fn stubborn(flood: bool) -> StubbornSession {
        StubbornSession {
            responding: false,
            flood,
            cancelled: false,
            stays_responding_after_cancel: false,
            settled: false,
            terminal_sentinel_delay: None,
            saw_terminal: false,
            events: std::collections::VecDeque::new(),
            poll_delay: Duration::ZERO,
        }
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
    fn a_terminal_event_survives_a_late_sentinel_without_a_false_timeout() {
        let mut terminal = stubborn(false);
        terminal.events.push_back(ToolLoopEvent::ResponseCompleted);
        terminal.terminal_sentinel_delay = Some(Duration::from_millis(20));
        let turn =
            drive_turn(&mut terminal, "prompt", Duration::from_millis(5))
                .expect("the accepted terminal is settled");
        assert!(turn.failure.is_none(), "{:?}", turn.failure);
        assert!(!turn.cancelled);
        assert!(terminal.settled);
    }

    #[test]
    fn a_late_text_delta_is_rejected_before_it_can_become_the_answer() {
        let mut late = stubborn(false);
        late.poll_delay = Duration::from_millis(15);
        late.events
            .push_back(ToolLoopEvent::TextDelta { text: "late".to_owned() });
        let turn = drive_turn(&mut late, "prompt", Duration::from_millis(1))
            .expect("the turn is driven");
        assert!(late.cancelled, "the late event is a timeout");
        assert!(turn.cancelled);
        assert!(turn.answer.is_empty(), "late bytes are not accepted");
        let failure = turn.failure.expect("the late event is reported");
        assert!(failure.contains("did not settle"), "{failure}");
    }

    #[test]
    fn a_stuck_cancelled_turn_reports_quiesce_failure_without_claiming_settlement()
     {
        let mut stuck = stubborn(false);
        stuck.stays_responding_after_cancel = true;
        let error = drive_turn(&mut stuck, "prompt", Duration::from_millis(1))
            .expect_err("the cleanup cannot claim quiescence");
        assert!(matches!(error, EvaluationRunError::Quiesce(_)), "{error}");
        assert!(stuck.cancelled, "the response was cancelled");
        assert!(!stuck.settled, "settlement is withheld after failed quiesce");
    }

    #[test]
    fn answer_deltas_are_capped_in_aggregate_and_report_truncation() {
        let mut overlong = stubborn(false);
        overlong.events.push_back(ToolLoopEvent::TextDelta {
            text: "é".repeat(MAX_ANSWER_BYTES / 2),
        });
        overlong.events.push_back(ToolLoopEvent::TextDelta {
            text: "é".repeat(MAX_ANSWER_BYTES / 2 + 1),
        });
        let turn = drive_turn(&mut overlong, "prompt", Duration::from_secs(1))
            .expect("the turn is driven");
        assert!(overlong.cancelled, "an overlong answer is cancelled");
        assert!(turn.answer_truncated);
        assert!(turn.answer.len() <= MAX_ANSWER_BYTES);
        assert!(turn.answer.ends_with(ANSWER_TRUNCATION_MARKER));
        assert!(turn.answer.is_char_boundary(turn.answer.len()));
        let failure = turn.failure.expect("truncation is an explicit error");
        assert!(failure.contains("aggregate cap"), "{failure}");
        assert!(failure.contains("truncated"), "{failure}");
    }

    #[test]
    fn a_target_budget_and_target_count_are_bounded() {
        // An ABSOLUTE workspace path, because that is what the Debug
        // projection is for: a relative name is not a path leak, and hiding
        // it too would make the projection lie about what it redacts.
        let absolute = std::env::temp_dir().join("siralos-eval-target");
        let absolute = absolute.to_string_lossy().into_owned();
        let mut target = EvaluationTarget::new(absolute.as_str());
        target.turn_timeout = Duration::from_secs(60 * 60 * 24);
        assert_eq!(target.effective_turn_timeout(), MAX_TURN_TIMEOUT);
        target.turn_timeout = DEFAULT_TURN_TIMEOUT;
        assert_eq!(target.effective_turn_timeout(), DEFAULT_TURN_TIMEOUT);
        let debug = format!("{target:?}");
        assert!(!debug.contains(&absolute), "{debug}");
        assert!(debug.contains("[ABSOLUTE]"), "{debug}");

        let targets: Vec<EvaluationTarget> = (0..=MAX_EVALUATION_TARGETS)
            .map(|_| EvaluationTarget::new(absolute.as_str()))
            .collect();
        let error = evaluate_targets(&evaluation_corpus(), &targets)
            .expect_err("an over-long target list is refused");
        assert!(error.to_string().contains("at most"), "{error}");
    }

    #[test]
    fn an_empty_target_list_is_a_typed_refusal() {
        let error = evaluate_targets(&evaluation_corpus(), &[])
            .expect_err("nothing to compare");
        assert!(matches!(error, EvaluationRunError::Comparison(_)), "{error}");
        assert!(error.to_string().contains("at least one"));
    }
}
