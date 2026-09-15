//! Multi-model evaluation runs (decision 178, ticket 135).
//!
//! One evaluation run sends a bounded, digest-bound task set through a session
//! composed for ONE provider/model and summarises what happened; several runs
//! are then set side by side. This module owns the pure shapes only -- the
//! runner that composes sessions lives in `siralos-cli::evaluation`, because it
//! needs the composition root, and nothing here touches I/O, a provider or a
//! clock (wall time is passed in).
//!
//! **Informational by construction.** The comparison never gates, scores, ranks
//! or blocks a session, and it never changes a threshold -- the same rule
//! decision 94 set for the estimator-calibration comparison (decision 102 P3).
//! [`ComparisonTable::informational`] is `true` and there is no API that can make
//! it anything else.

use std::collections::BTreeMap;

use crate::evolution::{CorpusValidationError, EvaluationCorpus};
use crate::identity::{CanonicalValue, compute_artifact_digest};

/// Maximum number of runs one comparison accepts.
pub const MAX_COMPARISON_RUNS: usize = 16;
/// Maximum recorded failure summaries per run (the count is recorded in full).
pub const MAX_RUN_FAILURES: usize = 8;
/// Maximum length of one recorded failure summary in bytes.
pub const MAX_FAILURE_BYTES: usize = 200;

/// Domain-separated digest over the task set alone: the corpus id plus the
/// ordered `(id, prompt, expected)` triples.
///
/// The answers are deliberately NOT bound. Two providers that answer the same
/// corpus differently must share this digest, because sharing it is what makes
/// the comparison a comparison of models rather than of corpora.
/// [`crate::evolution::create_corpus_evidence`] binds the match outcomes as
/// well, so it is the wrong identity for this job -- this digest is the one
/// the records are bound to.
///
/// # Errors
///
/// Returns [`CorpusValidationError`] for a malformed corpus or a digest
/// failure.
pub fn task_set_digest(
    corpus: &EvaluationCorpus,
) -> Result<String, CorpusValidationError> {
    corpus.validate()?;
    let mut sorted = corpus.cases.clone();
    sorted.sort_by(|left, right| left.id.cmp(&right.id));
    let cases: Vec<CanonicalValue> = sorted
        .iter()
        .map(|case| {
            CanonicalValue::Object(BTreeMap::from([
                (
                    "expected".to_owned(),
                    CanonicalValue::Str(case.expected.clone()),
                ),
                ("id".to_owned(), CanonicalValue::Str(case.id.clone())),
                (
                    "prompt".to_owned(),
                    CanonicalValue::Str(case.prompt.clone()),
                ),
            ]))
        })
        .collect();
    let payload = CanonicalValue::Object(BTreeMap::from([
        ("cases".to_owned(), CanonicalValue::Array(cases)),
        ("corpusId".to_owned(), CanonicalValue::Str(corpus.id.clone())),
    ]));
    compute_artifact_digest("EvaluationTaskSet", 1, &payload)
        .map(|digest| digest.value)
        .map_err(|error| CorpusValidationError { message: error.message })
}

/// Provider-reported usage totalled over one recorded session.
///
/// Every field stays `None` unless at least one recording actually reported
/// it: absent usage is never fabricated into a zero (the rule decision 102
/// set when usage capture was added).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UsageTotals {
    /// Sum of reported input/prompt tokens, where reported.
    pub input_tokens: Option<u64>,
    /// Sum of reported output/completion tokens, where reported.
    pub output_tokens: Option<u64>,
    /// Sum of reported cached tokens, where reported.
    pub cached_tokens: Option<u64>,
}

