//! R3 task kernel behavior tests (positive and negative invariants).
//!
//! Mirrors the observable semantics pinned by the TypeScript reference
//! tests (task-contract.test.ts, task-runtime-invariants.test.ts) without
//! copying their structure.

use crate::task::contract::{
    AcceptanceCriterion, CreateTaskContractInput, TaskContract,
    VerificationKind,
};
use crate::task::evidence::{EvidenceError, FindingError, FindingInput};
use crate::task::model::{
    AcceptanceStatus, ActivityEvent, ApprovalDecision, DispositionSource,
    EvidenceKind, EvidenceSource, EvidenceVerification, FindingSeverity,
    ProgressStateValue, TaskPhase, TaskReviewStatus, TaskStepKind,
    TaskStepSpec, TaskStepStatus, TaskValidationStatus, VerificationOutcome,
    WorkflowDisposition,
};
use crate::task::progress::HostObservation;
use crate::task::runtime::{
    AttachRejection, AttachResult, CompletionResult, CriterionResult, OpError,
    StepOpResult, TaskCreateError, TaskHandle, TaskRuntime,
};

const NOW: i64 = 1_700_000_000_000;

fn criterion(id: &str, kind: VerificationKind) -> AcceptanceCriterion {
    AcceptanceCriterion {
        id: id.to_owned(),
        description: format!("criterion {id}"),
        verification_kind: kind,
    }
}

fn contract(id: &str, criteria: Vec<AcceptanceCriterion>) -> TaskContract {
    TaskContract::create(CreateTaskContractInput {
        id: id.to_owned(),
        request: format!("Request for {id}"),
        context: None,
        constraints: None,
        acceptance_criteria: criteria,
        pause_policy: None,
    })
    .expect("valid contract")
}

fn step(id: &str, accepts: &[EvidenceKind]) -> TaskStepSpec {
    TaskStepSpec {
        id: id.to_owned(),
        description: format!("Step {id}"),
        kind: TaskStepKind::Implementation,
        accepts: accepts.to_vec(),
    }
}

fn workspace_read_evidence(_id: &str, paths: &[&str]) -> EvidenceSource {
    EvidenceSource::WorkspaceRead {
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        revision: None,
    }
}

fn runtime() -> TaskRuntime {
    TaskRuntime::with_clock(|| NOW)
}

fn create<'a>(runtime: &'a mut TaskRuntime, id: &str) -> TaskHandle<'a> {
    let task_id = runtime
        .create_task(crate::task::runtime::CreateTaskInput {
            contract: contract(
                id,
                vec![criterion("c1", VerificationKind::Deterministic)],
            ),
            steps: vec![step("s1", &[EvidenceKind::WorkspaceRead])],
            iteration: None,
        })
        .expect("task created");
    runtime.task(&task_id).expect("task handle")
}

fn verify_verification(criterion_id: &str) -> EvidenceVerification {
    EvidenceVerification {
        check_id: criterion_id.to_owned(),
        criterion_id: Some(criterion_id.to_owned()),
        outcome: VerificationOutcome::Passed,
        milestone: None,
    }
}

/// Complete a task through the full host gate.
fn complete_task(handle: &mut TaskHandle<'_>) {
    assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
    assert!(matches!(
        handle.attach_evidence(
            "evidence-1",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("evidence-1", &["project.main"]),
            Some(verify_verification("c1")),
        ),
        AttachResult::Attached
    ));
    if !handle.snapshot().steps.is_empty() {
        assert_eq!(handle.begin_step("s1"), StepOpResult::Ok);
        assert_eq!(
            handle.complete_step(
                "s1",
                &[crate::task::model::EvidenceRef {
                    evidence_id: "evidence-1".to_owned(),
                    kind: EvidenceKind::WorkspaceRead,
                }]
            ),
            StepOpResult::Ok
        );
    }
    assert_eq!(
        handle.verify_criterion("c1", Some("evidence-1"), None),
        CriterionResult::Verified
    );
    handle.set_validation_status(TaskValidationStatus::Clean);
    handle.set_review_status(TaskReviewStatus::Clean);
    assert_eq!(handle.complete_task(), CompletionResult::Completed);
}

