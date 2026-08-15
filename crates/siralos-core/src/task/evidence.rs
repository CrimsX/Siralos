//! Evidence validation and successful-outcome cross-check (R3).
//!
//! Permanent rule: model claims do not become evidence merely because the
//! model says they are true. Evidence must be Host-observed or
//! Host-accepted structured data, bounded, typed, task-scoped, bound to
//! the exact current contract revision/digest where acceptance depends on
//! it, and free of secrets or private model reasoning.
//!
//! A claimed 'passed' verification is not trusted on its own: the
//! structured source must independently support a successful outcome.

use std::collections::BTreeSet;

use crate::identity::{CanonicalValue, canonicalize};
use crate::task::model::{
    ApprovalDecision, EvidenceKind, EvidenceSource, EvidenceVerification,
    VerificationOutcome, limits,
};

/// Whether a bounded evidence source independently represents a successful
/// observation. Acceptance policy must never trust a claimed verification
/// outcome without checking the underlying structured result.
pub fn source_supports_successful_outcome(
    kind: EvidenceKind,
    source: &EvidenceSource,
) -> bool {
    match source {
        EvidenceSource::WorkspaceRead { paths, .. } => {
            kind == EvidenceKind::WorkspaceRead && !paths.is_empty()
        }
        EvidenceSource::Parser { checked_files, valid_files, errors } => {
            kind == EvidenceKind::ParserResult
                && *checked_files > 0
                && *errors == 0
                && valid_files == checked_files
        }
        EvidenceSource::Validation {
            workspace_integrity_verified,
            unexpected_changes,
            ..
        } => {
            kind == EvidenceKind::ValidationResult
                && *workspace_integrity_verified
                && *unexpected_changes == 0
        }
        EvidenceSource::Review { status, blocking_findings } => {
            kind == EvidenceKind::ReviewResult
                && status == "clean"
                && *blocking_findings == 0
        }
        EvidenceSource::UserApproval { decision, .. } => {
            kind == EvidenceKind::UserApproval
                && *decision == ApprovalDecision::Approved
        }
    }
}

/// Exact reference binding-id pattern for verification check and
/// criterion ids.
fn binding_id_pattern_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    if !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    bytes[1..].iter().all(|byte| {
        byte.is_ascii_alphanumeric()
            || *byte == b'.'
            || *byte == b'_'
            || *byte == b'-'
    })
}

/// Validated evidence payload accepted by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedEvidence {
    /// Bounded source (detached by value).
    /// Source.
    pub source: EvidenceSource,
    /// Verification binding, when supplied.
    /// Verification.
    pub verification: Option<EvidenceVerification>,
}

/// Typed rejection of an evidence payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceError {
    /// The evidence id is empty after trimming.
    /// Empty id.
    EmptyId,
    /// The evidence id exceeds the byte bound.
    /// Id too large.
    IdTooLarge,
    /// The evidence kind is unknown.
    /// Unknown kind.
    UnknownKind,
    /// The source type does not match the evidence kind.
    /// Source kind mismatch.
    SourceKindMismatch,
    /// The verification binding is invalid.
    /// Invalid verification.
    InvalidVerification,
    /// The serialized source exceeds the byte bound.
    /// Source too large.
    SourceTooLarge,
    /// A passed verification requires a source that supports success.
    /// Passed outcome without successful source.
    PassedOutcomeWithoutSuccessfulSource,
    /// The source is not finite serializable data.
    /// Non finite source.
    NonFiniteSource,
}

impl EvidenceError {
    /// Stable machine-branchable code (differential observation
    /// vocabulary shared with the TypeScript oracle adapter).
    pub fn code(&self) -> &'static str {
        match self {
            EvidenceError::EmptyId => "empty_evidence_id",
            EvidenceError::IdTooLarge => "evidence_id_too_large",
            EvidenceError::UnknownKind => "unknown_evidence_kind",
            EvidenceError::SourceKindMismatch => {
                "evidence_source_kind_mismatch"
            }
            EvidenceError::InvalidVerification => {
                "invalid_evidence_verification"
            }
            EvidenceError::SourceTooLarge => "evidence_source_too_large",
            EvidenceError::PassedOutcomeWithoutSuccessfulSource => {
                "passed_verification_without_successful_source"
            }
            EvidenceError::NonFiniteSource => "non_finite_evidence_source",
        }
    }
}

