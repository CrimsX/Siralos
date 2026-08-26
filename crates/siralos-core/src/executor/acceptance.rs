//! Host-owned milestone acceptance evaluation (executor briefing
//! foundation, ADR 0022).
//!
//! A small deterministic evaluator mapping milestone acceptance
//! requirements to HOST-OBSERVED evidence: the task's attached evidence
//! records and the host-verified acceptance states of its contract.
//! There is no path for an executor claim to enter. It supports exactly
//! the four requirement statuses below.

use crate::task::{
    AcceptanceState, EvidenceKind, EvidenceRecord, VerificationOutcome,
};

use super::milestone::{
    AcceptanceRequirement, MilestoneManifest,
    resolve_acceptance_evidence_kinds,
};

/// Status of one acceptance requirement evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptanceRequirementStatus {
    /// Satisfied by valid host-observed evidence.
    Pass,
    /// Invalid or failed evidence targeted the requirement.
    Fail,
    /// Not yet satisfied.
    Incomplete,
    /// Optional requirement with no applicable target.
    NotApplicable,
}

impl AcceptanceRequirementStatus {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            AcceptanceRequirementStatus::Pass => "pass",
            AcceptanceRequirementStatus::Fail => "fail",
            AcceptanceRequirementStatus::Incomplete => "incomplete",
            AcceptanceRequirementStatus::NotApplicable => "not_applicable",
        }
    }
}

/// One requirement result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneRequirementResult {
    /// Requirement id.
    pub id: String,
    /// Evaluation status.
    pub status: AcceptanceRequirementStatus,
    /// Evidence ids that satisfied the requirement, when any.
    pub satisfied_by: Vec<String>,
    /// Exact note when not passed.
    pub note: Option<String>,
}

/// Status counts over a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneAcceptanceCounts {
    /// Passed count.
    pub pass: usize,
    /// Failed count.
    pub fail: usize,
    /// Incomplete count.
    pub incomplete: usize,
    /// Not-applicable count.
    pub not_applicable: usize,
    /// Total requirements.
    pub total: usize,
}

/// Full acceptance report for one manifest version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneAcceptanceReport {
    /// Evaluated manifest id.
    pub manifest_id: String,
    /// Evaluated manifest version.
    pub manifest_version: u64,
    /// Per-requirement results.
    pub requirements: Vec<MilestoneRequirementResult>,
    /// Status counts.
    pub counts: MilestoneAcceptanceCounts,
    /// True only when every non-optional requirement passes or is
    /// not-applicable; a single incomplete keeps the milestone open.
    pub passed: bool,
}

/// Exact authoritative task identity being evaluated.
#[derive(Clone, Copy)]
pub struct AcceptanceTaskIdentity<'a> {
    /// Task id.
    pub task_id: &'a str,
    /// Contract revision.
    pub contract_revision: u64,
    /// Contract content digest.
    pub contract_digest: Option<&'a str>,
}

/// Evaluation input.
pub struct AcceptanceEvaluationInput<'a> {
    /// The milestone manifest.
    pub manifest: &'a MilestoneManifest,
    /// The exact task identity.
    pub task: AcceptanceTaskIdentity<'a>,
    /// Host-attached evidence records (never executor claims).
    pub evidence: &'a [EvidenceRecord],
    /// Host-verified acceptance states of the task's contract.
    pub acceptance: &'a [AcceptanceState],
}

fn is_current_task_evidence(
    record: &EvidenceRecord,
    task: &AcceptanceTaskIdentity<'_>,
) -> bool {
    match task.contract_digest {
        Some(digest) => {
            record.task_id == task.task_id
                && record.task_contract_revision == task.contract_revision
                && record.task_contract_digest == digest
        }
        None => false,
    }
}

