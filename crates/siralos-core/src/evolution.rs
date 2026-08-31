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

/// The escalation level for an evolve proposal (ADR 0036 45): the
/// cheapest configurable layer that could realize the measured improvement
/// is preferred. Lower-cost layers must be tried before Host code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Escalation {
    /// Profile-level improvement (cheapest).
    Profile,
    /// Context-level improvement.
    Context,
    /// Skill-level improvement.
    Skill,
    /// Plugin-level improvement.
    Plugin,
    /// Host-level improvement (most expensive).
    Host,
}

impl Escalation {
    /// Stable string for digests and rendering.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Context => "context",
            Self::Skill => "skill",
            Self::Plugin => "plugin",
            Self::Host => "host",
        }
    }

    /// Parse a bounded escalation string.
    ///
    /// # Errors
    ///
    /// Returns [`CorpusValidationError`] for unknown values.
    pub fn parse(value: &str) -> Result<Self, CorpusValidationError> {
        match value {
            "profile" => Ok(Self::Profile),
            "context" => Ok(Self::Context),
            "skill" => Ok(Self::Skill),
            "plugin" => Ok(Self::Plugin),
            "host" => Ok(Self::Host),
            _ => Err(CorpusValidationError {
                message: format!("Unknown escalation level {value:?}."),
            }),
        }
    }
}

/// The typed outcome of an evolve comparison: propose the candidate when
/// its score strictly exceeds the baseline; otherwise reject (equal scores
/// reject, per the deletion-preference and measurement-driven rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowOutcome {
    /// Candidate does not strictly improve over baseline.
    Reject,
    /// Candidate strictly improves over baseline.
    Propose,
}

impl WorkflowOutcome {
    /// Stable string for digests and rendering.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::Propose => "propose",
        }
    }
}

/// Digest-bound evidence for one workflow comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowEvidence {
    /// Baseline evidence.
    pub baseline: CorpusEvidence,
    /// Candidate evidence.
    pub candidate: CorpusEvidence,
    /// Strict improvement (candidate score - baseline score).
    pub improvement: f64,
    /// Formatted improvement with sign and two decimals.
    pub improvement_value: String,
    /// Typed decision.
    pub decision: WorkflowOutcome,
    /// Escalation level that would realize the improvement.
    pub escalation: Escalation,
    /// Domain-separated digest over baseline/candidate digests, scores,
    /// decision, and escalation.
    pub workflow_digest: String,
}

/// Create digest-bound evidence for a workflow comparison.
///
/// # Errors
///
/// Returns [`CorpusValidationError`] for digest failures or unknown
/// escalation.
pub fn create_workflow_evidence(
    baseline: &CorpusEvidence,
    baseline_score: &CorpusScore,
    candidate: &CorpusEvidence,
    candidate_score: &CorpusScore,
    escalation: Escalation,
) -> Result<(WorkflowEvidence, WorkflowOutcome), CorpusValidationError> {
    let improvement = candidate_score.score - baseline_score.score;
    let improvement_value = format!("{improvement:+.2}");
    let decision = if candidate_score.score > baseline_score.score {
        WorkflowOutcome::Propose
    } else {
        WorkflowOutcome::Reject
    };
    let payload = CanonicalValue::Object(BTreeMap::from([
        (
            "baselineDigest".to_owned(),
            CanonicalValue::Str(baseline.corpus_digest.clone()),
        ),
        (
            "baselineScore".to_owned(),
            CanonicalValue::Str(baseline.score_value.clone()),
        ),
        (
            "candidateDigest".to_owned(),
            CanonicalValue::Str(candidate.corpus_digest.clone()),
        ),
        (
            "candidateScore".to_owned(),
            CanonicalValue::Str(candidate.score_value.clone()),
        ),
        (
            "decision".to_owned(),
            CanonicalValue::Str(decision.as_str().to_owned()),
        ),
        (
            "escalation".to_owned(),
            CanonicalValue::Str(escalation.as_str().to_owned()),
        ),
        (
            "improvement".to_owned(),
            CanonicalValue::Str(improvement_value.clone()),
        ),
    ]));
    let workflow_digest =
        compute_artifact_digest("WorkflowEvidence", 1, &payload)
            .map_err(|error| CorpusValidationError { message: error.message })?
            .value;
    let evidence = WorkflowEvidence {
        baseline: baseline.clone(),
        candidate: candidate.clone(),
        improvement,
        improvement_value,
        decision: decision.clone(),
        escalation,
        workflow_digest,
    };
    Ok((evidence, decision))
}