/// One provider/model evaluation run, summarised.
///
/// Identity is deliberately shallow: the provider id and the model label are
/// carried, an endpoint, credential or workspace path never is (the decision 70
/// §4 hygiene contract, applied to the record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// Provider the run was composed with (an id, never an endpoint).
    pub provider: String,
    /// Model label the run used, when one was applied.
    pub model: Option<String>,
    /// Evaluation corpus id.
    pub corpus_id: String,
    /// Corpus digest the run was bound to.
    pub corpus_digest: String,
    /// Cases executed.
    pub cases_run: usize,
    /// Cases whose outcome matched the corpus expectation.
    pub cases_passed: usize,
    /// Prompt turns issued (one per case).
    pub turns: usize,
    /// Tool rounds observed.
    pub tool_rounds: usize,
    /// Provider-reported input tokens, where reported.
    pub input_tokens: Option<u64>,
    /// Provider-reported output tokens, where reported.
    pub output_tokens: Option<u64>,
    /// Provider-reported cached tokens, where reported.
    pub cached_tokens: Option<u64>,
    /// Every failure observed, including the ones not sampled below.
    pub failure_count: usize,
    /// Bounded, sanitized failure summaries (never a transcript, never the
    /// whole count: [`Self::failure_count`] is the count).
    pub failures: Vec<String>,
    /// Whether the run was cancelled.
    pub cancelled: bool,
    /// Wall time in milliseconds. Recorded, but never compared: it is the one
    /// field a deterministic run cannot promise.
    pub wall_ms: u64,
}

impl RunOutcome {
    /// Total provider-reported tokens, when the provider reported any.
    #[must_use]
    pub fn total_tokens(&self) -> Option<u64> {
        match (self.input_tokens, self.output_tokens) {
            (None, None) => None,
            (input, output) => {
                Some(input.unwrap_or(0).saturating_add(output.unwrap_or(0)))
            }
        }
    }
}

/// One row of a comparison: the deterministic facts of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonRow {
    /// Provider id.
    pub provider: String,
    /// Model label, when one was applied.
    pub model: Option<String>,
    /// Cases executed.
    pub cases_run: usize,
    /// Cases matched.
    pub cases_passed: usize,
    /// Prompt turns issued.
    pub turns: usize,
    /// Tool rounds observed.
    pub tool_rounds: usize,
    /// Provider-reported input tokens, where reported.
    pub input_tokens: Option<u64>,
    /// Provider-reported output tokens, where reported.
    pub output_tokens: Option<u64>,
    /// Provider-reported cached tokens, where reported.
    pub cached_tokens: Option<u64>,
    /// Every failure observed in the run.
    pub failure_count: usize,
    /// How many failure summaries the record sampled (bounded).
    pub failures: usize,
    /// Whether the run was cancelled.
    pub cancelled: bool,
}

/// The informational comparison over one or more runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonTable {
    /// Pinned `true`: this surface is evidence, never a gate (decision 94).
    pub informational: bool,
    /// Corpus id every row was bound to.
    pub corpus_id: String,
    /// Corpus digest every row was bound to.
    pub corpus_digest: String,
    /// Rows, ordered by (provider, model) so the table is deterministic.
    pub rows: Vec<ComparisonRow>,
    /// Whether more than one run is being compared. A single run is a baseline,
    /// not a comparison, and the table says so instead of pretending.
    pub comparable: bool,
}

/// A comparison that cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonError {
    /// Deterministic, human-readable reason.
    pub message: String,
}

impl std::fmt::Display for ComparisonError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ComparisonError {}