#[test]
fn initial_state_is_prepared_with_pending_steps_and_acceptance() {
    let mut runtime = runtime();
    let handle = create(&mut runtime, "task-initial");
    let state = handle.snapshot();
    assert_eq!(state.phase, TaskPhase::Prepared);
    assert_eq!(state.contract_revision, 1);
    assert_eq!(state.steps[0].status, TaskStepStatus::Pending);
    assert_eq!(state.acceptance[0].status, AcceptanceStatus::Pending);
    assert_eq!(state.validation_status, TaskValidationStatus::NotRun);
    assert_eq!(state.review_status, TaskReviewStatus::NotRun);
    assert_eq!(state.started_at_ms, NOW);
    assert_eq!(state.completed_at_ms, None);
    assert_eq!(state.terminal_reason, None);
    assert_eq!(state.evidence.len(), 0);
    assert_eq!(handle.activity_log().len(), 1);
    assert_eq!(handle.activity_log()[0].type_str(), "task_started");
    assert_eq!(handle.activity_log()[0].sequence(), 1);
}

#[test]
fn allowed_and_rejected_phase_transitions_match_the_reference_table() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-transitions");
    assert_eq!(
        handle.transition_phase(TaskPhase::Prepared),
        StepOpResult::Rejected(OpError::AlreadyPhase)
    );
    assert_eq!(
        handle.transition_phase(TaskPhase::Completed),
        StepOpResult::Rejected(OpError::InvalidTransition)
    );
    assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
    assert_eq!(
        handle.transition_phase(TaskPhase::Validating),
        StepOpResult::Ok
    );
    assert_eq!(
        handle.transition_phase(TaskPhase::Reviewing),
        StepOpResult::Ok
    );
    assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
    assert_eq!(handle.transition_phase(TaskPhase::Blocked), StepOpResult::Ok);
    assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
    assert_eq!(
        handle.transition_phase(TaskPhase::Cancelled),
        StepOpResult::Ok
    );
    let state = handle.snapshot();
    assert_eq!(state.phase, TaskPhase::Cancelled);
    assert!(state.completed_at_ms.is_some());
    // Terminal states have no outgoing transitions.
    assert_eq!(
        handle.transition_phase(TaskPhase::Working),
        StepOpResult::Rejected(OpError::InvalidTransition)
    );
}

#[test]
fn terminal_state_rejects_every_ordinary_mutation() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-terminal");
    complete_task(&mut handle);
    let before = handle.snapshot();
    let activity_before = handle.activity_log().to_vec();
    let progress_before = handle.progress();

    assert_eq!(
        handle.begin_step("s1"),
        StepOpResult::Rejected(OpError::Terminal)
    );
    assert!(matches!(
        handle.attach_evidence(
            "late",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("late", &["late.gd"]),
            None,
        ),
        AttachResult::Rejected(AttachRejection::Terminal)
    ));
    assert_eq!(
        handle.mark_criterion_failed("c1", None),
        CriterionResult::Rejected(OpError::Terminal)
    );
    assert_eq!(
        handle.complete_task(),
        CompletionResult::Rejected {
            reasons: vec![
                "The task is terminal (completed); authoritative state can no longer be changed."
                    .to_owned()
            ]
        }
    );
    let disposition = handle.submit_disposition(
        WorkflowDisposition::Continue { next_action: Some("late".to_owned()) },
        DispositionSource::Host,
    );
    assert!(!disposition.accepted);
    assert_eq!(disposition.code, Some(OpError::Terminal));
    assert!(
        handle
            .revise_contract(crate::task::contract::ReviseTaskContractInput {
                id: "task-terminal".to_owned(),
                request: Some("late revision".to_owned()),
                context: None,
                constraints: None,
                acceptance_criteria: None,
                pause_policy: None,
            })
            .is_err()
    );

    handle.set_validation_status(TaskValidationStatus::Failed);
    handle.set_review_status(TaskReviewStatus::Findings);
    handle.set_iteration(99);
    assert!(
        handle
            .set_findings(vec![FindingInput {
                finding_id: "late".to_owned(),
                severity: FindingSeverity::High,
                source: "must-not-be-retained".to_owned(),
            }])
            .is_ok()
    );
    handle.observe(HostObservation {
        action: "late".to_owned(),
        fingerprint: "late".to_owned(),
        progress: true,
    });

    assert_eq!(handle.snapshot(), before);
    assert_eq!(handle.activity_log(), activity_before.as_slice());
    assert_eq!(handle.progress(), progress_before);
}