/// Deterministic rendering of workflow evidence.
#[must_use]
pub fn render_workflow_evidence(evidence: &WorkflowEvidence) -> String {
    format!(
        "workflow {} baseline={} candidate={} improvement={} escalation={}",
        evidence.decision.as_str(),
        evidence.baseline.score_value,
        evidence.candidate.score_value,
        evidence.improvement_value,
        evidence.escalation.as_str()
    )
}

/// The typed outcome of evaluating a workflow at the pure boundary.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum WorkflowEvaluation {
    /// The workflow was valid and produced evidence.
    Valid {
        /// Digest-bound evidence.
        evidence: WorkflowEvidence,
        /// Typed decision.
        decision: WorkflowOutcome,
    },
    /// The workflow was invalid; the truthful reason is carried.
    Invalid {
        /// Deterministic reason.
        reason: String,
    },
}

impl WorkflowEvaluation {
    /// Stable disposition token.
    #[must_use]
    pub fn disposition(&self) -> &'static str {
        match self {
            Self::Valid { .. } => "valid",
            Self::Invalid { .. } => "invalid",
        }
    }
}

/// Evaluate a workflow at the pure boundary. Invalid baselines or
/// candidates (malformed corpora, unknown escalation) never panic; they
/// return the typed `Invalid` disposition.
#[must_use]
pub fn evaluate_workflow(
    baseline_corpus: Option<EvaluationCorpus>,
    baseline_candidate: &BTreeMap<String, String>,
    candidate_corpus: Option<EvaluationCorpus>,
    candidate_candidate: &BTreeMap<String, String>,
    escalation_raw: &str,
) -> WorkflowEvaluation {
    let escalation = match Escalation::parse(escalation_raw) {
        Ok(level) => level,
        Err(error) => {
            return WorkflowEvaluation::Invalid { reason: error.message };
        }
    };
    let baseline_eval = evaluate_corpus(baseline_corpus, baseline_candidate);
    let candidate_eval =
        evaluate_corpus(candidate_corpus, candidate_candidate);
    let (baseline_evidence, baseline_score) = match baseline_eval {
        CorpusEvaluation::Valid { evidence, score } => (evidence, score),
        CorpusEvaluation::Invalid { reason } => {
            return WorkflowEvaluation::Invalid { reason };
        }
    };
    let (candidate_evidence, candidate_score) = match candidate_eval {
        CorpusEvaluation::Valid { evidence, score } => (evidence, score),
        CorpusEvaluation::Invalid { reason } => {
            return WorkflowEvaluation::Invalid { reason };
        }
    };
    match create_workflow_evidence(
        &baseline_evidence,
        &baseline_score,
        &candidate_evidence,
        &candidate_score,
        escalation,
    ) {
        Ok((evidence, decision)) => {
            WorkflowEvaluation::Valid { evidence, decision }
        }
        Err(error) => WorkflowEvaluation::Invalid { reason: error.message },
    }
}

/// Maximum proposal id length in UTF-8 bytes.
pub const MAX_PROPOSAL_ID_BYTES: usize = 64;
/// Maximum proposal description length in UTF-8 bytes.
pub const MAX_PROPOSAL_DESCRIPTION_BYTES: usize = 512;