fn kind_can_verify_criterion(
    kind: EvidenceKind,
    criterion: &AcceptanceState,
) -> bool {
    use crate::task::VerificationKind;
    match criterion.verification_kind {
        VerificationKind::User => kind == EvidenceKind::UserApproval,
        VerificationKind::Review => kind == EvidenceKind::ReviewResult,
        VerificationKind::Deterministic => !matches!(
            kind,
            EvidenceKind::ReviewResult | EvidenceKind::UserApproval
        ),
    }
}

fn valid_successful_record(record: &EvidenceRecord) -> bool {
    let Some(verification) = &record.verification else {
        return false;
    };
    verification.outcome == VerificationOutcome::Passed
        && crate::task::source_supports_successful_outcome(
            record.kind,
            &record.source,
        )
}

fn evaluate_requirement(
    manifest: &MilestoneManifest,
    requirement: &AcceptanceRequirement,
    task: &AcceptanceTaskIdentity<'_>,
    evidence: &[EvidenceRecord],
    acceptance: &[AcceptanceState],
) -> MilestoneRequirementResult {
    let evidence_kinds = resolve_acceptance_evidence_kinds(
        &requirement.evidence_kinds,
        &requirement.standard_ids,
    );

    if let Some(criterion_id) = &requirement.criterion_id {
        let criterion = acceptance
            .iter()
            .find(|entry| &entry.criterion_id == criterion_id);
        let Some(criterion) = criterion else {
            let note =
                "No linked task criterion exists for this task.".to_owned();
            let status = if requirement.optional {
                AcceptanceRequirementStatus::NotApplicable
            } else {
                AcceptanceRequirementStatus::Incomplete
            };
            return MilestoneRequirementResult {
                id: requirement.id.clone(),
                status,
                satisfied_by: Vec::new(),
                note: Some(note),
            };
        };
        if criterion.status == crate::task::AcceptanceStatus::Satisfied
            && criterion.verified_by.is_some()
        {
            let Some(verified_by) = criterion.verified_by.as_deref() else {
                unreachable!("checked some");
            };
            let matching: Vec<&EvidenceRecord> = evidence
                .iter()
                .filter(|entry| entry.id == *verified_by)
                .collect();
            let evaluation = match matching.as_slice() {
                [record] => {
                    let verification_ok = record
                        .verification
                        .as_ref()
                        .is_some_and(|verification| {
                            verification.criterion_id.as_deref()
                                == Some(criterion_id.as_str())
                                && verification.check_id
                                    == requirement.check_id
                        });
                    let task_bound = is_current_task_evidence(record, task);
                    let kind_ok =
                        kind_can_verify_criterion(record.kind, criterion)
                            && (evidence_kinds.is_empty()
                                || evidence_kinds.contains(&record.kind));
                    verification_ok
                        && task_bound
                        && kind_ok
                        && valid_successful_record(record)
                }
                _ => false,
            };
            return if evaluation {
                MilestoneRequirementResult {
                    id: requirement.id.clone(),
                    status: AcceptanceRequirementStatus::Pass,
                    satisfied_by: vec![matching[0].id.clone()],
                    note: None,
                }
            } else {
                MilestoneRequirementResult {
                    id: requirement.id.clone(),
                    status: AcceptanceRequirementStatus::Fail,
                    satisfied_by: Vec::new(),
                    note: Some(format!(
                        "Linked criterion {criterion_id} has invalid or stale verification evidence."
                    )),
                }
            };
        }
        return if criterion.status == crate::task::AcceptanceStatus::Failed {
            MilestoneRequirementResult {
                id: requirement.id.clone(),
                status: AcceptanceRequirementStatus::Fail,
                satisfied_by: Vec::new(),
                note: Some(format!("Linked criterion {criterion_id} failed.")),
            }
        } else {
            MilestoneRequirementResult {
                id: requirement.id.clone(),
                status: AcceptanceRequirementStatus::Incomplete,
                satisfied_by: Vec::new(),
                note: Some(format!(
                    "Linked criterion {criterion_id} is not host-verified."
                )),
            }
        };
    }

    // A kind is only a whitelist. Direct milestone evidence must
    // additionally target this immutable manifest requirement/check and
    // the current task contract, and its structured source must
    // independently show success.
    let targeted: Vec<&EvidenceRecord> = evidence
        .iter()
        .filter(|record| {
            is_current_task_evidence(record, task)
                && record.verification.as_ref().is_some_and(|verification| {
                    verification.milestone.as_ref().is_some_and(|milestone| {
                        milestone.manifest_id == manifest.id
                            && milestone.manifest_version == manifest.version
                            && milestone.requirement_id == requirement.id
                    })
                })
        })
        .collect();
    let passing: Vec<&EvidenceRecord> = targeted
        .iter()
        .copied()
        .filter(|record| {
            record.verification.as_ref().is_some_and(|verification| {
                verification.check_id == requirement.check_id
            }) && evidence_kinds.contains(&record.kind)
                && valid_successful_record(record)
        })
        .collect();
    if !passing.is_empty() {
        let mut ids: Vec<String> =
            passing.iter().map(|record| record.id.clone()).collect();
        ids.sort();
        return MilestoneRequirementResult {
            id: requirement.id.clone(),
            status: AcceptanceRequirementStatus::Pass,
            satisfied_by: ids,
            note: None,
        };
    }
    if !targeted.is_empty() {
        let any_failed = targeted.iter().any(|record| {
            record.verification.as_ref().is_some_and(|verification| {
                verification.outcome == VerificationOutcome::Failed
            })
        });
        return MilestoneRequirementResult {
            id: requirement.id.clone(),
            status: if any_failed {
                AcceptanceRequirementStatus::Fail
            } else {
                AcceptanceRequirementStatus::Incomplete
            },
            satisfied_by: Vec::new(),
            note: Some(
                "Targeted host evidence did not contain a matching successful check outcome."
                    .to_owned(),
            ),
        };
    }
    if requirement.optional {
        return MilestoneRequirementResult {
            id: requirement.id.clone(),
            status: AcceptanceRequirementStatus::NotApplicable,
            satisfied_by: Vec::new(),
            note: Some(
                "No matching host evidence; optional requirement not applicable.".to_owned(),
            ),
        };
    }
    MilestoneRequirementResult {
        id: requirement.id.clone(),
        status: AcceptanceRequirementStatus::Incomplete,
        satisfied_by: Vec::new(),
        note: Some("No matching host-attached evidence.".to_owned()),
    }
}

