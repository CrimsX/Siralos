//! Property tests over the R3 task kernel invariants.
//!
//! These strengthen the unit tests without turning the milestone into a
//! fuzzing project: legal-transition closure, terminal immutability,
//! contract revision monotonicity, id preservation, and digest
//! determinism for the same normalized material input.

use proptest::prelude::*;

use crate::task::contract::{
    AcceptanceCriterion, CreateTaskContractInput, ReviseTaskContractInput,
    TaskContract, VerificationKind,
};
use crate::task::model::{
    AcceptanceStatus, EvidenceKind, EvidenceSource, TaskPhase, TaskStepSpec,
    TaskStepStatus, TaskValidationStatus, WorkflowDisposition,
};
use crate::task::runtime::{
    AttachResult, CompletionResult, CriterionResult, StepOpResult, TaskRuntime,
};

const NOW: i64 = 1_700_000_000_000;

fn sample_contract(id: &str) -> TaskContract {
    TaskContract::create(CreateTaskContractInput {
        id: id.to_owned(),
        request: "Property-test request".to_owned(),
        context: Some("context".to_owned()),
        constraints: None,
        acceptance_criteria: vec![AcceptanceCriterion {
            id: "c1".to_owned(),
            description: "Property criterion".to_owned(),
            verification_kind: VerificationKind::Deterministic,
        }],
        pause_policy: None,
    })
    .expect("valid contract")
}

fn sample_step() -> TaskStepSpec {
    TaskStepSpec {
        id: "s1".to_owned(),
        description: "Property step".to_owned(),
        kind: crate::task::model::TaskStepKind::Implementation,
        accepts: vec![EvidenceKind::WorkspaceRead],
    }
}

