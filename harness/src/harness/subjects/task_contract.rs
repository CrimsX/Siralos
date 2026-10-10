//! Task-contract subject: host-owned task kernel parity record builders.
use crate::harness::*;

// ---------------------------------------------------------------------------
// Stage 3R R3 subject: task-contract (host-owned task kernel parity).
// ---------------------------------------------------------------------------
//
// Executes each R3 scenario against the real siralos-core task runtime and
// builds the canonical R3 observation object that the TypeScript oracle
// probe emits for the same inputs. The observation is semantic parity data,
// never a TypeScript object layout.

pub(in crate::harness) fn verification_kind(
    value: &str,
) -> Result<VerificationKind, HarnessError> {
    match value {
        "deterministic" => Ok(VerificationKind::Deterministic),
        "review" => Ok(VerificationKind::Review),
        "user" => Ok(VerificationKind::User),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported verification kind {value}"
        ))),
    }
}

fn constraint_kind(value: &str) -> Result<ConstraintKind, HarnessError> {
    match value {
        "scope" => Ok(ConstraintKind::Scope),
        "process" => Ok(ConstraintKind::Process),
        "security" => Ok(ConstraintKind::Security),
        "escalation" => Ok(ConstraintKind::Escalation),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported constraint kind {value}"
        ))),
    }
}

fn pause_policy(value: &str) -> Result<PausePolicy, HarnessError> {
    match value {
        "none" => Ok(PausePolicy::None),
        "on_approval" => Ok(PausePolicy::OnApproval),
        "on_escalation" => Ok(PausePolicy::OnEscalation),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported pause policy {value}"
        ))),
    }
}

fn evidence_kind(value: &str) -> Result<EvidenceKind, HarnessError> {
    match value {
        "workspace_read" => Ok(EvidenceKind::WorkspaceRead),
        "parser_result" => Ok(EvidenceKind::ParserResult),
        "validation_result" => Ok(EvidenceKind::ValidationResult),
        "review_result" => Ok(EvidenceKind::ReviewResult),
        "user_approval" => Ok(EvidenceKind::UserApproval),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported evidence kind {value}"
        ))),
    }
}

fn task_phase(value: &str) -> Result<TaskPhase, HarnessError> {
    match value {
        "prepared" => Ok(TaskPhase::Prepared),
        "working" => Ok(TaskPhase::Working),
        "validating" => Ok(TaskPhase::Validating),
        "reviewing" => Ok(TaskPhase::Reviewing),
        "blocked" => Ok(TaskPhase::Blocked),
        "completed" => Ok(TaskPhase::Completed),
        "cancelled" => Ok(TaskPhase::Cancelled),
        "failed" => Ok(TaskPhase::Failed),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported phase {value}"
        ))),
    }
}

fn step_kind(value: &str) -> Result<TaskStepKind, HarnessError> {
    match value {
        "research" => Ok(TaskStepKind::Research),
        "implementation" => Ok(TaskStepKind::Implementation),
        "review" => Ok(TaskStepKind::Review),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported step kind {value}"
        ))),
    }
}

fn validation_status(
    value: &str,
) -> Result<TaskValidationStatus, HarnessError> {
    match value {
        "not_run" => Ok(TaskValidationStatus::NotRun),
        "clean" => Ok(TaskValidationStatus::Clean),
        "warnings" => Ok(TaskValidationStatus::Warnings),
        "failed" => Ok(TaskValidationStatus::Failed),
        "incomplete" => Ok(TaskValidationStatus::Incomplete),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported validation status {value}"
        ))),
    }
}

fn review_status(value: &str) -> Result<TaskReviewStatus, HarnessError> {
    match value {
        "not_run" => Ok(TaskReviewStatus::NotRun),
        "clean" => Ok(TaskReviewStatus::Clean),
        "findings" => Ok(TaskReviewStatus::Findings),
        "incomplete" => Ok(TaskReviewStatus::Incomplete),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported review status {value}"
        ))),
    }
}