#[test]
fn duplicate_task_ids_never_replace_authoritative_history() {
    let mut runtime = runtime();
    let input = crate::task::runtime::CreateTaskInput {
        contract: contract(
            "task-duplicate",
            vec![criterion("c1", VerificationKind::Deterministic)],
        ),
        steps: Vec::new(),
        iteration: None,
    };
    runtime.create_task(input).expect("first task");
    let duplicate = crate::task::runtime::CreateTaskInput {
        contract: contract(
            "task-duplicate",
            vec![criterion("c1", VerificationKind::Deterministic)],
        ),
        steps: Vec::new(),
        iteration: None,
    };
    assert_eq!(
        runtime.create_task(duplicate).expect_err("duplicate"),
        TaskCreateError::DuplicateTask
    );
    assert_eq!(runtime.list_task_ids(), vec!["task-duplicate".to_owned()]);
    assert_eq!(runtime.len(), 1);
}

#[test]
fn tasks_list_in_creation_order_and_latest_is_last() {
    let mut runtime = runtime();
    for id in ["task-a", "task-b", "task-c"] {
        runtime
            .create_task(crate::task::runtime::CreateTaskInput {
                contract: contract(
                    id,
                    vec![criterion("c1", VerificationKind::Deterministic)],
                ),
                steps: Vec::new(),
                iteration: None,
            })
            .expect("created");
    }
    assert_eq!(
        runtime.list_task_ids(),
        vec!["task-a".to_owned(), "task-b".to_owned(), "task-c".to_owned()]
    );
    assert_eq!(runtime.latest_task_id().as_deref(), Some("task-c"));
}

#[test]
fn step_operations_validate_ids_status_and_evidence() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-steps");
    assert_eq!(
        handle.begin_step("unknown"),
        StepOpResult::Rejected(OpError::UnknownStep)
    );
    assert_eq!(handle.begin_step("s1"), StepOpResult::Ok);
    assert_eq!(
        handle.begin_step("s1"),
        StepOpResult::Rejected(OpError::StepAlreadyActive)
    );
    assert_eq!(
        handle.complete_step("s1", &[]),
        StepOpResult::Rejected(OpError::StepRequiresEvidence)
    );
    assert_eq!(
        handle.complete_step(
            "s1",
            &[crate::task::model::EvidenceRef {
                evidence_id: "missing".to_owned(),
                kind: EvidenceKind::WorkspaceRead,
            }]
        ),
        StepOpResult::Rejected(OpError::UnknownEvidenceRef)
    );
    assert!(matches!(
        handle.attach_evidence(
            "e1",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("e1", &["a.gd"]),
            None,
        ),
        AttachResult::Attached
    ));
    // A step that does not accept the kind is rejected.
    assert_eq!(
        handle.complete_step(
            "s1",
            &[crate::task::model::EvidenceRef {
                evidence_id: "e1".to_owned(),
                kind: EvidenceKind::ReviewResult,
            }]
        ),
        StepOpResult::Rejected(OpError::StepRejectsEvidenceKind)
    );
    // A kind the step does not accept is rejected before the record
    // kind comparison (reference ordering: accepts check first).
    assert_eq!(
        handle.complete_step(
            "s1",
            &[crate::task::model::EvidenceRef {
                evidence_id: "e1".to_owned(),
                kind: EvidenceKind::ParserResult,
            }]
        ),
        StepOpResult::Rejected(OpError::StepRejectsEvidenceKind)
    );
    // Duplicate references are rejected.
    assert_eq!(
        handle.complete_step(
            "s1",
            &[
                crate::task::model::EvidenceRef {
                    evidence_id: "e1".to_owned(),
                    kind: EvidenceKind::WorkspaceRead,
                },
                crate::task::model::EvidenceRef {
                    evidence_id: "e1".to_owned(),
                    kind: EvidenceKind::WorkspaceRead,
                },
            ]
        ),
        StepOpResult::Rejected(OpError::DuplicateEvidenceRef)
    );
    assert_eq!(
        handle.complete_step(
            "s1",
            &[crate::task::model::EvidenceRef {
                evidence_id: "e1".to_owned(),
                kind: EvidenceKind::WorkspaceRead,
            }]
        ),
        StepOpResult::Ok
    );
    assert_eq!(
        handle.complete_step(
            "s1",
            &[crate::task::model::EvidenceRef {
                evidence_id: "e1".to_owned(),
                kind: EvidenceKind::WorkspaceRead,
            }]
        ),
        StepOpResult::Rejected(OpError::StepNotActive)
    );
    assert_eq!(
        handle.fail_step("s1", "already done"),
        StepOpResult::Rejected(OpError::StepAlreadyCompleted)
    );
    // A completed step cannot be re-begun either.
    assert_eq!(
        handle.begin_step("s1"),
        StepOpResult::Rejected(OpError::StepAlreadyCompleted)
    );
}