/// A typed proposal for an evolve improvement (Stage 6.3, decision 58 C2):
/// `Skill` and `Plugin` proposals carry declarative guidance or plugin
/// selection; `Host` proposals carry a bounded description of the required
/// Host change and are the only ones that may set `requires_host_approval`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// Stable proposal id (non-empty, bounded, NUL-free).
    pub id: String,
    /// The workflow digest this proposal derives from (64 hex).
    pub workflow_digest: String,
    /// Escalation level that would realize the improvement.
    pub kind: Escalation,
    /// Bounded human-readable description of the proposed change.
    pub description: String,
    /// Whether the proposal requires explicit Host approval (only Host).
    pub requires_host_approval: bool,
}

/// Digest-bound evidence for one proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalEvidence {
    /// Stable proposal id.
    pub proposal_id: String,
    /// The workflow digest this proposal derives from.
    pub workflow_digest: String,
    /// Escalation level.
    pub kind: Escalation,
    /// Bounded description.
    pub description: String,
    /// Whether Host approval is required.
    pub requires_host_approval: bool,
    /// Domain-separated digest over the proposal fields.
    pub proposal_digest: String,
}

/// Typed validation failure for a malformed proposal.
pub type ProposalValidationError = CorpusValidationError;

impl Proposal {
    /// Validate bounds, NUL bytes, digest shape, kind, and Host-gating
    /// invariant: only `Host` proposals may require Host approval, and
    /// every `Host` proposal must require it.
    pub fn validate(&self) -> Result<(), ProposalValidationError> {
        if self.id.is_empty() {
            return Err(ProposalValidationError {
                message: "A proposal requires a non-empty id.".to_owned(),
            });
        }
        if self.id.len() > MAX_PROPOSAL_ID_BYTES {
            return Err(ProposalValidationError {
                message: format!(
                    "The proposal id exceeds the {MAX_PROPOSAL_ID_BYTES}-byte bound."
                ),
            });
        }
        if self.id.contains('\0') {
            return Err(ProposalValidationError {
                message: "A proposal id must not contain NUL.".to_owned(),
            });
        }
        if self.workflow_digest.len() != 64
            || !self.workflow_digest.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(ProposalValidationError {
                message:
                    "A proposal workflow digest must be 64 hex characters."
                        .to_owned(),
            });
        }
        if self.description.is_empty() {
            return Err(ProposalValidationError {
                message: "A proposal requires a non-empty description."
                    .to_owned(),
            });
        }
        if self.description.len() > MAX_PROPOSAL_DESCRIPTION_BYTES {
            return Err(ProposalValidationError {
                message: format!(
                    "The proposal description exceeds the {MAX_PROPOSAL_DESCRIPTION_BYTES}-byte bound."
                ),
            });
        }
        if self.description.contains('\0') {
            return Err(ProposalValidationError {
                message: "A proposal description must not contain NUL."
                    .to_owned(),
            });
        }
        match self.kind {
            Escalation::Host => {
                if !self.requires_host_approval {
                    return Err(ProposalValidationError {
                        message: "A Host proposal must require Host approval."
                            .to_owned(),
                    });
                }
            }
            Escalation::Skill | Escalation::Plugin => {
                if self.requires_host_approval {
                    return Err(ProposalValidationError {
                        message:
                            "Only Host proposals may require Host approval."
                                .to_owned(),
                    });
                }
            }
            Escalation::Profile | Escalation::Context => {
                return Err(ProposalValidationError {
                    message: "A proposal kind must be skill, plugin, or host."
                        .to_owned(),
                });
            }
        }
        Ok(())
    }
}