fn finding_severity(value: &str) -> Result<FindingSeverity, HarnessError> {
    match value {
        "critical" => Ok(FindingSeverity::Critical),
        "high" => Ok(FindingSeverity::High),
        "medium" => Ok(FindingSeverity::Medium),
        "low" => Ok(FindingSeverity::Low),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported finding severity {value}"
        ))),
    }
}

fn verification_outcome(
    value: &str,
) -> Result<VerificationOutcome, HarnessError> {
    match value {
        "passed" => Ok(VerificationOutcome::Passed),
        "failed" => Ok(VerificationOutcome::Failed),
        "incomplete" => Ok(VerificationOutcome::Incomplete),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported verification outcome {value}"
        ))),
    }
}

fn approval_decision(value: &str) -> Result<ApprovalDecision, HarnessError> {
    match value {
        "approved" => Ok(ApprovalDecision::Approved),
        "denied" => Ok(ApprovalDecision::Denied),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported approval decision {value}"
        ))),
    }
}

fn parse_acceptance_criterion(
    value: &Value,
) -> Result<AcceptanceCriterion, HarnessError> {
    Ok(AcceptanceCriterion::new(
        string_field(value, "id")?,
        string_field(value, "description")?,
        verification_kind(&string_field(value, "verificationKind")?)?,
    ))
}

fn parse_constraint(value: &Value) -> Result<TaskConstraint, HarnessError> {
    Ok(TaskConstraint::new(
        string_field(value, "id")?,
        string_field(value, "description")?,
        constraint_kind(&string_field(value, "kind")?)?,
    ))
}

/// Contract parsing outcome: a validated contract, a reference
/// contract-validation rejection (canonical code), or a structural
/// harness failure (malformed scenario input).
enum ContractParseOutcome {
    Valid(TaskContract),
    Rejected(String),
    Structure(HarnessError),
}

fn parse_contract(value: &Value) -> ContractParseOutcome {
    let parse = (|| -> Result<TaskContract, HarnessError> {
        let constraints = value
            .get("constraints")
            .map(|entries| {
                entries
                    .as_array()
                    .ok_or_else(|| {
                        HarnessError::corpus(
                            "task input constraints must be an array",
                        )
                    })?
                    .iter()
                    .map(parse_constraint)
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let criteria = object_field(value, "acceptanceCriteria")?
            .as_array()
            .ok_or_else(|| {
                HarnessError::corpus(
                    "task input acceptanceCriteria must be an array",
                )
            })?
            .iter()
            .map(parse_acceptance_criterion)
            .collect::<Result<Vec<_>, _>>()?;
        TaskContract::create(CreateTaskContractInput {
            id: string_field(value, "id")?,
            request: string_field(value, "request")?,
            context: optional_string_field(value, "context"),
            constraints,
            acceptance_criteria: criteria,
            pause_policy: Some(pause_policy(
                value
                    .get("pausePolicy")
                    .map(|entry| entry.as_str().unwrap_or("none"))
                    .unwrap_or("none"),
            )?),
        })
        .map_err(|error| {
            HarnessError::corpus(format!(
                "task input contract is invalid: {}",
                error.code()
            ))
        })
    })();
    match parse {
        Ok(contract) => ContractParseOutcome::Valid(contract),
        Err(error) => {
            let text = error.to_string();
            if let Some(code) =
                text.strip_prefix("task input contract is invalid: ")
            {
                ContractParseOutcome::Rejected(code.to_owned())
            } else {
                ContractParseOutcome::Structure(error)
            }
        }
    }
}

fn parse_evidence_source(
    value: &Value,
) -> Result<EvidenceSource, HarnessError> {
    let source_type = string_field(value, "type")?;
    match source_type.as_str() {
        "workspace_read" => {
            let paths = object_field(value, "paths")?
                .as_array()
                .ok_or_else(|| {
                    HarnessError::corpus("task input workspace_read paths must be an array")
                })?
                .iter()
                .map(|entry| string_value(entry, "path"))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(EvidenceSource::WorkspaceRead {
                paths,
                revision: optional_string_field(value, "revision"),
            })
        }
        "parser" => Ok(EvidenceSource::Parser {
            checked_files: object_field(value, "checkedFiles")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus("task input parser checkedFiles must be an integer")
                })?,
            valid_files: object_field(value, "validFiles")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus("task input parser validFiles must be an integer")
                })?,
            errors: object_field(value, "errors")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus("task input parser errors must be an integer")
                })?,
        }),
        "validation" => Ok(EvidenceSource::Validation {
            outcome: string_field(value, "outcome")?,
            workspace_integrity_verified: object_field(value, "workspaceIntegrityVerified")?
                .as_bool()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input validation workspaceIntegrityVerified must be a boolean",
                    )
                })?,
            unexpected_changes: object_field(value, "unexpectedChanges")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input validation unexpectedChanges must be an integer",
                    )
                })?,
        }),
        "review" => Ok(EvidenceSource::Review {
            status: string_field(value, "status")?,
            blocking_findings: object_field(value, "blockingFindings")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input review blockingFindings must be an integer",
                    )
                })?,
        }),
        "user_approval" => Ok(EvidenceSource::UserApproval {
            approval_id: string_field(value, "approvalId")?,
            subject_id: string_field(value, "subjectId")?,
            decision: approval_decision(&string_field(value, "decision")?)?,
        }),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported evidence source type {source_type}"
        ))),
    }
}