/// Compare evaluation runs.
///
/// Rows are ordered by (provider, model) so the same runs always render the same
/// bytes. Token counts are never summed across rows: they are model-specific
/// (the same rule the calibration report uses).
///
/// # Errors
///
/// Returns [`ComparisonError`] when there is nothing to compare or when more
/// than [`MAX_COMPARISON_RUNS`] runs are handed in -- a bound, not a truncation,
/// because a silently shortened table would be misleading evidence.
pub fn compare_runs(
    runs: &[RunOutcome],
) -> Result<ComparisonTable, ComparisonError> {
    if runs.is_empty() {
        return Err(ComparisonError {
            message: "A comparison needs at least one evaluation run."
                .to_owned(),
        });
    }
    if runs.len() > MAX_COMPARISON_RUNS {
        return Err(ComparisonError {
            message: format!(
                "A comparison accepts at most {MAX_COMPARISON_RUNS} runs, got {}.",
                runs.len()
            ),
        });
    }
    let corpus_id = runs[0].corpus_id.clone();
    let corpus_digest = runs[0].corpus_digest.clone();
    let mut rows: Vec<ComparisonRow> = runs
        .iter()
        .map(|run| ComparisonRow {
            provider: run.provider.clone(),
            model: run.model.clone(),
            cases_run: run.cases_run,
            cases_passed: run.cases_passed,
            turns: run.turns,
            tool_rounds: run.tool_rounds,
            input_tokens: run.input_tokens,
            output_tokens: run.output_tokens,
            cached_tokens: run.cached_tokens,
            failure_count: run.failure_count,
            failures: run.failures.len(),
            cancelled: run.cancelled,
        })
        .collect();
    rows.sort_by(|left, right| {
        (left.provider.as_str(), left.model.as_deref().unwrap_or("")).cmp(&(
            right.provider.as_str(),
            right.model.as_deref().unwrap_or(""),
        ))
    });
    Ok(ComparisonTable {
        informational: true,
        corpus_id,
        corpus_digest,
        comparable: rows.len() > 1,
        rows,
    })
}

/// Render a comparison as deterministic, terminal-safe text.
///
/// Host-composed values only: the caller still passes the result through the
/// terminal sanitizer like every other rendered line, because provider and model
/// labels originate in a profile a user can edit.
#[must_use]
pub fn render_comparison(table: &ComparisonTable) -> String {
    let mut out = String::new();
    out.push_str("Multi-model evaluation (INFORMATIONAL -- evidence only, never a gate)\n");
    out.push_str(&format!(
        "  corpus {} ({})\n",
        table.corpus_id,
        &table.corpus_digest[..table.corpus_digest.len().min(8)]
    ));
    if !table.comparable {
        out.push_str("  one run: a baseline, not a comparison\n");
    }
    out.push_str("  provider / model | cases | turns | tools | in/out/cached tokens | failures | cancelled\n");
    for row in &table.rows {
        let model = row.model.as_deref().unwrap_or("(default)");
        let tokens = format!(
            "{}/{}/{}",
            optional_tokens(row.input_tokens),
            optional_tokens(row.output_tokens),
            optional_tokens(row.cached_tokens)
        );
        out.push_str(&format!(
            "  {} / {} | {}/{} | {} | {} | {} | {} | {}\n",
            row.provider,
            model,
            row.cases_passed,
            row.cases_run,
            row.turns,
            row.tool_rounds,
            tokens,
            row.failure_count,
            if row.cancelled { "yes" } else { "no" }
        ));
    }
    out
}