/// Create digest-bound evidence for a proposal.
///
/// # Errors
///
/// Returns [`ProposalValidationError`] for malformed proposals or digest
/// failures.
pub fn create_proposal_evidence(
    proposal: &Proposal,
) -> Result<ProposalEvidence, ProposalValidationError> {
    proposal.validate()?;
    let payload = CanonicalValue::Object(BTreeMap::from([
        (
            "description".to_owned(),
            CanonicalValue::Str(proposal.description.clone()),
        ),
        (
            "kind".to_owned(),
            CanonicalValue::Str(proposal.kind.as_str().to_owned()),
        ),
        ("proposalId".to_owned(), CanonicalValue::Str(proposal.id.clone())),
        (
            "requiresHostApproval".to_owned(),
            CanonicalValue::Bool(proposal.requires_host_approval),
        ),
        (
            "workflowDigest".to_owned(),
            CanonicalValue::Str(proposal.workflow_digest.clone()),
        ),
    ]));
    let proposal_digest =
        compute_artifact_digest("ProposalEvidence", 1, &payload)
            .map_err(|error| ProposalValidationError {
                message: error.message,
            })?
            .value;
    Ok(ProposalEvidence {
        proposal_id: proposal.id.clone(),
        workflow_digest: proposal.workflow_digest.clone(),
        kind: proposal.kind.clone(),
        description: proposal.description.clone(),
        requires_host_approval: proposal.requires_host_approval,
        proposal_digest,
    })
}

/// Deterministic rendering of proposal evidence.
#[must_use]
pub fn render_proposal_evidence(evidence: &ProposalEvidence) -> String {
    format!(
        "proposal {} kind={} host_approval={} digest={}",
        evidence.proposal_id,
        evidence.kind.as_str(),
        evidence.requires_host_approval,
        &evidence.proposal_digest[..8]
    )
}

/// The typed outcome of evaluating a proposal at the pure boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalEvaluation {
    /// The proposal was valid and produced evidence.
    Valid {
        /// Digest-bound evidence.
        evidence: ProposalEvidence,
    },
    /// The proposal was invalid; the truthful reason is carried.
    Invalid {
        /// Deterministic reason.
        reason: String,
    },
}

impl ProposalEvaluation {
    /// Stable disposition token.
    #[must_use]
    pub fn disposition(&self) -> &'static str {
        match self {
            Self::Valid { .. } => "valid",
            Self::Invalid { .. } => "invalid",
        }
    }
}

/// Evaluate a proposal at the pure boundary. Invalid proposals never
/// panic; they return the typed `Invalid` disposition.
#[must_use]
pub fn evaluate_proposal(proposal: Option<Proposal>) -> ProposalEvaluation {
    let Some(proposal) = proposal else {
        return ProposalEvaluation::Invalid {
            reason: "A proposal is required.".to_owned(),
        };
    };
    match create_proposal_evidence(&proposal) {
        Ok(evidence) => ProposalEvaluation::Valid { evidence },
        Err(error) => ProposalEvaluation::Invalid { reason: error.message },
    }
}

/// Maximum release id length in UTF-8 bytes.
pub const MAX_RELEASE_ID_BYTES: usize = 64;
/// Maximum version string length in UTF-8 bytes.
pub const MAX_VERSION_BYTES: usize = 32;

/// Compatibility level for a release (Stage 6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compatibility {
    /// Patch-level compatible change.
    Patch,
    /// Minor compatible change.
    Compatible,
    /// Breaking change.
    Breaking,
}

impl Compatibility {
    /// Stable string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Compatible => "compatible",
            Self::Breaking => "breaking",
        }
    }

    /// Parse a bounded compatibility string.
    ///
    /// # Errors
    ///
    /// Returns [`CorpusValidationError`] for unknown values.
    pub fn parse(value: &str) -> Result<Self, CorpusValidationError> {
        match value {
            "patch" => Ok(Self::Patch),
            "compatible" => Ok(Self::Compatible),
            "breaking" => Ok(Self::Breaking),
            _ => Err(CorpusValidationError {
                message: format!("Unknown compatibility level {value:?}."),
            }),
        }
    }
}