fn parse_verification(
    value: &Value,
) -> Result<EvidenceVerification, HarnessError> {
    let milestone = match value.get("milestone") {
        None | Some(Value::Null) => None,
        Some(target) => Some(siralos_core::task::MilestoneEvidenceTarget {
            manifest_id: string_field(target, "manifestId")?,
            manifest_version: object_field(target, "manifestVersion")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "verification manifestVersion must be an integer",
                    )
                })?,
            requirement_id: string_field(target, "requirementId")?,
        }),
    };
    Ok(EvidenceVerification {
        check_id: string_field(value, "checkId")?,
        criterion_id: value
            .get("criterionId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        milestone,
        outcome: verification_outcome(&string_field(value, "outcome")?)?,
    })
}

fn parse_step(value: &Value) -> Result<TaskStepSpec, HarnessError> {
    let accepts = object_field(value, "accepts")?
        .as_array()
        .ok_or_else(|| {
            HarnessError::corpus("task input step accepts must be an array")
        })?
        .iter()
        .map(|entry| evidence_kind(&string_value(entry, "accepts entry")?))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TaskStepSpec {
        id: string_field(value, "id")?,
        description: string_field(value, "description")?,
        kind: step_kind(&string_field(value, "kind")?)?,
        accepts,
    })
}

fn parse_finding(value: &Value) -> Result<FindingInput, HarnessError> {
    Ok(FindingInput {
        finding_id: string_field(value, "findingId")?,
        severity: finding_severity(&string_field(value, "severity")?)?,
        source: string_field(value, "source")?,
    })
}

fn parse_disposition(
    value: &Value,
) -> Result<WorkflowDisposition, HarnessError> {
    match string_field(value, "type")?.as_str() {
        "continue" => Ok(WorkflowDisposition::Continue {
            next_action: optional_string_field(value, "nextAction"),
        }),
        "complete" => Ok(WorkflowDisposition::Complete),
        "blocked" => Ok(WorkflowDisposition::Blocked {
            reason: string_field(value, "reason")?,
        }),
        other => Err(HarnessError::corpus(format!(
            "task input has an unsupported disposition type {other}"
        ))),
    }
}

fn step_op_json(op: &str, result: &StepOpResult) -> Value {
    match result {
        StepOpResult::Ok => json!({ "op": op, "ok": true }),
        StepOpResult::Rejected(error) => {
            json!({ "op": op, "ok": false, "code": error.code() })
        }
    }
}

fn criterion_json(op: &str, result: &CriterionResult) -> Value {
    match result {
        CriterionResult::Verified => json!({ "op": op, "status": "verified" }),
        CriterionResult::Failed => json!({ "op": op, "status": "failed" }),
        CriterionResult::Rejected(error) => {
            json!({ "op": op, "status": "rejected", "code": error.code() })
        }
    }
}

