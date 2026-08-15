//! R3 task-kernel benchmark baseline (Stage 3R R3 measurement).
//!
//! Small before/after baseline for the host task kernel: contract
//! validation + digest, task creation, phase transition, and
//! evidence-attach + acceptance evaluation. Benchmarks are evidence
//! tools, not PR gates (docs/development/performance-baseline.md).

// The criterion macros generate public harness functions; this is an
// internal benchmark harness, not a public API surface.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};

use siralos_core::task::contract::{
    AcceptanceCriterion, CreateTaskContractInput, TaskContract,
    VerificationKind,
};
use siralos_core::task::evidence::FindingInput;
use siralos_core::task::model::{
    ApprovalDecision, EvidenceKind, EvidenceSource, EvidenceVerification,
    TaskPhase, TaskStepSpec, VerificationOutcome,
};
use siralos_core::task::runtime::{CreateTaskInput, TaskRuntime};

fn contract() -> TaskContract {
    TaskContract::create(CreateTaskContractInput {
        id: "benchmark-task".to_owned(),
        request: "Add a health component with deterministic validation"
            .to_owned(),
        context: Some("Benchmark context.".to_owned()),
        constraints: None,
        acceptance_criteria: vec![AcceptanceCriterion::new(
            "parses".to_owned(),
            "Parses cleanly.".to_owned(),
            VerificationKind::Deterministic,
        )],
        pause_policy: None,
    })
    .expect("valid benchmark contract")
}

fn runtime() -> TaskRuntime {
    TaskRuntime::with_clock(|| 1_700_000_000_000)
}

fn bench_contract_validation_and_digest(criterion: &mut Criterion) {
    criterion.bench_function(
        "task/contract-validation-and-digest",
        |bencher| {
            bencher.iter(|| {
                let contract = contract();
                std::hint::black_box(contract.digest().len())
            });
        },
    );
}

fn bench_task_creation(criterion: &mut Criterion) {
    let input = CreateTaskInput {
        contract: contract(),
        steps: vec![TaskStepSpec {
            id: "s1".to_owned(),
            description: "Implement".to_owned(),
            kind: siralos_core::task::model::TaskStepKind::Implementation,
            accepts: vec![EvidenceKind::WorkspaceRead],
        }],
        iteration: None,
    };
    criterion.bench_function("task/create", |bencher| {
        bencher.iter(|| {
            let mut runtime = runtime();
            let task_id =
                runtime.create_task(input.clone()).expect("task created");
            std::hint::black_box(task_id)
        });
    });
}

fn bench_phase_transition(criterion: &mut Criterion) {
    criterion.bench_function("task/phase-transition", |bencher| {
        bencher.iter(|| {
            let mut runtime = runtime();
            let task_id = runtime
                .create_task(CreateTaskInput {
                    contract: contract(),
                    steps: Vec::new(),
                    iteration: None,
                })
                .expect("task created");
            let mut handle = runtime.task(&task_id).expect("handle");
            std::hint::black_box(handle.transition_phase(TaskPhase::Working));
        });
    });
}

fn bench_evidence_attach_and_acceptance(criterion: &mut Criterion) {
    criterion.bench_function("task/evidence-attach-acceptance", |bencher| {
        bencher.iter(|| {
            let mut runtime = runtime();
            let task_id = runtime
                .create_task(CreateTaskInput {
                    contract: contract(),
                    steps: Vec::new(),
                    iteration: None,
                })
                .expect("task created");
            let mut handle = runtime.task(&task_id).expect("handle");
            let result = handle.attach_evidence(
                "evidence-1",
                EvidenceKind::WorkspaceRead,
                EvidenceSource::WorkspaceRead {
                    paths: vec!["project.main".to_owned()],
                    revision: None,
                },
                Some(EvidenceVerification {
                    check_id: "parses".to_owned(),
                    criterion_id: Some("parses".to_owned()),
                    outcome: VerificationOutcome::Passed,
                }),
            );
            std::hint::black_box(result);
        });
    });
}

fn bench_findings_validation(criterion: &mut Criterion) {
    criterion.bench_function("task/findings-validation", |bencher| {
        bencher.iter(|| {
            let findings = vec![FindingInput {
                finding_id: "finding-1".to_owned(),
                severity: siralos_core::task::model::FindingSeverity::Low,
                source: "review".to_owned(),
            }];
            let _ = std::hint::black_box(
                siralos_core::task::evidence::validate_findings(findings),
            );
        });
    });
}

criterion_group!(
    task_baseline,
    bench_contract_validation_and_digest,
    bench_task_creation,
    bench_phase_transition,
    bench_evidence_attach_and_acceptance,
    bench_findings_validation,
);
criterion_main!(task_baseline);

// Silence unused imports that only serve future benchmarks.
#[allow(unused_imports)]
use ApprovalDecision as _ApprovalDecision;