#[test]
fn a_failed_step_can_be_rebegun() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-step-fail-rebegin");
    assert_eq!(handle.begin_step("s1"), StepOpResult::Ok);
    assert_eq!(
        handle.fail_step("s1", "first attempt failed"),
        StepOpResult::Ok
    );
    assert_eq!(handle.begin_step("s1"), StepOpResult::Ok);
    let state = handle.snapshot();
    assert_eq!(state.steps[0].status, TaskStepStatus::Active);
    assert_eq!(state.steps[0].failed_reason, None);
}

#[test]
fn evidence_validation_enforces_kind_source_binding_and_bounds() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-evidence");
    // Kind/source mismatch.
    assert!(matches!(
        handle.attach_evidence(
            "e1",
            EvidenceKind::ReviewResult,
            workspace_read_evidence("e1", &["a.gd"]),
            None,
        ),
        AttachResult::Rejected(AttachRejection::Invalid(
            EvidenceError::SourceKindMismatch
        ))
    ));
    // Empty id.
    assert!(matches!(
        handle.attach_evidence(
            "   ",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("e1", &["a.gd"]),
            None,
        ),
        AttachResult::Rejected(AttachRejection::Invalid(
            EvidenceError::EmptyId
        ))
    ));
    // Oversized id.
    assert!(matches!(
        handle.attach_evidence(
            &"x".repeat(257),
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("e1", &["a.gd"]),
            None,
        ),
        AttachResult::Rejected(AttachRejection::Invalid(
            EvidenceError::IdTooLarge
        ))
    ));
    // Passed verification requires a source that supports success.
    assert!(matches!(
        handle.attach_evidence(
            "e2",
            EvidenceKind::ParserResult,
            EvidenceSource::Parser {
                checked_files: 1,
                valid_files: 0,
                errors: 1,
            },
            Some(verify_verification("c1")),
        ),
        AttachResult::Rejected(AttachRejection::Invalid(
            EvidenceError::PassedOutcomeWithoutSuccessfulSource
        ))
    ));
    // Oversized source.
    assert!(matches!(
        handle.attach_evidence(
            "e3",
            EvidenceKind::WorkspaceRead,
            EvidenceSource::WorkspaceRead {
                paths: vec!["x".repeat(5000)],
                revision: None,
            },
            None,
        ),
        AttachResult::Rejected(AttachRejection::Invalid(
            EvidenceError::SourceTooLarge
        ))
    ));
    // Valid attachment, then duplicate id rejection.
    assert!(matches!(
        handle.attach_evidence(
            "e4",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("e4", &["a.gd"]),
            None,
        ),
        AttachResult::Attached
    ));
    assert!(matches!(
        handle.attach_evidence(
            "e4",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("e4", &["b.gd"]),
            None,
        ),
        AttachResult::Rejected(AttachRejection::DuplicateId)
    ));
    let state = handle.snapshot();
    assert_eq!(state.evidence.len(), 1);
    assert_eq!(state.evidence[0].task_contract_revision, 1);
    assert_eq!(state.evidence[0].task_contract_digest, state.contract_digest);
}

#[test]
fn evidence_record_count_is_bounded() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-evidence-bound");
    for index in 0..crate::task::model::limits::MAX_TASK_EVIDENCE_RECORDS {
        let id = format!("evidence-{index}");
        assert!(matches!(
            handle.attach_evidence(
                &id,
                EvidenceKind::WorkspaceRead,
                workspace_read_evidence(&id, &["a.gd"]),
                None,
            ),
            AttachResult::Attached
        ));
    }
    assert!(matches!(
        handle.attach_evidence(
            "evidence-overflow",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("overflow", &["a.gd"]),
            None,
        ),
        AttachResult::Rejected(AttachRejection::RecordLimit)
    ));
}