fn disposition_json(disposition: &WorkflowDisposition) -> Value {
    match disposition {
        WorkflowDisposition::Continue { next_action } => match next_action {
            Some(next_action) => {
                json!({ "type": "continue", "nextAction": next_action })
            }
            None => json!({ "type": "continue" }),
        },
        WorkflowDisposition::Complete => json!({ "type": "complete" }),
        WorkflowDisposition::Blocked { reason } => {
            json!({ "type": "blocked", "reason": reason })
        }
    }
}

fn evidence_ref_json(
    reference: &siralos_core::task::model::EvidenceRef,
) -> Value {
    json!({
        "evidenceId": reference.evidence_id,
        "kind": reference.kind.as_str(),
    })
}

fn activity_json(event: &ActivityEvent) -> Value {
    let base =
        json!({ "type": event.type_str(), "sequence": event.sequence() });
    match event {
        ActivityEvent::TaskStarted { contract_revision, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "contractRevision": contract_revision })
        }
        ActivityEvent::TaskPhaseChanged { phase, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "phase": phase.as_str() })
        }
        ActivityEvent::StepStarted { step_id, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "stepId": step_id })
        }
        ActivityEvent::StepCompleted { step_id, evidence_refs, .. } => {
            json!({
                "type": event.type_str(),
                "sequence": event.sequence(),
                "stepId": step_id,
                "evidenceRefs": evidence_refs.iter().map(evidence_ref_json).collect::<Vec<_>>(),
            })
        }
        ActivityEvent::StepFailed { step_id, reason, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "stepId": step_id, "reason": reason })
        }
        ActivityEvent::EvidenceAttached { evidence_id, kind, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "evidenceId": evidence_id, "kind": kind.as_str() })
        }
        ActivityEvent::CriterionVerified {
            criterion_id, verified_by, ..
        } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "criterionId": criterion_id, "verifiedBy": verified_by })
        }
        ActivityEvent::TaskBlocked { reason, .. }
        | ActivityEvent::TaskCancelled { reason, .. }
        | ActivityEvent::TaskFailed { reason, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "reason": reason })
        }
        ActivityEvent::TaskCompleted { .. } => base,
        ActivityEvent::TaskContractRevised { revision, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "revision": revision })
        }
        ActivityEvent::DispositionSubmitted {
            disposition,
            source,
            accepted,
            note,
            ..
        } => {
            json!({
                "type": event.type_str(),
                "sequence": event.sequence(),
                "disposition": disposition_json(disposition),
                "source": source.as_str(),
                "accepted": accepted,
                "note": note,
            })
        }
        ActivityEvent::PlanningRouted { depth, reason, .. } => {
            json!({
                "type": event.type_str(),
                "sequence": event.sequence(),
                "depth": depth.as_str(),
                "reason": reason,
            })
        }
        ActivityEvent::PlanCreated { plan_id, revision, depth, .. } => {
            json!({
                "type": event.type_str(),
                "sequence": event.sequence(),
                "planId": plan_id,
                "revision": revision,
                "depth": depth.as_str(),
            })
        }
        ActivityEvent::PlanRejected { reason, .. } => {
            json!({
                "type": event.type_str(),
                "sequence": event.sequence(),
                "reason": reason,
            })
        }
        ActivityEvent::PlanApproved { plan_id, revision, digest, .. } => {
            json!({
                "type": event.type_str(),
                "sequence": event.sequence(),
                "planId": plan_id,
                "revision": revision,
                "digest": digest,
            })
        }
        ActivityEvent::PlanInvalidated {
            plan_id, revision, reason, ..
        } => {
            json!({
                "type": event.type_str(),
                "sequence": event.sequence(),
                "planId": plan_id,
                "revision": revision,
                "reason": reason,
            })
        }
    }
}

/// Deterministic per-scenario clock value (set before each scenario).
static SCENARIO_NOW: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

/// Store the deterministic per-scenario clock (used by the R13.4 module).
pub(crate) fn store_scenario_now(now_ms: i64) {
    SCENARIO_NOW.store(now_ms, std::sync::atomic::Ordering::Relaxed);
}

/// Zero-capture clock feeding the runtime the scenario's controlled now.
pub(crate) fn scenario_clock() -> i64 {
    SCENARIO_NOW.load(std::sync::atomic::Ordering::Relaxed)
}