/// Canonical serialized bytes of a source; used for the byte bound.
/// Key order never affects the total byte length, so the canonical form
/// matches the reference's plain JSON.stringify length.
pub fn source_canonical_bytes(source: &EvidenceSource) -> String {
    let value = source_to_canonical(source);
    canonicalize(&value)
}

fn source_to_canonical(source: &EvidenceSource) -> CanonicalValue {
    use std::collections::BTreeMap;
    let mut object = BTreeMap::new();
    match source {
        EvidenceSource::WorkspaceRead { paths, revision } => {
            object.insert(
                "type".to_owned(),
                CanonicalValue::Str("workspace_read".to_owned()),
            );
            object.insert(
                "paths".to_owned(),
                CanonicalValue::Array(
                    paths
                        .iter()
                        .map(|path| CanonicalValue::Str(path.clone()))
                        .collect(),
                ),
            );
            if let Some(revision) = revision {
                object.insert(
                    "revision".to_owned(),
                    CanonicalValue::Str(revision.clone()),
                );
            }
        }
        EvidenceSource::Parser { checked_files, valid_files, errors } => {
            object.insert(
                "type".to_owned(),
                CanonicalValue::Str("parser".to_owned()),
            );
            object.insert(
                "checkedFiles".to_owned(),
                CanonicalValue::U64(*checked_files),
            );
            object.insert(
                "validFiles".to_owned(),
                CanonicalValue::U64(*valid_files),
            );
            object.insert("errors".to_owned(), CanonicalValue::U64(*errors));
        }
        EvidenceSource::Validation {
            outcome,
            workspace_integrity_verified,
            unexpected_changes,
        } => {
            object.insert(
                "type".to_owned(),
                CanonicalValue::Str("validation".to_owned()),
            );
            object.insert(
                "outcome".to_owned(),
                CanonicalValue::Str(outcome.clone()),
            );
            object.insert(
                "workspaceIntegrityVerified".to_owned(),
                CanonicalValue::Bool(*workspace_integrity_verified),
            );
            object.insert(
                "unexpectedChanges".to_owned(),
                CanonicalValue::U64(*unexpected_changes),
            );
        }
        EvidenceSource::Review { status, blocking_findings } => {
            object.insert(
                "type".to_owned(),
                CanonicalValue::Str("review".to_owned()),
            );
            object.insert(
                "status".to_owned(),
                CanonicalValue::Str(status.clone()),
            );
            object.insert(
                "blockingFindings".to_owned(),
                CanonicalValue::U64(*blocking_findings),
            );
        }
        EvidenceSource::UserApproval { approval_id, subject_id, decision } => {
            object.insert(
                "type".to_owned(),
                CanonicalValue::Str("user_approval".to_owned()),
            );
            object.insert(
                "approvalId".to_owned(),
                CanonicalValue::Str(approval_id.clone()),
            );
            object.insert(
                "subjectId".to_owned(),
                CanonicalValue::Str(subject_id.clone()),
            );
            object.insert(
                "decision".to_owned(),
                CanonicalValue::Str(
                    match decision {
                        ApprovalDecision::Approved => "approved",
                        ApprovalDecision::Denied => "denied",
                    }
                    .to_owned(),
                ),
            );
        }
    }
    CanonicalValue::Object(object)
}

/// Validate an evidence payload: id, kind/source compatibility, the
/// verification binding, the serialized source byte bound, and the
/// successful-outcome cross-check.
pub fn validate_evidence_payload(
    id: &str,
    kind: EvidenceKind,
    source: EvidenceSource,
    verification: Option<EvidenceVerification>,
) -> Result<ValidatedEvidence, EvidenceError> {
    if id.trim().is_empty() {
        return Err(EvidenceError::EmptyId);
    }
    if id.len() > limits::MAX_TASK_EVIDENCE_ID_BYTES {
        return Err(EvidenceError::IdTooLarge);
    }
    if !source_kind_matches(kind, &source) {
        return Err(EvidenceError::SourceKindMismatch);
    }
    if let Some(verification) = &verification {
        validate_verification(verification)?;
    }
    let serialized = source_canonical_bytes(&source);
    if serialized.len() > limits::MAX_EVIDENCE_SOURCE_BYTES {
        return Err(EvidenceError::SourceTooLarge);
    }
    if verification
        .as_ref()
        .is_some_and(|entry| entry.outcome == VerificationOutcome::Passed)
        && !source_supports_successful_outcome(kind, &source)
    {
        return Err(EvidenceError::PassedOutcomeWithoutSuccessfulSource);
    }
    Ok(ValidatedEvidence { source, verification })
}