#[test]
fn acceptance_verification_requires_exact_bound_successful_evidence() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-acceptance");
    assert_eq!(
        handle.verify_criterion("c1", None, None),
        CriterionResult::Rejected(
            OpError::CriterionRequiresVerificationEvidence
        )
    );
    assert_eq!(
        handle.verify_criterion("missing", Some("e1"), None),
        CriterionResult::Rejected(OpError::UnknownCriterion)
    );
    // Evidence bound to another check cannot verify this criterion.
    assert!(matches!(
        handle.attach_evidence(
            "e-wrong",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("e-wrong", &["a.gd"]),
            Some(EvidenceVerification {
                check_id: "other".to_owned(),
                criterion_id: Some("other".to_owned()),
                outcome: VerificationOutcome::Passed,
                milestone: None,
            }),
        ),
        AttachResult::Attached
    ));
    assert_eq!(
        handle.verify_criterion("c1", Some("e-wrong"), None),
        CriterionResult::Rejected(OpError::EvidenceNotBoundToCriterion)
    );
    // Failed outcome evidence cannot satisfy acceptance.
    assert!(matches!(
        handle.attach_evidence(
            "e-failed",
            EvidenceKind::ParserResult,
            EvidenceSource::Parser {
                checked_files: 1,
                valid_files: 0,
                errors: 1,
            },
            Some(EvidenceVerification {
                check_id: "c1".to_owned(),
                criterion_id: Some("c1".to_owned()),
                outcome: VerificationOutcome::Failed,
                milestone: None,
            }),
        ),
        AttachResult::Attached
    ));
    assert_eq!(
        handle.verify_criterion("c1", Some("e-failed"), None),
        CriterionResult::Rejected(OpError::EvidenceNotSuccessful)
    );
    // A parser source with errors cannot claim success even when the
    // verification outcome says passed.
    assert!(matches!(
        handle.attach_evidence(
            "e-inconsistent",
            EvidenceKind::ParserResult,
            EvidenceSource::Parser {
                checked_files: 1,
                valid_files: 0,
                errors: 1,
            },
            Some(verify_verification("c1")),
        ),
        AttachResult::Rejected(AttachRejection::Invalid(
            EvidenceError::PassedOutcomeWithoutSuccessfulSource
        ))
    ));
    // Wrong verification kind for a deterministic criterion.
    assert!(matches!(
        handle.attach_evidence(
            "e-review",
            EvidenceKind::ReviewResult,
            EvidenceSource::Review {
                status: "clean".to_owned(),
                blocking_findings: 0,
            },
            Some(verify_verification("c1")),
        ),
        AttachResult::Attached
    ));
    assert_eq!(
        handle.verify_criterion("c1", Some("e-review"), None),
        CriterionResult::Rejected(OpError::EvidenceKindCannotVerifyCriterion)
    );
    // Exact valid evidence satisfies the criterion.
    assert!(matches!(
        handle.attach_evidence(
            "e-ok",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("e-ok", &["a.gd"]),
            Some(verify_verification("c1")),
        ),
        AttachResult::Attached
    ));
    assert_eq!(
        handle.verify_criterion("c1", Some("e-ok"), None),
        CriterionResult::Verified
    );
    let state = handle.snapshot();
    assert_eq!(state.acceptance[0].status, AcceptanceStatus::Satisfied);
    assert_eq!(state.acceptance[0].verified_by.as_deref(), Some("e-ok"));
}