/// Execute one task-contract scenario against the real siralos-core task
/// runtime and build the canonical R3 observation object.
fn run_task_scenario(input: &Value) -> Result<Value, HarnessError> {
    let now_value =
        input.get("now").and_then(Value::as_i64).ok_or_else(|| {
            HarnessError::corpus("task input requires an integer now")
        })?;
    let contract_value = object_field(input, "contract")?;
    let contract = match parse_contract(contract_value) {
        ContractParseOutcome::Valid(contract) => contract,
        ContractParseOutcome::Rejected(code) => {
            // Contract validation failure: the canonical rejection shape.
            return Ok(json!({ "rejected": true, "code": code }));
        }
        ContractParseOutcome::Structure(error) => return Err(error),
    };
    let steps = input
        .get("steps")
        .map(|entries| {
            entries
                .as_array()
                .ok_or_else(|| {
                    HarnessError::corpus("task input steps must be an array")
                })?
                .iter()
                .map(parse_step)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let iteration = input.get("iteration").and_then(Value::as_f64);

    // The core clock is a zero-capture fn pointer; the scenario now is
    // deterministic per run, so a module-level atomic seam carries it.
    SCENARIO_NOW.store(now_value, std::sync::atomic::Ordering::Relaxed);
    let mut runtime = TaskRuntime::with_clock(scenario_clock);
    let task_id = match runtime.create_task(CreateTaskInput {
        contract,
        steps,
        iteration,
    }) {
        Ok(task_id) => task_id,
        Err(error) => {
            return Ok(json!({ "rejected": true, "code": error.code() }));
        }
    };

    let mut ops: Vec<Value> = Vec::new();
    if let Some(entries) = input.get("ops") {
        let entries = entries.as_array().ok_or_else(|| {
            HarnessError::corpus("task input ops must be an array")
        })?;
        for entry in entries {
            let op = string_field(entry, "op")?;
            let observation = run_task_op(&mut runtime, &task_id, &op, entry)?;
            ops.push(observation);
        }
    }

    let handle = runtime.task(&task_id).expect("task handle exists");
    let state = handle.snapshot();
    let completion = handle.evaluate_completion();
    let progress = handle.progress();
    let activity =
        handle.activity_log().iter().map(activity_json).collect::<Vec<_>>();
    let steps_json = state
        .steps
        .iter()
        .map(|step| {
            json!({
                "id": step.id,
                "status": step.status.as_str(),
                "evidenceRefs": step
                    .evidence_refs
                    .iter()
                    .map(evidence_ref_json)
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let acceptance_json = state
        .acceptance
        .iter()
        .map(|criterion| {
            json!({
                "criterionId": criterion.criterion_id,
                "status": criterion.status.as_str(),
                "verifiedBy": criterion.verified_by,
            })
        })
        .collect::<Vec<_>>();
    let findings_json = state
        .current_findings
        .iter()
        .map(|finding| {
            json!({
                "findingId": finding.finding_id,
                "severity": finding.severity.as_str(),
                "source": finding.source,
            })
        })
        .collect::<Vec<_>>();
    let evidence_ids = state
        .evidence
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();

    Ok(json!({
        "rejected": false,
        "finalPhase": state.phase.as_str(),
        "contractRevision": state.contract_revision,
        "contractDigest": state.contract_digest,
        "stepStates": steps_json,
        "acceptance": acceptance_json,
        "evidenceIds": evidence_ids,
        "validationStatus": state.validation_status.as_str(),
        "reviewStatus": state.review_status.as_str(),
        "iteration": state.iteration,
        "currentFindings": findings_json,
        "terminalReason": state.terminal_reason,
        "startedAtMs": state.started_at_ms,
        "completedAtMs": state.completed_at_ms,
        "ops": ops,
        "activity": activity,
        "completion": { "allowed": completion.allowed, "missing": completion.missing },
        "progress": {
            "state": progress.state.as_str(),
            "usefulObservations": progress.useful_observations,
            "repeatedActions": progress.repeated_actions,
        },
    }))
}

/// Execute one operation and return its canonical observation.
fn run_task_op(
    runtime: &mut TaskRuntime,
    task_id: &str,
    op: &str,
    entry: &Value,
) -> Result<Value, HarnessError> {
    match op {
        "transitionPhase" => {
            let phase = task_phase(&string_field(entry, "phase")?)?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.transition_phase(phase);
            Ok(step_op_json("transitionPhase", &result))
        }
        "beginStep" => {
            let step_id = string_field(entry, "stepId")?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.begin_step(&step_id);
            Ok(step_op_json("beginStep", &result))
        }
        "completeStep" => {
            let step_id = string_field(entry, "stepId")?;
            let refs = object_field(entry, "refs")?
                .as_array()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input completeStep refs must be an array",
                    )
                })?
                .iter()
                .map(|reference| {
                    Ok(siralos_core::task::model::EvidenceRef {
                        evidence_id: string_field(reference, "evidenceId")?,
                        kind: evidence_kind(&string_field(
                            reference, "kind",
                        )?)?,
                    })
                })
                .collect::<Result<Vec<_>, HarnessError>>()?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.complete_step(&step_id, &refs);
            Ok(step_op_json("completeStep", &result))
        }
        "failStep" => {
            let step_id = string_field(entry, "stepId")?;
            let reason = string_field(entry, "reason")?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.fail_step(&step_id, &reason);
            Ok(step_op_json("failStep", &result))
        }
        "attachEvidence" => {
            let id = string_field(entry, "id")?;
            let kind = evidence_kind(&string_field(entry, "kind")?)?;
            let source =
                parse_evidence_source(object_field(entry, "source")?)?;
            let verification = entry
                .get("verification")
                .filter(|value| !value.is_null())
                .map(parse_verification)
                .transpose()?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result =
                handle.attach_evidence(&id, kind, source, verification);
            match result {
                AttachResult::Attached => {
                    Ok(json!({ "op": "attachEvidence", "ok": true }))
                }
                AttachResult::Rejected(rejection) => Ok(json!({
                    "op": "attachEvidence",
                    "ok": false,
                    "code": rejection.code(),
                })),
            }
        }
        "verifyCriterion" => {
            let criterion_id = string_field(entry, "criterionId")?;
            let verified_by = entry
                .get("verifiedBy")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let note = optional_string_field(entry, "note");
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.verify_criterion(
                &criterion_id,
                verified_by.as_deref(),
                note.as_deref(),
            );
            Ok(criterion_json("verifyCriterion", &result))
        }
        "markCriterionFailed" => {
            let criterion_id = string_field(entry, "criterionId")?;
            let note = optional_string_field(entry, "note");
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result =
                handle.mark_criterion_failed(&criterion_id, note.as_deref());
            Ok(criterion_json("markCriterionFailed", &result))
        }
        "setFindings" => {
            let findings = object_field(entry, "findings")?
                .as_array()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input findings must be an array",
                    )
                })?
                .iter()
                .map(parse_finding)
                .collect::<Result<Vec<_>, _>>()?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            match handle.set_findings(findings) {
                Ok(()) => Ok(json!({ "op": "setFindings", "ok": true })),
                Err(error) => Ok(
                    json!({ "op": "setFindings", "ok": false, "code": error.code() }),
                ),
            }
        }
        "setValidationStatus" => {
            let status = validation_status(&string_field(entry, "status")?)?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.set_validation_status(status);
            Ok(json!({ "op": "setValidationStatus", "ok": true }))
        }
        "setReviewStatus" => {
            let status = review_status(&string_field(entry, "status")?)?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.set_review_status(status);
            Ok(json!({ "op": "setReviewStatus", "ok": true }))
        }
        "setIteration" => {
            let iteration = object_field(entry, "iteration")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input iteration must be an integer",
                    )
                })?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.set_iteration(iteration);
            Ok(json!({ "op": "setIteration", "ok": true }))
        }
        "reviseContract" => {
            let changes_value = object_field(entry, "changes")?;
            let changes = ReviseTaskContractInput {
                id: string_field(changes_value, "id")?,
                request: optional_string_field(changes_value, "request"),
                context: match changes_value.get("context") {
                    Some(context) => Some(ReviseContext::Set(
                        context
                            .as_str()
                            .ok_or_else(|| {
                                HarnessError::corpus(
                                    "task input revise context must be a string",
                                )
                            })?
                            .to_owned(),
                    )),
                    None => None,
                },
                constraints: changes_value
                    .get("constraints")
                    .map(|entries| {
                        entries
                            .as_array()
                            .ok_or_else(|| {
                                HarnessError::corpus(
                                    "task input revise constraints must be an array",
                                )
                            })?
                            .iter()
                            .map(parse_constraint)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?,
                acceptance_criteria: changes_value
                    .get("acceptanceCriteria")
                    .map(|entries| {
                        entries
                            .as_array()
                            .ok_or_else(|| {
                                HarnessError::corpus(
                                    "task input revise acceptanceCriteria must be an array",
                                )
                            })?
                            .iter()
                            .map(parse_acceptance_criterion)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?,
                pause_policy: changes_value
                    .get("pausePolicy")
                    .map(|policy| {
                        pause_policy(
                            policy
                                .as_str()
                                .ok_or_else(|| {
                                    HarnessError::corpus(
                                        "task input revise pausePolicy must be a string",
                                    )
                                })?,
                        )
                    })
                    .transpose()?,
            };
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            match handle.revise_contract(changes) {
                Ok(revision) => Ok(json!({
                    "op": "reviseContract",
                    "ok": true,
                    "revision": revision.revision(),
                })),
                Err(error) => Ok(json!({
                    "op": "reviseContract",
                    "ok": false,
                    "code": error.code(),
                })),
            }
        }
        "submitDisposition" => {
            let disposition =
                parse_disposition(object_field(entry, "disposition")?)?;
            let source = match string_field(entry, "source")?.as_str() {
                "host" => DispositionSource::Host,
                "model" => DispositionSource::Model,
                other => {
                    return Err(HarnessError::corpus(format!(
                        "task input has an unsupported disposition source {other}"
                    )));
                }
            };
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.submit_disposition(disposition, source);
            if result.accepted {
                Ok(json!({ "op": "submitDisposition", "accepted": true }))
            } else {
                Ok(json!({
                    "op": "submitDisposition",
                    "accepted": false,
                    "code": result.code.map(|code| code.code()).unwrap_or("rejected"),
                }))
            }
        }
        "completeTask" => {
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            match handle.complete_task() {
                CompletionResult::Completed => {
                    Ok(json!({ "op": "completeTask", "status": "completed" }))
                }
                CompletionResult::Rejected { reasons } => Ok(json!({
                    "op": "completeTask",
                    "status": "rejected",
                    "missing": reasons,
                })),
            }
        }
        "cancel" => {
            let reason = string_field(entry, "reason")?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.cancel(&reason);
            Ok(json!({ "op": "cancel", "ok": true }))
        }
        "fail" => {
            let reason = string_field(entry, "reason")?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.fail(&reason);
            Ok(json!({ "op": "fail", "ok": true }))
        }
        "markBlocked" => {
            let reason = string_field(entry, "reason")?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.mark_blocked(&reason);
            Ok(json!({ "op": "markBlocked", "ok": true }))
        }
        "observe" => {
            let action = string_field(entry, "action")?;
            let fingerprint = string_field(entry, "fingerprint")?;
            let progress = entry
                .get("progress")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.observe(HostObservation { action, fingerprint, progress });
            Ok(json!({ "op": "observe", "ok": true }))
        }
        other => Err(HarnessError::corpus(format!(
            "task input has an unsupported op {other}"
        ))),
    }
}

pub(in crate::harness) fn task_contract_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    let result = run_task_scenario(input)?;
    Ok(json!({
        "scenarioId": scenario_id,
        "subject": SUBJECT_TASK_CONTRACT,
        "outcome": "COMPLETED",
        "result": result,
    }))
}

// ---------------------------------------------------------------------------
// Stage 3R R4 subjects: generic workspace / project foundation.
// ---------------------------------------------------------------------------
//
// Executes each R4 scenario against the REAL Rust candidate workspace
// implementation (siralos-core workspace contracts and siralos-adapters
// workspace adapters) and builds the canonical R4 observation object
// that the TypeScript oracle probe emits for the same inputs. Fixtures
// are created in the host temp directory from declared inputs; records
// contain workspace-relative paths and content identities only.