/// Host-owned milestone acceptance evaluator.
#[derive(Debug, Clone, Copy, Default)]
pub struct AcceptanceEvaluator;

impl AcceptanceEvaluator {
    /// Evaluate one manifest against host-observed evidence.
    pub fn evaluate(
        &self,
        input: &AcceptanceEvaluationInput<'_>,
    ) -> MilestoneAcceptanceReport {
        let mut requirements = Vec::new();
        for requirement in &input.manifest.acceptance {
            requirements.push(evaluate_requirement(
                input.manifest,
                requirement,
                &input.task,
                input.evidence,
                input.acceptance,
            ));
        }
        let mut counts = MilestoneAcceptanceCounts {
            pass: 0,
            fail: 0,
            incomplete: 0,
            not_applicable: 0,
            total: requirements.len(),
        };
        for result in &requirements {
            match result.status {
                AcceptanceRequirementStatus::Pass => counts.pass += 1,
                AcceptanceRequirementStatus::Fail => counts.fail += 1,
                AcceptanceRequirementStatus::Incomplete => {
                    counts.incomplete += 1
                }
                AcceptanceRequirementStatus::NotApplicable => {
                    counts.not_applicable += 1
                }
            }
        }
        let passed =
            counts.fail == 0 && counts.incomplete == 0 && counts.total > 0;
        MilestoneAcceptanceReport {
            manifest_id: input.manifest.id.clone(),
            manifest_version: input.manifest.version,
            requirements,
            counts,
            passed,
        }
    }
}