#[test]
fn review_and_user_criteria_require_matching_evidence_kinds() {
    let mut runtime = runtime();
    let task_id = runtime
        .create_task(crate::task::runtime::CreateTaskInput {
            contract: contract(
                "task-kinds",
                vec![
                    criterion("review", VerificationKind::Review),
                    criterion("user", VerificationKind::User),
                ],
            ),
            steps: Vec::new(),
            iteration: None,
        })
        .expect("created");
    let mut handle = runtime.task(&task_id).expect("handle");
    // A review criterion cannot be satisfied by workspace-read evidence.
    assert!(matches!(
        handle.attach_evidence(
            "e-read",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("e-read", &["a.gd"]),
            Some(EvidenceVerification {
                check_id: "review".to_owned(),
                criterion_id: Some("review".to_owned()),
                outcome: VerificationOutcome::Passed,
                milestone: None,
            }),
        ),
        AttachResult::Attached
    ));
    assert_eq!(
        handle.verify_criterion("review", Some("e-read"), None),
        CriterionResult::Rejected(OpError::EvidenceKindCannotVerifyCriterion)
    );
    // A clean review result satisfies the review criterion.
    assert!(matches!(
        handle.attach_evidence(
            "e-review",
            EvidenceKind::ReviewResult,
            EvidenceSource::Review {
                status: "clean".to_owned(),
                blocking_findings: 0,
            },
            Some(EvidenceVerification {
                check_id: "review".to_owned(),
                criterion_id: Some("review".to_owned()),
                outcome: VerificationOutcome::Passed,
                milestone: None,
            }),
        ),
        AttachResult::Attached
    ));
    assert_eq!(
        handle.verify_criterion("review", Some("e-review"), None),
        CriterionResult::Verified
    );
    // A review with blocking findings cannot claim success.
    assert!(matches!(
        handle.attach_evidence(
            "e-review-blocked",
            EvidenceKind::ReviewResult,
            EvidenceSource::Review {
                status: "findings".to_owned(),
                blocking_findings: 2,
            },
            Some(EvidenceVerification {
                check_id: "user".to_owned(),
                criterion_id: Some("user".to_owned()),
                outcome: VerificationOutcome::Passed,
                milestone: None,
            }),
        ),
        AttachResult::Rejected(AttachRejection::Invalid(
            EvidenceError::PassedOutcomeWithoutSuccessfulSource
        ))
    ));
    // A denied user approval cannot satisfy a user criterion.
    assert!(matches!(
        handle.attach_evidence(
            "e-denied",
            EvidenceKind::UserApproval,
            EvidenceSource::UserApproval {
                approval_id: "a1".to_owned(),
                subject_id: "s1".to_owned(),
                decision: ApprovalDecision::Denied,
            },
            Some(EvidenceVerification {
                check_id: "user".to_owned(),
                criterion_id: Some("user".to_owned()),
                outcome: VerificationOutcome::Passed,
                milestone: None,
            }),
        ),
        AttachResult::Rejected(AttachRejection::Invalid(
            EvidenceError::PassedOutcomeWithoutSuccessfulSource
        ))
    ));
    // An approved user approval satisfies the user criterion.
    assert!(matches!(
        handle.attach_evidence(
            "e-approved",
            EvidenceKind::UserApproval,
            EvidenceSource::UserApproval {
                approval_id: "a1".to_owned(),
                subject_id: "s1".to_owned(),
                decision: ApprovalDecision::Approved,
            },
            Some(EvidenceVerification {
                check_id: "user".to_owned(),
                criterion_id: Some("user".to_owned()),
                outcome: VerificationOutcome::Passed,
                milestone: None,
            }),
        ),
        AttachResult::Attached
    ));
    assert_eq!(
        handle.verify_criterion("user", Some("e-approved"), None),
        CriterionResult::Verified
    );
}

#[test]
fn contract_revision_invalidates_prior_acceptance() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-revised-acceptance");
    assert!(matches!(
        handle.attach_evidence(
            "e1",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("e1", &["a.gd"]),
            Some(verify_verification("c1")),
        ),
        AttachResult::Attached
    ));
    assert_eq!(
        handle.verify_criterion("c1", Some("e1"), None),
        CriterionResult::Verified
    );
    let revision = handle
        .revise_contract(crate::task::contract::ReviseTaskContractInput {
            id: "task-revised-acceptance".to_owned(),
            request: None,
            context: Some(crate::task::contract::ReviseContext::Set(
                "new task context".to_owned(),
            )),
            constraints: None,
            acceptance_criteria: None,
            pause_policy: None,
        })
        .expect("revised");
    assert_eq!(revision.revision(), 2);
    // Acceptance is reconciled against the new exact contract.
    let state = handle.snapshot();
    assert_eq!(state.contract_revision, 2);
    assert_eq!(state.acceptance[0].status, AcceptanceStatus::Pending);
    assert_eq!(state.acceptance[0].verified_by, None);
    // Evidence bound to the previous revision can no longer verify.
    assert_eq!(
        handle.verify_criterion("c1", Some("e1"), None),
        CriterionResult::Rejected(OpError::EvidenceNotBoundToContract)
    );
}

#[test]
fn completion_gate_lists_exact_missing_conditions() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-completion-gate");
    let evaluation = handle.evaluate_completion();
    assert!(!evaluation.allowed);
    assert_eq!(
        evaluation.missing,
        vec![
            "step not completed: s1".to_owned(),
            "acceptance criterion not satisfied: c1".to_owned(),
            "validation is not_run (clean required)".to_owned(),
            "review is not_run (clean required)".to_owned(),
        ]
    );
    assert_eq!(
        handle.complete_task(),
        CompletionResult::Rejected { reasons: evaluation.missing.clone() }
    );
    // Rejected completion leaves no partial terminal mutation.
    let state = handle.snapshot();
    assert_eq!(state.phase, TaskPhase::Prepared);
    assert_eq!(state.completed_at_ms, None);
}

