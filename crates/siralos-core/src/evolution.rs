//! Evolution foundations (Stage 6.1, decision 58): bounded, measurement-driven
//! evaluation corpora and deterministic baselines.
//!
//! Stage 6 is the bounded `/evolve` workflow (`baseline → candidate →
//! evaluation → comparison → reject|propose`) per ADR 0036 §§44–48. This
//! module owns the first slice: **typed, bounded evaluation corpora** with
//! deterministic digests and scoring. An evaluation corpus is a declarative
//! set of cases (`id → prompt → expected`); it is not code, not a plugin,
//! and never grants authority (C2). Scoring is exact-match over the
//! expected payload, fully deterministic and order-independent.
//!
//! All operations are pure over in-memory inputs: no spawn, no fs, no
//! wall clock. Every case and corpus is validated at the boundary; an
//! invalid corpus never produces a digest and is reported with a typed
//! reason.

use std::collections::{BTreeMap, BTreeSet};

use crate::identity::{CanonicalValue, compute_artifact_digest};

/// Maximum corpus id length in UTF-8 bytes.
pub const MAX_CORPUS_ID_BYTES: usize = 64;
/// Maximum number of cases in one corpus.
pub const MAX_CORPUS_CASES: usize = 64;
/// Maximum case id length in UTF-8 bytes.
pub const MAX_CASE_ID_BYTES: usize = 64;
/// Maximum prompt length in UTF-8 bytes.
pub const MAX_PROMPT_BYTES: usize = 1024;
/// Maximum expected output length in UTF-8 bytes.
pub const MAX_EXPECTED_BYTES: usize = 1024;

/// One evaluation case: the model prompt and the expected output that
/// defines the deterministic baseline for scoring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationCase {
    /// Stable case id (non-empty, bounded, NUL-free, unique within corpus).
    pub id: String,
    /// Prompt text (bounded).
    pub prompt: String,
    /// Expected output for exact-match scoring (bounded).
    pub expected: String,
}

/// A bounded, declarative evaluation corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationCorpus {
    /// Stable corpus id (non-empty, bounded, NUL-free).
    pub id: String,
    /// Cases in declaration order (validation enforces sorted uniqueness).
    pub cases: Vec<EvaluationCase>,
}

/// Typed validation failure for a malformed case or corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusValidationError {
    /// Deterministic, human-readable reason.
    pub message: String,
}

impl EvaluationCase {
    /// Validate one case.
    ///
    /// # Errors
    ///
    /// Returns [`CorpusValidationError`] for empty/oversized ids, NUL bytes,
    /// or oversized prompt/expected payloads.
    pub fn validate(&self) -> Result<(), CorpusValidationError> {
        if self.id.is_empty() {
            return Err(CorpusValidationError {
                message: "An evaluation case requires a non-empty id."
                    .to_owned(),
            });
        }
        if self.id.len() > MAX_CASE_ID_BYTES {
            return Err(CorpusValidationError {
                message: format!(
                    "The case id exceeds the {MAX_CASE_ID_BYTES}-byte bound."
                ),
            });
        }
        if self.id.contains('\0') {
            return Err(CorpusValidationError {
                message: "A case id must not contain NUL.".to_owned(),
            });
        }
        if self.prompt.len() > MAX_PROMPT_BYTES {
            return Err(CorpusValidationError {
                message: format!(
                    "The case prompt exceeds the {MAX_PROMPT_BYTES}-byte bound."
                ),
            });
        }
        if self.prompt.contains('\0') {
            return Err(CorpusValidationError {
                message: "A case prompt must not contain NUL.".to_owned(),
            });
        }
        if self.expected.len() > MAX_EXPECTED_BYTES {
            return Err(CorpusValidationError {
                message: format!(
                    "The case expected output exceeds the {MAX_EXPECTED_BYTES}-byte bound."
                ),
            });
        }
        if self.expected.contains('\0') {
            return Err(CorpusValidationError {
                message: "A case expected output must not contain NUL."
                    .to_owned(),
            });
        }
        Ok(())
    }
}

impl EvaluationCorpus {
    /// Validate bounds and uniqueness. Deterministic order: id, then case
    /// count, then per-case checks in declaration order, then duplicate
    /// detection over sorted ids.
    ///
    /// # Errors
    ///
    /// Returns [`CorpusValidationError`] for malformed ids, oversized counts,
    /// per-case failures, or duplicate case ids.
    pub fn validate(&self) -> Result<(), CorpusValidationError> {
        if self.id.is_empty() {
            return Err(CorpusValidationError {
                message: "An evaluation corpus requires a non-empty id."
                    .to_owned(),
            });
        }
        if self.id.len() > MAX_CORPUS_ID_BYTES {
            return Err(CorpusValidationError {
                message: format!(
                    "The corpus id exceeds the {MAX_CORPUS_ID_BYTES}-byte bound."
                ),
            });
        }
        if self.id.contains('\0') {
            return Err(CorpusValidationError {
                message: "A corpus id must not contain NUL.".to_owned(),
            });
        }
        if self.cases.len() > MAX_CORPUS_CASES {
            return Err(CorpusValidationError {
                message: format!(
                    "The corpus exceeds the {MAX_CORPUS_CASES}-case bound."
                ),
            });
        }
        for case in &self.cases {
            case.validate()?;
        }
        let mut seen = BTreeSet::new();
        for case in &self.cases {
            if !seen.insert(case.id.as_str()) {
                return Err(CorpusValidationError {
                    message: format!(
                        "The corpus contains duplicate case id {:?}.",
                        case.id
                    ),
                });
            }
        }
        Ok(())
    }
}