fn optional_tokens(value: Option<u64>) -> String {
    match value {
        Some(tokens) => tokens.to_string(),
        None => "-".to_owned(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::evolution::EvaluationCase;

    fn run(provider: &str, model: Option<&str>) -> RunOutcome {
        RunOutcome {
            provider: provider.to_owned(),
            model: model.map(str::to_owned),
            corpus_id: "siralos-evaluation-smoke".to_owned(),
            corpus_digest: "0123456789abcdef".to_owned(),
            cases_run: 3,
            cases_passed: 2,
            turns: 3,
            tool_rounds: 1,
            input_tokens: Some(120),
            output_tokens: Some(30),
            cached_tokens: None,
            failure_count: 1,
            failures: vec!["case 'two' did not match".to_owned()],
            cancelled: false,
            wall_ms: 42,
        }
    }

    #[test]
    fn a_comparison_is_informational_by_construction() {
        let table = compare_runs(&[run("deterministic-fake", None)])
            .expect("comparable");
        assert!(table.informational, "the table can never claim to be a gate");
        assert!(!table.comparable, "one run is a baseline, not a comparison");
    }

    #[test]
    fn rows_are_ordered_so_the_same_runs_render_the_same_bytes() {
        let table = compare_runs(&[
            run("replay", Some("b")),
            run("deterministic-fake", None),
            run("replay", Some("a")),
        ])
        .expect("comparable");
        let order: Vec<(String, Option<String>)> = table
            .rows
            .iter()
            .map(|row| (row.provider.clone(), row.model.clone()))
            .collect();
        assert_eq!(
            order,
            vec![
                ("deterministic-fake".to_owned(), None),
                ("replay".to_owned(), Some("a".to_owned())),
                ("replay".to_owned(), Some("b".to_owned())),
            ]
        );
        assert!(table.comparable);
        let rendered = render_comparison(&table);
        assert_eq!(rendered, render_comparison(&table), "rendering is pure");
        assert!(rendered.contains("INFORMATIONAL"));
        assert!(
            rendered.contains("(default)"),
            "an absent model is shown, not blank"
        );
    }

    #[test]
    fn tokens_are_never_summed_across_runs_and_missing_usage_stays_missing() {
        let mut without_usage = run("replay", None);
        without_usage.input_tokens = None;
        without_usage.output_tokens = None;
        without_usage.cached_tokens = None;
        without_usage.failures.clear();
        without_usage.failure_count = 0;
        assert_eq!(without_usage.total_tokens(), None);
        let table =
            compare_runs(&[run("deterministic-fake", None), without_usage])
                .expect("comparable");
        let replay = table
            .rows
            .iter()
            .find(|row| row.provider == "replay")
            .expect("the replay row");
        assert_eq!(replay.input_tokens, None, "unreported usage is not zero");
        assert_eq!(replay.failures, 0);
        assert!(render_comparison(&table).contains("-/-"));
    }

    fn corpus(expected: &str) -> EvaluationCorpus {
        EvaluationCorpus {
            id: "siralos-evaluation-smoke".to_owned(),
            cases: vec![
                EvaluationCase {
                    id: "a".to_owned(),
                    prompt: "p".to_owned(),
                    expected: "e".to_owned(),
                },
                EvaluationCase {
                    id: "b".to_owned(),
                    prompt: "p".to_owned(),
                    expected: expected.to_owned(),
                },
            ],
        }
    }

    #[test]
    fn the_task_set_digest_binds_the_corpus_and_not_the_answers() {
        let first = task_set_digest(&corpus("e")).expect("digest");
        assert_eq!(first.len(), 64);
        assert_eq!(
            first,
            task_set_digest(&corpus("e")).expect("digest"),
            "the same task set always digests the same"
        );
        assert_ne!(
            first,
            task_set_digest(&corpus("other")).expect("digest"),
            "a changed expectation moves the task-set identity"
        );
        // The answers are NOT part of this identity: two providers that
        // answer the same corpus differently still share one task set.
        let answered: BTreeMap<String, String> = BTreeMap::from([
            ("a".to_owned(), "nope".to_owned()),
            ("b".to_owned(), "e".to_owned()),
        ]);
        let (evidence, _) =
            crate::evolution::create_corpus_evidence(&corpus("e"), &answered)
                .expect("evidence");
        assert_ne!(
            evidence.corpus_digest, first,
            "corpus evidence binds the answers, the task set does not"
        );
    }

    #[test]
    fn an_empty_or_oversized_comparison_is_a_typed_refusal() {
        let empty = compare_runs(&[]).expect_err("nothing to compare");
        assert!(empty.to_string().contains("at least one"));
        let oversized: Vec<RunOutcome> = (0..=MAX_COMPARISON_RUNS)
            .map(|index| run(&format!("provider-{index}"), None))
            .collect();
        let refused =
            compare_runs(&oversized).expect_err("over the run bound");
        assert!(refused.to_string().contains("at most"), "{}", refused);
    }
}