#[test]
fn completion_succeeds_only_through_the_full_host_gate() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-completion-success");
    assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
    assert_eq!(handle.begin_step("s1"), StepOpResult::Ok);
    assert!(matches!(
        handle.attach_evidence(
            "e1",
            EvidenceKind::WorkspaceRead,
            workspace_read_evidence("e1", &["project.main"]),
            Some(verify_verification("c1")),
        ),
        AttachResult::Attached
    ));
    assert_eq!(
        handle.complete_step(
            "s1",
            &[crate::task::model::EvidenceRef {
                evidence_id: "e1".to_owned(),
                kind: EvidenceKind::WorkspaceRead,
            }]
        ),
        StepOpResult::Ok
    );
    assert_eq!(
        handle.verify_criterion("c1", Some("e1"), None),
        CriterionResult::Verified
    );
    // Validation 'warnings' is acceptable; 'failed' blocks completion.
    handle.set_validation_status(TaskValidationStatus::Warnings);
    handle.set_review_status(TaskReviewStatus::Clean);
    assert!(handle.evaluate_completion().allowed);
    // Blocking findings close the gate again.
    assert!(
        handle
            .set_findings(vec![FindingInput {
                finding_id: "f1".to_owned(),
                severity: FindingSeverity::High,
                source: "review".to_owned(),
            }])
            .is_ok()
    );
    let blocked = handle.evaluate_completion();
    assert!(!blocked.allowed);
    assert_eq!(
        blocked.missing,
        vec!["1 blocking finding(s) unresolved".to_owned()]
    );
    // Non-blocking findings do not close the gate.
    assert!(
        handle
            .set_findings(vec![FindingInput {
                finding_id: "f2".to_owned(),
                severity: FindingSeverity::Low,
                source: "review".to_owned(),
            }])
            .is_ok()
    );
    assert!(handle.evaluate_completion().allowed);
    assert_eq!(handle.complete_task(), CompletionResult::Completed);
    let state = handle.snapshot();
    assert_eq!(state.phase, TaskPhase::Completed);
    assert_eq!(state.completed_at_ms, Some(NOW));
    assert_eq!(state.terminal_reason, None);
    assert_eq!(
        handle.activity_log().last().unwrap().type_str(),
        "task_completed"
    );
}