proptest! {
    /// From any reachable phase, a legal transition always succeeds, a
    /// same-phase transition is always rejected, and terminal phases
    /// have no outgoing legal transitions.
    #[test]
    fn legal_transition_closure(phase in prop_oneof![
        Just(TaskPhase::Prepared),
        Just(TaskPhase::Working),
        Just(TaskPhase::Validating),
        Just(TaskPhase::Reviewing),
        Just(TaskPhase::Blocked),
        Just(TaskPhase::Completed),
        Just(TaskPhase::Cancelled),
        Just(TaskPhase::Failed),
    ]) {
        let mut runtime = TaskRuntime::with_clock(|| NOW);
        let task_id = runtime
            .create_task(crate::task::runtime::CreateTaskInput {
                contract: sample_contract("task-prop-transitions"),
                steps: vec![sample_step()],
                iteration: None,
            })
            .expect("created");
        let mut handle = runtime.task(&task_id).expect("handle");
        // Move into the requested phase when the transition table allows
        // it; otherwise this phase is simply unreachable and the property
        // is vacuous for it.
        match phase {
            TaskPhase::Prepared => {}
            TaskPhase::Working => {
                assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
            }
            TaskPhase::Validating => {
                assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
                assert_eq!(handle.transition_phase(TaskPhase::Validating), StepOpResult::Ok);
            }
            TaskPhase::Reviewing => {
                assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
                assert_eq!(handle.transition_phase(TaskPhase::Validating), StepOpResult::Ok);
                assert_eq!(handle.transition_phase(TaskPhase::Reviewing), StepOpResult::Ok);
            }
            TaskPhase::Blocked => {
                assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
                assert_eq!(handle.transition_phase(TaskPhase::Blocked), StepOpResult::Ok);
            }
            TaskPhase::Completed => {
                // Only reachable through the completion gate.
                complete_for_property(&mut handle);
            }
            TaskPhase::Cancelled => {
                handle.cancel("property");
            }
            TaskPhase::Failed => {
                handle.fail("property");
            }
        }
        let current = handle.snapshot().phase;
        // Same-phase transitions are always rejected.
        assert_eq!(
            handle.transition_phase(current),
            StepOpResult::Rejected(crate::task::runtime::OpError::AlreadyPhase)
        );
        // Terminal phases accept no further transitions.
        if matches!(
            current,
            TaskPhase::Completed | TaskPhase::Cancelled | TaskPhase::Failed
        ) {
            for candidate in [
                TaskPhase::Prepared,
                TaskPhase::Working,
                TaskPhase::Validating,
                TaskPhase::Reviewing,
                TaskPhase::Blocked,
                TaskPhase::Completed,
                TaskPhase::Cancelled,
                TaskPhase::Failed,
            ] {
                assert!(
                    matches!(
                        handle.transition_phase(candidate),
                        StepOpResult::Rejected(_)
                    ),
                    "terminal phase {current:?} must reject {candidate:?}"
                );
            }
        }
    }

    /// Revision numbers are monotonically increasing and the contract id
    /// never changes across revisions.
    #[test]
    fn revision_monotonicity_and_id_preservation(rounds in 1usize..12) {
        let mut contract = sample_contract("task-prop-revisions");
        for round in 0..rounds {
            let next = contract
                .revise(ReviseTaskContractInput {
                    id: contract.id().to_owned(),
                    request: Some(format!("Request {round}")),
                    context: None,
                    constraints: None,
                    acceptance_criteria: None,
                    pause_policy: None,
                })
                .expect("valid revision");
            assert_eq!(next.id(), contract.id());
            assert_eq!(next.revision(), contract.revision() + 1);
            contract = next;
        }
        assert_eq!(contract.revision(), rounds as u64 + 1);
    }

    /// Digest computation is deterministic for the same material input.
    #[test]
    fn digest_determinism(rounds in 1usize..8) {
        let mut contract = sample_contract("task-prop-digests");
        for _ in 0..rounds {
            let noop = contract
                .revise(ReviseTaskContractInput {
                    id: contract.id().to_owned(),
                    request: None,
                    context: None,
                    constraints: None,
                    acceptance_criteria: None,
                    pause_policy: None,
                })
                .expect("valid no-op revision");
            assert_eq!(noop.digest(), contract.digest());
            contract = noop;
        }
    }

    /// Terminal tasks keep their authoritative state stable under
    /// arbitrary ordinary operations.
    #[test]
    fn terminal_state_is_stable(iterations in 1usize..10) {
        let mut runtime = TaskRuntime::with_clock(|| NOW);
        let task_id = runtime
            .create_task(crate::task::runtime::CreateTaskInput {
                contract: sample_contract("task-prop-terminal"),
                steps: vec![sample_step()],
                iteration: None,
            })
            .expect("created");
        let mut handle = runtime.task(&task_id).expect("handle");
        complete_for_property(&mut handle);
        let before = handle.snapshot();
        for round in 0..iterations {
            let _ = handle.transition_phase(TaskPhase::Working);
            let _ = handle.begin_step("s1");
            let _ = handle.complete_step("s1", &[]);
            let _ = handle.attach_evidence(
                &format!("late-{round}"),
                EvidenceKind::WorkspaceRead,
                EvidenceSource::WorkspaceRead {
                    paths: vec!["late.gd".to_owned()],
                    revision: None,
                },
                None,
            );
            let _ = handle.verify_criterion("c1", Some("missing"), None);
            let _ = handle.mark_criterion_failed("c1", None);
            let _ = handle.complete_task();
            let _ = handle.submit_disposition(
                WorkflowDisposition::Complete,
                crate::task::model::DispositionSource::Model,
            );
            handle.set_validation_status(TaskValidationStatus::Failed);
            handle.set_iteration(round as u64);
            handle.cancel("late");
            handle.fail("late");
        }
        prop_assert_eq!(handle.snapshot(), before);
        prop_assert!(handle.snapshot().steps.iter().all(|step| step.status == TaskStepStatus::Completed));
        prop_assert!(handle.snapshot().acceptance.iter().all(|criterion| criterion.status == AcceptanceStatus::Satisfied));
    }
}

/// Drive a task through the completion gate for the property helpers.
fn complete_for_property(handle: &mut crate::task::runtime::TaskHandle<'_>) {
    assert_eq!(handle.transition_phase(TaskPhase::Working), StepOpResult::Ok);
    assert!(matches!(
        handle.attach_evidence(
            "evidence-1",
            EvidenceKind::WorkspaceRead,
            EvidenceSource::WorkspaceRead {
                paths: vec!["project.main".to_owned()],
                revision: None,
            },
            Some(crate::task::model::EvidenceVerification {
                check_id: "c1".to_owned(),
                criterion_id: Some("c1".to_owned()),
                outcome: crate::task::model::VerificationOutcome::Passed,
            }),
        ),
        AttachResult::Attached
    ));
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
    assert_eq!(
        handle.verify_criterion("c1", Some("evidence-1"), None),
        CriterionResult::Verified
    );
    handle.set_validation_status(TaskValidationStatus::Clean);
    handle.set_review_status(crate::task::model::TaskReviewStatus::Clean);
    assert_eq!(handle.complete_task(), CompletionResult::Completed);
}