/// The typed outcome of scoring a corpus against a candidate output map.
#[derive(Debug, Clone, PartialEq)]
pub struct CorpusScore {
    /// Deterministic score in `[0.0, 1.0]`.
    pub score: f64,
    /// Number of cases that matched exactly.
    pub matches: usize,
    /// Total cases in the corpus.
    pub total: usize,
}

/// Digest-bound evidence for one corpus evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusEvidence {
    /// Stable corpus id.
    pub corpus_id: String,
    /// Number of cases.
    pub case_count: usize,
    /// Deterministic score numerator/denominator rendered as `matches/total`.
    pub score: String,
    /// Score as a formatted string with two decimals (deterministic).
    pub score_value: String,
    /// Domain-separated digest over the ordered case identities and the
    /// candidate matching outcome.
    pub corpus_digest: String,
}

/// Create digest-bound evidence for a corpus and its candidate scoring.
///
/// The payload binds the corpus id, the ordered case list (sorted by id,
/// each entry is `id + expected`), and the per-case match outcome, so any
/// change to the corpus or to the candidate's answers moves the digest.
///
/// # Errors
///
/// Returns [`CorpusValidationError`] for malformed corpora or digest
/// failures.
pub fn create_corpus_evidence(
    corpus: &EvaluationCorpus,
    candidate: &BTreeMap<String, String>,
) -> Result<(CorpusEvidence, CorpusScore), CorpusValidationError> {
    corpus.validate()?;
    let mut sorted = corpus.cases.clone();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut matches = 0usize;
    for case in &sorted {
        if let Some(output) = candidate.get(&case.id) {
            if output == &case.expected {
                matches += 1;
            }
        }
    }
    let total = sorted.len();
    let score = if total == 0 { 1.0 } else { matches as f64 / total as f64 };
    let score_value = format!("{score:.2}");
    let cases_payload: Vec<CanonicalValue> = sorted
        .iter()
        .map(|case| {
            CanonicalValue::Object(BTreeMap::from([
                (
                    "expected".to_owned(),
                    CanonicalValue::Str(case.expected.clone()),
                ),
                ("id".to_owned(), CanonicalValue::Str(case.id.clone())),
                (
                    "matched".to_owned(),
                    CanonicalValue::Bool(
                        candidate.get(&case.id) == Some(&case.expected),
                    ),
                ),
                (
                    "prompt".to_owned(),
                    CanonicalValue::Str(case.prompt.clone()),
                ),
            ]))
        })
        .collect();
    let payload = CanonicalValue::Object(BTreeMap::from([
        ("cases".to_owned(), CanonicalValue::Array(cases_payload)),
        ("corpusId".to_owned(), CanonicalValue::Str(corpus.id.clone())),
        ("matches".to_owned(), CanonicalValue::U64(matches as u64)),
        ("total".to_owned(), CanonicalValue::U64(total as u64)),
    ]));
    let corpus_digest = compute_artifact_digest("CorpusEvidence", 1, &payload)
        .map_err(|error| CorpusValidationError { message: error.message })?
        .value;
    let evidence = CorpusEvidence {
        corpus_id: corpus.id.clone(),
        case_count: total,
        score: format!("{matches}/{total}"),
        score_value,
        corpus_digest,
    };
    let score_detail = CorpusScore { score, matches, total };
    Ok((evidence, score_detail))
}

/// Deterministic report-safe rendering of corpus evidence.
#[must_use]
pub fn render_corpus_evidence(evidence: &CorpusEvidence) -> String {
    format!(
        "corpus {} cases={} score={} ({})",
        evidence.corpus_id,
        evidence.case_count,
        evidence.score_value,
        evidence.score
    )
}

/// The typed outcome of evaluating a corpus at the differential boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum CorpusEvaluation {
    /// The corpus was valid and scored.
    Valid {
        /// Digest-bound evidence.
        evidence: CorpusEvidence,
        /// Numeric score detail.
        score: CorpusScore,
    },
    /// The corpus was invalid; the truthful reason is carried.
    Invalid {
        /// Deterministic reason.
        reason: String,
    },
}

impl CorpusEvaluation {
    /// Stable disposition token.
    #[must_use]
    pub fn disposition(&self) -> &'static str {
        match self {
            Self::Valid { .. } => "valid",
            Self::Invalid { .. } => "invalid",
        }
    }
}