#[test]
fn dispositions_are_requests_not_mutations() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-dispositions");
    // Blocked requires a phase that allows the transition (prepared does
    // not), so move to working first.
    assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
    // Continue is accepted and recorded.
    let continued = handle.submit_disposition(
        WorkflowDisposition::Continue {
            next_action: Some("keep going".to_owned()),
        },
        DispositionSource::Model,
    );
    assert!(continued.accepted);
    // Complete is rejected while the gate is unsatisfied.
    let completed = handle.submit_disposition(
        WorkflowDisposition::Complete,
        DispositionSource::Host,
    );
    assert!(!completed.accepted);
    assert!(completed.evaluation.is_some());
    assert_eq!(completed.code, Some(OpError::CompletionGate));
    // Blocked transitions through the table and records the reason.
    let blocked = handle.submit_disposition(
        WorkflowDisposition::Blocked { reason: "needs user input".to_owned() },
        DispositionSource::Host,
    );
    assert!(blocked.accepted);
    let state = handle.snapshot();
    assert_eq!(state.phase, TaskPhase::Blocked);
    assert_eq!(state.terminal_reason.as_deref(), Some("needs user input"));
    let types: Vec<&str> =
        handle.activity_log().iter().map(ActivityEvent::type_str).collect();
    assert_eq!(
        types,
        vec![
            "task_started",
            "task_phase_changed",
            "disposition_submitted",
            "disposition_submitted",
            "task_phase_changed",
            "task_blocked",
            "disposition_submitted",
        ]
    );
    // Activity sequences are monotonic.
    let sequences: Vec<u64> =
        handle.activity_log().iter().map(ActivityEvent::sequence).collect();
    assert_eq!(sequences, vec![1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn cancel_and_fail_terminalize_with_reasons() {
    let mut cancel_runtime = runtime();
    let mut handle = create(&mut cancel_runtime, "task-cancel");
    handle.cancel("no longer needed");
    let state = handle.snapshot();
    assert_eq!(state.phase, TaskPhase::Cancelled);
    assert_eq!(state.terminal_reason.as_deref(), Some("no longer needed"));
    assert_eq!(state.completed_at_ms, Some(NOW));
    // A second cancel is a no-op that preserves the first reason.
    handle.cancel("second reason");
    assert_eq!(
        handle.snapshot().terminal_reason.as_deref(),
        Some("no longer needed")
    );

    let mut fail_runtime = runtime();
    let task_id = fail_runtime
        .create_task(crate::task::runtime::CreateTaskInput {
            contract: contract(
                "task-fail",
                vec![criterion("c1", VerificationKind::Deterministic)],
            ),
            steps: Vec::new(),
            iteration: None,
        })
        .expect("created");
    let mut handle = fail_runtime.task(&task_id).expect("handle");
    handle.fail("verification failed");
    let state = handle.snapshot();
    assert_eq!(state.phase, TaskPhase::Failed);
    assert_eq!(state.terminal_reason.as_deref(), Some("verification failed"));
}

#[test]
fn progress_distinguishes_useful_observations_from_repetition() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-progress");
    assert_eq!(handle.progress().state, ProgressStateValue::Healthy);
    assert_eq!(handle.progress().useful_observations, 1, "task.started");
    for _ in 0..2 {
        handle.observe(HostObservation {
            action: "tool.read".to_owned(),
            fingerprint: "same".to_owned(),
            progress: false,
        });
    }
    let degraded = handle.observe(HostObservation {
        action: "tool.read".to_owned(),
        fingerprint: "same".to_owned(),
        progress: false,
    });
    assert_eq!(degraded.state, ProgressStateValue::Degraded);
    handle.observe(HostObservation {
        action: "tool.read".to_owned(),
        fingerprint: "same".to_owned(),
        progress: false,
    });
    let stalled = handle.observe(HostObservation {
        action: "tool.read".to_owned(),
        fingerprint: "same".to_owned(),
        progress: false,
    });
    assert_eq!(stalled.state, ProgressStateValue::Stalled);
    // A new useful observation recovers to healthy.
    let recovered = handle.observe(HostObservation {
        action: "tool.write".to_owned(),
        fingerprint: "new".to_owned(),
        progress: true,
    });
    assert_eq!(recovered.state, ProgressStateValue::Healthy);
}

#[test]
fn findings_are_bounded_validated_and_detached() {
    let mut runtime = runtime();
    let mut handle = create(&mut runtime, "task-findings");
    assert_eq!(
        handle
            .set_findings(vec![FindingInput {
                finding_id: "f1".to_owned(),
                severity: FindingSeverity::Low,
                source: "review".to_owned(),
            }])
            .expect("valid"),
        ()
    );
    assert!(matches!(
        handle.set_findings(vec![
            FindingInput {
                finding_id: "f1".to_owned(),
                severity: FindingSeverity::Low,
                source: "review".to_owned(),
            },
            FindingInput {
                finding_id: "f1".to_owned(),
                severity: FindingSeverity::Low,
                source: "review".to_owned(),
            }
        ]),
        Err(FindingError::DuplicateId)
    ));
    assert!(matches!(
        handle.set_findings(vec![FindingInput {
            finding_id: "".to_owned(),
            severity: FindingSeverity::Low,
            source: "review".to_owned(),
        }]),
        Err(FindingError::EmptyField)
    ));
    let oversized = "x"
        .repeat(crate::task::model::limits::MAX_TASK_FINDING_FIELD_BYTES + 1);
    assert!(matches!(
        handle.set_findings(vec![FindingInput {
            finding_id: oversized,
            severity: FindingSeverity::Low,
            source: "review".to_owned(),
        }]),
        Err(FindingError::FieldTooLarge)
    ));
}

#[test]
fn iteration_is_normalized_and_terminal_setters_are_noops() {
    let mut runtime = runtime();
    let task_id = runtime
        .create_task(crate::task::runtime::CreateTaskInput {
            contract: contract(
                "task-iteration",
                vec![criterion("c1", VerificationKind::Deterministic)],
            ),
            steps: Vec::new(),
            iteration: Some(3.9),
        })
        .expect("created");
    let mut handle = runtime.task(&task_id).expect("handle");
    assert_eq!(handle.snapshot().iteration, 3, "floored");
    handle.set_iteration(7);
    assert_eq!(handle.snapshot().iteration, 7);
    complete_task(&mut handle);
    handle.set_iteration(99);
    handle.set_validation_status(TaskValidationStatus::Failed);
    assert_eq!(handle.snapshot().iteration, 7);
    assert_eq!(
        handle.snapshot().validation_status,
        TaskValidationStatus::Clean
    );
}