fn source_kind_matches(kind: EvidenceKind, source: &EvidenceSource) -> bool {
    matches!(
        (kind, source),
        (EvidenceKind::WorkspaceRead, EvidenceSource::WorkspaceRead { .. })
            | (EvidenceKind::ParserResult, EvidenceSource::Parser { .. })
            | (
                EvidenceKind::ValidationResult,
                EvidenceSource::Validation { .. }
            )
            | (EvidenceKind::ReviewResult, EvidenceSource::Review { .. })
            | (
                EvidenceKind::UserApproval,
                EvidenceSource::UserApproval { .. }
            )
    )
}

/// Validate the verification binding: check id and criterion id follow
/// the binding pattern and the outcome is one of the typed values.
pub fn validate_verification(
    verification: &EvidenceVerification,
) -> Result<(), EvidenceError> {
    if !binding_id_pattern_ok(&verification.check_id) {
        return Err(EvidenceError::InvalidVerification);
    }
    if let Some(criterion_id) = &verification.criterion_id {
        if !binding_id_pattern_ok(criterion_id) {
            return Err(EvidenceError::InvalidVerification);
        }
    }
    // A verification without a criterion binding cannot satisfy
    // acceptance; the milestone-requirement binding arrives with its own
    // milestone.
    if verification.criterion_id.is_none() {
        return Err(EvidenceError::InvalidVerification);
    }
    Ok(())
}

/// Validate and detach step findings (bounded, non-empty, unique ids,
/// valid severities).
pub fn validate_findings(
    findings: Vec<FindingInput>,
) -> Result<Vec<crate::task::model::FindingRef>, FindingError> {
    if findings.len() > limits::MAX_TASK_FINDINGS {
        return Err(FindingError::TooMany);
    }
    let mut ids = BTreeSet::new();
    let mut validated = Vec::with_capacity(findings.len());
    for finding in findings {
        let finding_id = finding.finding_id;
        let source = finding.source;
        if finding_id.trim().is_empty() || source.trim().is_empty() {
            return Err(FindingError::EmptyField);
        }
        if finding_id.len() > limits::MAX_TASK_FINDING_FIELD_BYTES
            || source.len() > limits::MAX_TASK_FINDING_FIELD_BYTES
        {
            return Err(FindingError::FieldTooLarge);
        }
        if !ids.insert(finding_id.clone()) {
            return Err(FindingError::DuplicateId);
        }
        validated.push(crate::task::model::FindingRef {
            finding_id,
            severity: finding.severity,
            source,
        });
    }
    Ok(validated)
}

/// Input for a host-observed finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingInput {
    /// Finding id.
    /// Finding id.
    pub finding_id: String,
    /// Severity.
    /// Severity.
    pub severity: crate::task::model::FindingSeverity,
    /// Source label.
    /// Source.
    pub source: String,
}

/// Typed rejection of a findings replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingError {
    /// More than the maximum findings were supplied.
    /// Too many.
    TooMany,
    /// A finding id or source is empty.
    /// Empty field.
    EmptyField,
    /// A finding field exceeds the byte bound.
    /// Field too large.
    FieldTooLarge,
    /// A finding id is duplicated.
    /// Duplicate id.
    DuplicateId,
}

impl FindingError {
    /// Stable machine-branchable code.
    pub fn code(&self) -> &'static str {
        match self {
            FindingError::TooMany => "too_many_findings",
            FindingError::EmptyField => "empty_finding_field",
            FindingError::FieldTooLarge => "finding_field_too_large",
            FindingError::DuplicateId => "duplicate_finding_id",
        }
    }
}

/// Normalize an iteration input exactly like the reference: undefined or
/// non-finite becomes 0; otherwise max(0, floor(value)).
pub fn normalize_iteration(iteration: Option<f64>) -> u64 {
    match iteration {
        Some(value) if value.is_finite() && value >= 0.0 => {
            value.floor() as u64
        }
        _ => 0,
    }
}