/// Evaluate a corpus and candidate map at the pure boundary. An invalid
/// corpus never panics; it returns the typed `Invalid` disposition with a
/// truthful reason (never a throw).
#[must_use]
pub fn evaluate_corpus(
    corpus: Option<EvaluationCorpus>,
    candidate: &BTreeMap<String, String>,
) -> CorpusEvaluation {
    let Some(corpus) = corpus else {
        // Absent corpus is a valid empty evaluation (transparent).
        let empty =
            EvaluationCorpus { id: "absent".to_owned(), cases: Vec::new() };
        let (evidence, score) = create_corpus_evidence(&empty, candidate)
            .expect("empty corpus is valid");
        return CorpusEvaluation::Valid { evidence, score };
    };
    match create_corpus_evidence(&corpus, candidate) {
        Ok((evidence, score)) => CorpusEvaluation::Valid { evidence, score },
        Err(error) => CorpusEvaluation::Invalid { reason: error.message },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CorpusValidationError, EvaluationCase, EvaluationCorpus,
        create_corpus_evidence, evaluate_corpus, render_corpus_evidence,
    };
    use std::collections::BTreeMap;

    fn case(id: &str, expected: &str) -> EvaluationCase {
        EvaluationCase {
            id: id.to_owned(),
            prompt: format!("prompt for {id}"),
            expected: expected.to_owned(),
        }
    }

    #[test]
    fn valid_corpus_scores_exact_match() {
        let corpus = EvaluationCorpus {
            id: "baseline".to_owned(),
            cases: vec![case("a", "ok"), case("b", "ok")],
        };
        let candidate = BTreeMap::from([
            ("a".to_owned(), "ok".to_owned()),
            ("b".to_owned(), "ok".to_owned()),
        ]);
        let (evidence, score) =
            create_corpus_evidence(&corpus, &candidate).expect("valid");
        assert_eq!(score.matches, 2);
        assert_eq!(score.total, 2);
        assert_eq!(score.score, 1.0);
        assert_eq!(evidence.case_count, 2);
        assert!(evidence.corpus_digest.len() == 64);
        assert!(render_corpus_evidence(&evidence).contains("score=1.00"));
    }

    #[test]
    fn partial_score_is_deterministic() {
        let corpus = EvaluationCorpus {
            id: "baseline".to_owned(),
            cases: vec![case("a", "ok"), case("b", "ok")],
        };
        let candidate = BTreeMap::from([("a".to_owned(), "ok".to_owned())]);
        let (evidence, score) =
            create_corpus_evidence(&corpus, &candidate).expect("valid");
        assert_eq!(score.matches, 1);
        assert_eq!(score.score, 0.5);
        assert_eq!(evidence.score, "1/2");
    }

    #[test]
    fn empty_corpus_scores_one() {
        let corpus =
            EvaluationCorpus { id: "empty".to_owned(), cases: Vec::new() };
        let candidate = BTreeMap::new();
        let (evidence, score) =
            create_corpus_evidence(&corpus, &candidate).expect("valid");
        assert_eq!(score.score, 1.0);
        assert_eq!(evidence.case_count, 0);
    }

    #[test]
    fn duplicate_ids_are_invalid() {
        let corpus = EvaluationCorpus {
            id: "dup".to_owned(),
            cases: vec![case("a", "ok"), case("a", "ok")],
        };
        let err = corpus.validate().expect_err("duplicate");
        assert!(err.message.contains("duplicate"));
    }

    #[test]
    fn absent_corpus_is_valid_empty() {
        let eval = evaluate_corpus(None, &BTreeMap::new());
        match eval {
            super::CorpusEvaluation::Valid { evidence, score } => {
                assert_eq!(evidence.corpus_id, "absent");
                assert_eq!(score.total, 0);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn digest_is_order_independent() {
        let c1 = EvaluationCorpus {
            id: "id".to_owned(),
            cases: vec![case("b", "ok"), case("a", "ok")],
        };
        let c2 = EvaluationCorpus {
            id: "id".to_owned(),
            cases: vec![case("a", "ok"), case("b", "ok")],
        };
        let cand = BTreeMap::from([
            ("a".to_owned(), "ok".to_owned()),
            ("b".to_owned(), "ok".to_owned()),
        ]);
        let (e1, _) = create_corpus_evidence(&c1, &cand).expect("valid");
        let (e2, _) = create_corpus_evidence(&c2, &cand).expect("valid");
        assert_eq!(e1.corpus_digest, e2.corpus_digest);
    }

    #[test]
    fn oversized_prompt_is_invalid() {
        let corpus = EvaluationCorpus {
            id: "big".to_owned(),
            cases: vec![EvaluationCase {
                id: "a".to_owned(),
                prompt: "x".repeat(super::MAX_PROMPT_BYTES + 1),
                expected: "ok".to_owned(),
            }],
        };
        let err: CorpusValidationError =
            corpus.validate().expect_err("oversized prompt");
        assert!(err.message.contains("prompt exceeds"));
    }
}