/// A bounded release record for packaging & stabilization (Stage 6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Stable release id.
    pub id: String,
    /// Semver-like version `MAJOR.MINOR.PATCH`.
    pub version: String,
    /// Previous version for compatibility check.
    pub previous_version: String,
    /// Compatibility level.
    pub compatibility: Compatibility,
}

/// Digest-bound evidence for one release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseEvidence {
    /// Stable release id.
    pub release_id: String,
    /// Version string.
    pub version: String,
    /// Compatibility level.
    pub compatibility: Compatibility,
    /// Domain-separated digest.
    pub release_digest: String,
}

impl Release {
    /// Validate bounds, NUL bytes, semver shape, and compatibility.
    pub fn validate(&self) -> Result<(), CorpusValidationError> {
        if self.id.is_empty() {
            return Err(CorpusValidationError {
                message: "A release requires a non-empty id.".to_owned(),
            });
        }
        if self.id.len() > MAX_RELEASE_ID_BYTES {
            return Err(CorpusValidationError {
                message: format!(
                    "The release id exceeds the {MAX_RELEASE_ID_BYTES}-byte bound."
                ),
            });
        }
        if self.id.contains('\0') {
            return Err(CorpusValidationError {
                message: "A release id must not contain NUL.".to_owned(),
            });
        }
        for (label, value) in [
            ("version", &self.version),
            ("previous_version", &self.previous_version),
        ] {
            if value.is_empty() || value.len() > MAX_VERSION_BYTES {
                return Err(CorpusValidationError {
                    message: format!(
                        "A release {label} must be 1..={MAX_VERSION_BYTES} bytes."
                    ),
                });
            }
            if value.contains('\0') {
                return Err(CorpusValidationError {
                    message: format!(
                        "A release {label} must not contain NUL."
                    ),
                });
            }
            let parts: Vec<&str> = value.split('.').collect();
            if parts.len() != 3
                || parts.iter().any(|p| {
                    p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())
                })
            {
                return Err(CorpusValidationError {
                    message: format!(
                        "A release {label} must be MAJOR.MINOR.PATCH numeric."
                    ),
                });
            }
        }
        Ok(())
    }
}

/// Create digest-bound evidence for a release.
///
/// # Errors
///
/// Returns [`CorpusValidationError`] for malformed releases or digest
/// failures.
pub fn create_release_evidence(
    release: &Release,
) -> Result<ReleaseEvidence, CorpusValidationError> {
    release.validate()?;
    let payload = CanonicalValue::Object(BTreeMap::from([
        (
            "compatibility".to_owned(),
            CanonicalValue::Str(release.compatibility.as_str().to_owned()),
        ),
        ("id".to_owned(), CanonicalValue::Str(release.id.clone())),
        (
            "previousVersion".to_owned(),
            CanonicalValue::Str(release.previous_version.clone()),
        ),
        ("version".to_owned(), CanonicalValue::Str(release.version.clone())),
    ]));
    let release_digest =
        compute_artifact_digest("ReleaseEvidence", 1, &payload)
            .map_err(|error| CorpusValidationError { message: error.message })?
            .value;
    Ok(ReleaseEvidence {
        release_id: release.id.clone(),
        version: release.version.clone(),
        compatibility: release.compatibility.clone(),
        release_digest,
    })
}

/// Deterministic rendering of release evidence.
#[must_use]
pub fn render_release_evidence(evidence: &ReleaseEvidence) -> String {
    format!(
        "release {} version={} compat={} digest={}",
        evidence.release_id,
        evidence.version,
        evidence.compatibility.as_str(),
        &evidence.release_digest[..8]
    )
}

/// The typed outcome of evaluating a release at the pure boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseEvaluation {
    /// The release was valid.
    Valid {
        /// Digest-bound evidence.
        evidence: ReleaseEvidence,
    },
    /// The release was invalid.
    Invalid {
        /// Deterministic reason.
        reason: String,
    },
}

impl ReleaseEvaluation {
    /// Stable disposition token.
    #[must_use]
    pub fn disposition(&self) -> &'static str {
        match self {
            Self::Valid { .. } => "valid",
            Self::Invalid { .. } => "invalid",
        }
    }
}

/// Evaluate a release at the pure boundary.
#[must_use]
pub fn evaluate_release(release: Option<Release>) -> ReleaseEvaluation {
    let Some(release) = release else {
        return ReleaseEvaluation::Invalid {
            reason: "A release is required.".to_owned(),
        };
    };
    match create_release_evidence(&release) {
        Ok(evidence) => ReleaseEvaluation::Valid { evidence },
        Err(error) => ReleaseEvaluation::Invalid { reason: error.message },
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

    #[test]
    fn workflow_proposes_when_candidate_improves() {
        let baseline = EvaluationCorpus {
            id: "baseline".to_owned(),
            cases: vec![case("a", "ok"), case("b", "ok")],
        };
        let candidate = EvaluationCorpus {
            id: "candidate".to_owned(),
            cases: vec![case("a", "ok"), case("b", "ok")],
        };
        let baseline_cand =
            BTreeMap::from([("a".to_owned(), "wrong".to_owned())]);
        let candidate_cand = BTreeMap::from([
            ("a".to_owned(), "ok".to_owned()),
            ("b".to_owned(), "ok".to_owned()),
        ]);
        let eval = super::evaluate_workflow(
            Some(baseline),
            &baseline_cand,
            Some(candidate),
            &candidate_cand,
            "skill",
        );
        match eval {
            super::WorkflowEvaluation::Valid { evidence, decision } => {
                assert_eq!(decision, super::WorkflowOutcome::Propose);
                assert_eq!(evidence.escalation, super::Escalation::Skill);
                assert!(evidence.improvement > 0.0);
                assert!(evidence.workflow_digest.len() == 64);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn workflow_rejects_when_not_improving() {
        let baseline = EvaluationCorpus {
            id: "baseline".to_owned(),
            cases: vec![case("a", "ok"), case("b", "ok")],
        };
        let candidate = EvaluationCorpus {
            id: "candidate".to_owned(),
            cases: vec![case("a", "ok"), case("b", "ok")],
        };
        let baseline_cand = BTreeMap::from([
            ("a".to_owned(), "ok".to_owned()),
            ("b".to_owned(), "ok".to_owned()),
        ]);
        let candidate_cand =
            BTreeMap::from([("a".to_owned(), "wrong".to_owned())]);
        let eval = super::evaluate_workflow(
            Some(baseline),
            &baseline_cand,
            Some(candidate),
            &candidate_cand,
            "profile",
        );
        match eval {
            super::WorkflowEvaluation::Valid { decision, .. } => {
                assert_eq!(decision, super::WorkflowOutcome::Reject);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn workflow_invalid_on_bad_escalation() {
        let corpus = EvaluationCorpus {
            id: "id".to_owned(),
            cases: vec![case("a", "ok")],
        };
        let eval = super::evaluate_workflow(
            Some(corpus.clone()),
            &BTreeMap::from([("a".to_owned(), "ok".to_owned())]),
            Some(corpus),
            &BTreeMap::from([("a".to_owned(), "ok".to_owned())]),
            "unknown",
        );
        match eval {
            super::WorkflowEvaluation::Invalid { reason } => {
                assert!(reason.contains("Unknown escalation"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn workflow_invalid_on_duplicate_corpus() {
        let bad = EvaluationCorpus {
            id: "bad".to_owned(),
            cases: vec![case("dup", "ok"), case("dup", "ok")],
        };
        let good = EvaluationCorpus {
            id: "good".to_owned(),
            cases: vec![case("a", "ok")],
        };
        let eval = super::evaluate_workflow(
            Some(bad),
            &BTreeMap::new(),
            Some(good),
            &BTreeMap::new(),
            "skill",
        );
        match eval {
            super::WorkflowEvaluation::Invalid { reason } => {
                assert!(reason.contains("duplicate"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn proposal_skill_is_valid() {
        let proposal = super::Proposal {
            id: "skill-1".to_owned(),
            workflow_digest: "a".repeat(64),
            kind: super::Escalation::Skill,
            description: "Add skill for workflow".to_owned(),
            requires_host_approval: false,
        };
        let eval = super::evaluate_proposal(Some(proposal));
        match eval {
            super::ProposalEvaluation::Valid { evidence } => {
                assert_eq!(evidence.kind, super::Escalation::Skill);
                assert!(!evidence.requires_host_approval);
                assert_eq!(evidence.proposal_digest.len(), 64);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn proposal_host_requires_approval() {
        let proposal = super::Proposal {
            id: "host-1".to_owned(),
            workflow_digest: "b".repeat(64),
            kind: super::Escalation::Host,
            description: "Host change proposal".to_owned(),
            requires_host_approval: true,
        };
        let eval = super::evaluate_proposal(Some(proposal));
        assert!(matches!(eval, super::ProposalEvaluation::Valid { .. }));
        let bad = super::Proposal {
            id: "host-bad".to_owned(),
            workflow_digest: "c".repeat(64),
            kind: super::Escalation::Host,
            description: "Host without approval".to_owned(),
            requires_host_approval: false,
        };
        let eval = super::evaluate_proposal(Some(bad));
        match eval {
            super::ProposalEvaluation::Invalid { reason } => {
                assert!(reason.contains("Host proposal must require"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn proposal_invalid_kind_rejected() {
        let proposal = super::Proposal {
            id: "bad-kind".to_owned(),
            workflow_digest: "d".repeat(64),
            kind: super::Escalation::Profile,
            description: "Profile proposal should be invalid".to_owned(),
            requires_host_approval: false,
        };
        let eval = super::evaluate_proposal(Some(proposal));
        match eval {
            super::ProposalEvaluation::Invalid { reason } => {
                assert!(reason.contains("skill, plugin, or host"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn proposal_absent_is_invalid() {
        let eval = super::evaluate_proposal(None);
        match eval {
            super::ProposalEvaluation::Invalid { reason } => {
                assert!(reason.contains("A proposal is required"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn release_compatible_is_valid() {
        let release = super::Release {
            id: "rel-1".to_owned(),
            version: "1.2.3".to_owned(),
            previous_version: "1.2.2".to_owned(),
            compatibility: super::Compatibility::Compatible,
        };
        let eval = super::evaluate_release(Some(release));
        match eval {
            super::ReleaseEvaluation::Valid { evidence } => {
                assert_eq!(
                    evidence.compatibility,
                    super::Compatibility::Compatible
                );
                assert_eq!(evidence.release_digest.len(), 64);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn release_breaking_is_valid() {
        let release = super::Release {
            id: "rel-break".to_owned(),
            version: "2.0.0".to_owned(),
            previous_version: "1.9.9".to_owned(),
            compatibility: super::Compatibility::Breaking,
        };
        let eval = super::evaluate_release(Some(release));
        assert!(matches!(eval, super::ReleaseEvaluation::Valid { .. }));
    }

    #[test]
    fn release_invalid_version_rejected() {
        let release = super::Release {
            id: "bad".to_owned(),
            version: "not-semver".to_owned(),
            previous_version: "1.0.0".to_owned(),
            compatibility: super::Compatibility::Patch,
        };
        let eval = super::evaluate_release(Some(release));
        match eval {
            super::ReleaseEvaluation::Invalid { reason } => {
                assert!(reason.contains("MAJOR.MINOR.PATCH"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn release_absent_is_invalid() {
        let eval = super::evaluate_release(None);
        match eval {
            super::ReleaseEvaluation::Invalid { reason } => {
                assert!(reason.contains("A release is required"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
