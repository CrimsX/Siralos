//! Stage 3R R13.4 differential candidate execution: planning-runtime and
//! executor-brief subjects.
//!
//! Mirrors `planning-runtime-oracle.mjs` / `executor-brief-oracle.mjs`
//! against the real Rust candidate modules (`siralos_core::planning`,
//! `siralos_core::executor`, and the plan lifecycle on
//! `siralos_core::task`). Fixtures are synthetic constants; every
//! timestamp comes from the scenario clock.

use serde_json::{Value, json};

const PLAN_REV_A: &str = "rev_a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
const PLAN_REV_B: &str = "rev_b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6a1";
pub(crate) const MAX_R13_PLANNING_BRIEFING_INPUT_BYTES: usize = 64 * 1024;

type Contract = siralos_core::task::TaskContract;
type Runtime = siralos_core::task::TaskRuntime;

fn contract_with_criteria(
    criteria: Vec<siralos_core::task::AcceptanceCriterion>,
) -> Contract {
    siralos_core::task::TaskContract::create(
        siralos_core::task::CreateTaskContractInput {
            id: "task-1".to_owned(),
            request: "Implement the bounded feature".to_owned(),
            context: None,
            constraints: None,
            acceptance_criteria: criteria,
            pause_policy: None,
        },
    )
    .expect("valid contract")
}

fn briefing_request_contract() -> Contract {
    use siralos_core::task::VerificationKind;
    siralos_core::task::TaskContract::create(
        siralos_core::task::CreateTaskContractInput {
            id: "task-1".to_owned(),
            request: "Implement the executor briefing surface".to_owned(),
            context: None,
            constraints: None,
            acceptance_criteria: vec![
                criterion(
                    "ac1",
                    "feature works",
                    VerificationKind::Deterministic,
                ),
                criterion("ac2", "review clean", VerificationKind::Review),
            ],
            pause_policy: None,
        },
    )
    .expect("valid contract")
}

fn criterion(
    id: &str,
    description: &str,
    kind: siralos_core::task::VerificationKind,
) -> siralos_core::task::AcceptanceCriterion {
    siralos_core::task::AcceptanceCriterion::new(
        id.to_owned(),
        description.to_owned(),
        kind,
    )
}

fn main_contract() -> Contract {
    use siralos_core::task::VerificationKind;
    contract_with_criteria(vec![
        criterion("ac1", "feature works", VerificationKind::Deterministic),
        criterion("ac2", "tests pass", VerificationKind::Review),
    ])
}

fn single_criterion_contract() -> Contract {
    use siralos_core::task::VerificationKind;
    contract_with_criteria(vec![criterion(
        "ac1",
        "works",
        VerificationKind::Deterministic,
    )])
}

fn user_only_contract() -> Contract {
    use siralos_core::task::VerificationKind;
    contract_with_criteria(vec![
        criterion("ac1", "looks right", VerificationKind::User),
        criterion("ac2", "feels right", VerificationKind::User),
    ])
}

fn new_runtime(now_ms: i64, contract: &Contract) -> (Runtime, String) {
    crate::harness::store_scenario_now(now_ms);
    let mut runtime = Runtime::with_clock(crate::harness::scenario_clock);
    let task_id = runtime
        .create_task(siralos_core::task::CreateTaskInput {
            contract: contract.clone(),
            steps: Vec::new(),
            iteration: None,
        })
        .expect("valid task");
    (runtime, task_id)
}

fn op_rejection(op: &str, result: &Result<(), String>) -> Value {
    let mut base = json!({ "op": op });
    match result {
        Ok(()) => base["ok"] = json!(true),
        Err(reason) => {
            base["ok"] = json!(false);
            base["reason"] = json!(reason);
        }
    }
    base
}

// ---------------------------------------------------------------------------
// Planning fixtures.
// ---------------------------------------------------------------------------

fn plan_content() -> siralos_core::planning::TaskPlanContent {
    use siralos_core::planning::{
        PlanConstraint, PlanRisk, PlanRiskSeverity, PlanRollbackStrategy,
        PlanScope, PlanStep, PlanTouchpoint, PlanValidationStrategy,
        TaskPlanContent, TouchpointConfidence,
    };
    TaskPlanContent {
        objective: "Implement the feature".to_owned(),
        scope: PlanScope {
            in_scope: vec!["src/a.ts".to_owned()],
            out_of_scope: vec!["docs".to_owned()],
        },
        non_goals: vec!["no public API change".to_owned()],
        touchpoints: vec![
            PlanTouchpoint {
                id: "tp1".to_owned(),
                path: "src/a.ts".to_owned(),
                confidence: TouchpointConfidence::Verified,
                revision: Some(PLAN_REV_A.to_owned()),
                evidence: Some("read:src/a.ts".to_owned()),
                note: None,
            },
            PlanTouchpoint {
                id: "tp2".to_owned(),
                path: "src/b*.ts".to_owned(),
                confidence: TouchpointConfidence::Candidate,
                revision: None,
                evidence: None,
                note: None,
            },
        ],
        constraints: vec![PlanConstraint {
            id: "con1".to_owned(),
            description: "stay within scope".to_owned(),
        }],
        risks: vec![PlanRisk {
            id: "risk1".to_owned(),
            severity: PlanRiskSeverity::Low,
            description: "minor regression risk".to_owned(),
        }],
        steps: vec![
            PlanStep {
                id: "s1".to_owned(),
                title: "Edit a".to_owned(),
                description: None,
                expected_touchpoints: vec!["tp1".to_owned()],
                verification: Some(vec!["ac1".to_owned()]),
            },
            PlanStep {
                id: "s2".to_owned(),
                title: "Verify b".to_owned(),
                description: None,
                expected_touchpoints: vec!["tp2".to_owned()],
                verification: Some(vec!["ac2".to_owned()]),
            },
        ],
        validation: PlanValidationStrategy {
            checks: vec!["check-only parse".to_owned()],
            requirements: Some(vec!["workspace mutation".to_owned()]),
        },
        rollback: Some(PlanRollbackStrategy {
            description: "revert commits".to_owned(),
        }),
        rationale: Some("straightforward".to_owned()),
    }
}

fn create_plan(
    contract_digest: &str,
    id: &str,
    content: siralos_core::planning::TaskPlanContent,
    created_at: i64,
) -> siralos_core::planning::TaskPlan {
    siralos_core::planning::create_task_plan(
        siralos_core::planning::CreateTaskPlanInput {
            id: id.to_owned(),
            task_id: "task-1".to_owned(),
            task_contract_revision: 1,
            task_contract_digest: contract_digest.to_owned(),
            depth: siralos_core::planning::PlanningDepth::Full,
            content,
            created_at,
        },
    )
    .expect("valid plan")
}

fn revise_to(
    previous: &siralos_core::planning::TaskPlan,
    revision: u64,
) -> siralos_core::planning::TaskPlan {
    let mut plan = previous.clone();
    while plan.revision < revision {
        plan = siralos_core::planning::revise_task_plan(
            &plan,
            &siralos_core::planning::ReviseTaskPlanInput {
                content: plan_content(),
                task_contract_digest: None,
            },
        )
        .expect("valid intermediate revision");
    }
    plan
}

fn step_of(id: &str) -> Value {
    json!({ "id": id, "title": format!("Step {id}"), "expectedTouchpoints": [] })
}

/// Full valid candidate as raw JSON (mirrors the TS fixture object).
fn full_candidate_json() -> Value {
    json!({
        "objective": "Implement the feature",
        "scope": {"inScope": ["src/a.ts"], "outOfScope": ["docs"]},
        "nonGoals": ["no public API change"],
        "touchpoints": [
            {"id":"tp1","path":"src/a.ts","confidence":"verified","revision":PLAN_REV_A,"evidence":"read:src/a.ts"},
            {"id":"tp2","path":"src/b*.ts","confidence":"candidate"}],
        "constraints": [{"id":"con1","description":"stay within scope"}],
        "risks": [{"id":"risk1","severity":"low","description":"minor regression risk"}],
        "steps": [
            {"id":"s1","title":"Edit a","expectedTouchpoints":["tp1"],"verification":["ac1"]},
            {"id":"s2","title":"Verify b","expectedTouchpoints":["tp2"],"verification":["ac2"]}],
        "validation": {"checks":["check-only parse"],"requirements":["workspace mutation"]},
        "rollback": {"description":"revert commits"},
        "rationale": "straightforward"
    })
}

/// Candidate variants that keep the shared shape but override one field.
fn candidate_with_field(patch: Value) -> Value {
    let mut value =
        serde_json::to_value(full_candidate_json()).expect("serializable");
    if let (Some(target), Some(patch_object)) =
        (value.as_object_mut(), patch.as_object())
    {
        for (key, patched) in patch_object {
            target.insert(key.clone(), patched.clone());
        }
    }
    value
}

struct ScriptedPlanner {
    queue: std::collections::VecDeque<siralos_core::planning::PlannerOutcome>,
}

impl siralos_core::planning::PlannerPort for ScriptedPlanner {
    fn plan(
        &mut self,
        _input: &siralos_core::planning::PlannerRequest<'_>,
    ) -> siralos_core::planning::PlannerOutcome {
        match self.queue.pop_front() {
            Some(outcome) => outcome,
            None => panic!("scripted planner exhausted"),
        }
    }
}

fn planning_input(
    overrides: &Value,
) -> siralos_core::planning::PlanningDecisionInput<'_> {
    let flag = |key: &str| {
        overrides.get(key).and_then(Value::as_bool).unwrap_or(false)
    };
    let requested_depth =
        if overrides.get("requestedDepth").and_then(Value::as_str)
            == Some("light")
        {
            Some(siralos_core::planning::RequestedDepth::Light)
        } else {
            None
        };
    let surface = match overrides.get("surface").and_then(Value::as_str) {
        Some("mixed") => Some(siralos_core::planning::SurfaceKind::Mixed),
        _ => None,
    };
    siralos_core::planning::PlanningDecisionInput {
        request: overrides
            .get("request")
            .and_then(Value::as_str)
            .unwrap_or("implement the feature"),
        explicit_plan_request: flag("explicitPlanRequest"),
        requested_depth,
        inspection_only: flag("inspectionOnly"),
        expected_mutation: overrides
            .get("expectedMutation")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        acceptance_criterion_count: overrides
            .get("acceptanceCriterionCount")
            .and_then(Value::as_u64)
            .unwrap_or(2) as usize,
        protected_config_involved: flag("protectedConfigInvolved"),
        spans_multiple_subsystems: flag("spansMultipleSubsystems"),
        research_required: flag("researchRequired"),
        capability_uncertainty: flag("capabilityUncertainty"),
        narrow_repair: flag("narrowRepair"),
        known_touchpoints: overrides
            .get("knownTouchpoints")
            .and_then(Value::as_u64)
            .unwrap_or(3) as usize,
        involves_godot_scene_or_resource: overrides
            .get("involvesGodotSceneOrResource")
            .and_then(Value::as_bool),
        surface,
    }
}

fn plan_state_value(state: &siralos_core::planning::TaskPlanState) -> Value {
    json!({
        "planId": state.plan_id,
        "planRevision": state.plan_revision,
        "planDigest": state.plan_digest,
        "depth": state.depth.as_str(),
        "state": state.state.as_str(),
        "approval": state.approval.as_str(),
        "staleReason": state.stale_reason,
    })
}

#[allow(clippy::too_many_lines)]
pub(crate) fn planning_runtime_record(
    input: &Value,
) -> Result<Value, crate::harness::HarnessError> {
    use siralos_core::planning::{
        PlanCandidateContext, PlanCandidateResult, PlannerOutcome,
        PlanningDepth, PlanningFlowState, PlanningLimits, PlanningPolicy,
        content_candidate_value, extract_plan_candidate_json,
        has_meaningful_acceptance_criteria, is_safe_plan_path,
        validate_plan_candidate,
    };
    let now_ms =
        input.get("nowMs").and_then(Value::as_i64).ok_or_else(|| {
            crate::harness::HarnessError::corpus(
                "planning input requires nowMs",
            )
        })?;
    let contract = main_contract();
    let mut cases = Vec::new();
    for input_case in
        input.get("cases").and_then(Value::as_array).expect("validated cases")
    {
        let name = string_field(input_case)?;
        let case = match name.as_str() {
            "plan-model-identity" => {
                let plan_a = create_plan(
                    contract.digest(),
                    "plan-task-1",
                    plan_content(),
                    now_ms,
                );
                let digest_a = plan_a.digest.value.clone();
                let recomputed =
                    siralos_core::planning::compute_plan_revision_digest(
                        &plan_a,
                    )
                    .expect("digest");
                let mut content_b = plan_content();
                content_b.objective =
                    "Implement the feature faster".to_owned();
                let plan_b = siralos_core::planning::revise_task_plan(
                    &plan_a,
                    &siralos_core::planning::ReviseTaskPlanInput {
                        content: content_b,
                        task_contract_digest: None,
                    },
                )
                .expect("valid revision");
                json!({
                    "name": "plan-model-identity",
                    "revisionA": plan_a.revision,
                    "digestA": digest_a,
                    "digestADeterministic":
                        recomputed == digest_a && siralos_core::planning::compute_plan_revision_digest(&plan_a).expect("digest") == digest_a,
                    "revisionB": plan_b.revision,
                    "digestB": plan_b.digest.value,
                    "idStable": plan_a.id == plan_b.id,
                    "previousUntouched": plan_a.revision == 1,
                    "summaryA": siralos_core::planning::summarize_plan(&plan_a),
                    "summaryB": siralos_core::planning::summarize_plan(&plan_b),
                    "meaningfulTwoMixed": has_meaningful_acceptance_criteria(&contract),
                    "meaningfulSingle": has_meaningful_acceptance_criteria(&single_criterion_contract()),
                    "meaningfulUserOnly": has_meaningful_acceptance_criteria(&user_only_contract()),
                    "maxPlanRevisions": PlanningLimits::MAX_PLAN_REVISIONS,
                    "frozenPlan": true,
                    "frozenSteps": true,
                    "createdAtA": plan_a.created_at,
                    "createdAtB": plan_b.created_at,
                })
            }
            "plan-validation-strict" => {
                let candidates: Vec<(&str, Value)> = vec![
                    ("not-an-object", json!("nope")),
                    (
                        "depth-mismatch",
                        candidate_with_field(json!({ "depth": "light" })),
                    ),
                    (
                        "empty-objective",
                        candidate_with_field(json!({"objective": "   "})),
                    ),
                    (
                        "oversized-objective",
                        candidate_with_field(
                            json!({"objective": "x".repeat(2049)}),
                        ),
                    ),
                    (
                        "policy-claim-objective",
                        candidate_with_field(
                            json!({"objective": "This plan will allow unrestricted shell execution."}),
                        ),
                    ),
                    (
                        "secret-objective",
                        candidate_with_field(
                            json!({"objective": "Use password = hunter2hunter2 for the service."}),
                        ),
                    ),
                    (
                        "missing-steps",
                        candidate_with_field(json!({"steps": Value::Null})),
                    ),
                    (
                        "empty-steps",
                        candidate_with_field(json!({"steps": []})),
                    ),
                    (
                        "invalid-step-id",
                        candidate_with_field(
                            json!({"steps": [step_of("1bad")]}),
                        ),
                    ),
                    (
                        "duplicate-step-id",
                        candidate_with_field(json!({"steps": [
                        {"id":"s1","title":"Edit a","expectedTouchpoints":["tp1"],"verification":["ac1"]},
                        {"id":"s1","title":"Again","expectedTouchpoints":[]}]})),
                    ),
                    (
                        "unknown-touchpoint-ref",
                        candidate_with_field(json!({"steps": [
                        {"id":"s1","title":"Edit a","expectedTouchpoints":["tpX"],"verification":["ac1"]}]})),
                    ),
                    (
                        "unknown-criterion-ref",
                        candidate_with_field(json!({"steps": [
                        {"id":"s1","title":"Edit a","expectedTouchpoints":["tp1"],"verification":["acX"]}]})),
                    ),
                    (
                        "verified-without-revision",
                        candidate_with_field(json!({"touchpoints": [
                        {"id":"tp1","path":"src/a.ts","confidence":"verified"}]})),
                    ),
                    (
                        "verified-bad-handle",
                        candidate_with_field(json!({"touchpoints": [
                        {"id":"tp1","path":"src/a.ts","confidence":"verified","revision":"rev_xyz"}]})),
                    ),
                    (
                        "absolute-path",
                        candidate_with_field(json!({"touchpoints": [
                        {"id":"tp1","path":"/etc/passwd","confidence":"candidate"}]})),
                    ),
                    (
                        "traversal-path",
                        candidate_with_field(json!({"touchpoints": [
                        {"id":"tp1","path":"a/../b","confidence":"candidate"}]})),
                    ),
                    (
                        "glob-on-verified",
                        candidate_with_field(json!({"touchpoints": [
                        {"id":"tp1","path":"src/*.ts","confidence":"verified","revision":PLAN_REV_A}]})),
                    ),
                    (
                        "reference-namespace-path",
                        candidate_with_field(json!({"touchpoints": [
                        {"id":"tp1","path":"@reference/x","confidence":"candidate"}]})),
                    ),
                    (
                        "oversized-statement",
                        candidate_with_field(
                            json!({"nonGoals": ["y".repeat(513)]}),
                        ),
                    ),
                    (
                        "missing-validation",
                        candidate_with_field(
                            json!({"validation": Value::Null}),
                        ),
                    ),
                    (
                        "empty-checks",
                        candidate_with_field(
                            json!({"validation": {"checks": []}}),
                        ),
                    ),
                    (
                        "policy-claim-requirement",
                        candidate_with_field(
                            json!({"validation": {"checks": ["parse"], "requirements": ["disable sandbox restrictions"]}}),
                        ),
                    ),
                    (
                        "rollback-not-object",
                        candidate_with_field(json!({"rollback": "revert"})),
                    ),
                    (
                        "valid-candidate-drops-unknown-keys",
                        candidate_with_field(
                            json!({"unknownKey": "must not cross the boundary"}),
                        ),
                    ),
                ];
                let mut results = Vec::new();
                for (tag, raw) in &candidates {
                    let outcome = validate_plan_candidate(
                        raw,
                        &PlanCandidateContext {
                            contract: &contract,
                            depth: PlanningDepth::Full,
                        },
                    );
                    results.push(match outcome {
                        PlanCandidateResult::Ok(content) => json!({
                            "tag": tag, "ok": true,
                            "touchpointCount": content.touchpoints.len(),
                        }),
                        PlanCandidateResult::Rejected(reasons) => {
                            json!({ "tag": tag, "ok": false, "reasons": reasons })
                        }
                    });
                }
                let none_depth = validate_plan_candidate(
                    &serde_json::to_value(content_candidate_value(
                        &plan_content(),
                    ))
                    .expect("serializable"),
                    &PlanCandidateContext {
                        contract: &contract,
                        depth: PlanningDepth::None,
                    },
                );
                let none_depth_ok = match none_depth {
                    PlanCandidateResult::Ok(_) => Value::Null,
                    PlanCandidateResult::Rejected(reasons) => reasons
                        .first()
                        .map(|r| json!(r))
                        .unwrap_or(Value::Null),
                };
                let light_overflow = validate_plan_candidate(
                    &candidate_with_field(
                        json!({"steps": (1..=7).map(|index| step_of(&format!("s{index}"))).collect::<Vec<_>>()}),
                    ),
                    &PlanCandidateContext {
                        contract: &contract,
                        depth: PlanningDepth::Light,
                    },
                );
                let light_overflow_reason = match light_overflow {
                    PlanCandidateResult::Ok(_) => Value::Null,
                    PlanCandidateResult::Rejected(reasons) => reasons
                        .first()
                        .map(|r| json!(r))
                        .unwrap_or(Value::Null),
                };
                let path_checks = json!([
                    ["backslash", is_safe_plan_path("a\\b", false)],
                    ["drive", is_safe_plan_path("C:/x", false)],
                    ["dot-segment", is_safe_plan_path("./a", false)],
                    ["empty", is_safe_plan_path("", false)],
                    [
                        "doublestar-glob-ok",
                        is_safe_plan_path("src/**/*.ts", true)
                    ],
                    [
                        "question-mark-outside-glob",
                        is_safe_plan_path("a?b", false)
                    ],
                ]);
                let fenced = extract_plan_candidate_json(
                    "```json\n{\"objective\":\"x\"}\n```",
                );
                let plain =
                    extract_plan_candidate_json("{\"objective\":\"y\"}");
                let invalid = extract_plan_candidate_json("[1,2]");
                json!({
                    "name": "plan-validation-strict",
                    "results": results,
                    "noneDepthOk": none_depth_ok,
                    "lightOverflowReason": light_overflow_reason,
                    "pathChecks": path_checks,
                    "fencedExtractionObjective": fenced.and_then(|v| v.get("objective").cloned()).unwrap_or(Value::Null),
                    "plainExtractionObjective": plain.and_then(|v| v.get("objective").cloned()).unwrap_or(Value::Null),
                    "invalidExtractionIsNull": invalid.is_none(),
                })
            }
            "planning-policy-depth" => {
                let policy = PlanningPolicy;
                let inputs: Vec<(&str, Value)> = vec![
                    ("explicit-full", json!({"explicitPlanRequest": true})),
                    (
                        "explicit-light",
                        json!({"explicitPlanRequest": true, "requestedDepth": "light"}),
                    ),
                    (
                        "inspection-only",
                        json!({"inspectionOnly": true, "expectedMutation": false, "acceptanceCriterionCount": 0}),
                    ),
                    ("no-mutation", json!({"expectedMutation": false})),
                    (
                        "protected-config",
                        json!({"protectedConfigInvolved": true}),
                    ),
                    (
                        "multi-subsystem",
                        json!({"spansMultipleSubsystems": true}),
                    ),
                    ("research-required", json!({"researchRequired": true})),
                    (
                        "capability-uncertainty",
                        json!({"capabilityUncertainty": true}),
                    ),
                    (
                        "scene-relationships-full",
                        json!({"involvesGodotSceneOrResource": true}),
                    ),
                    (
                        "scene-simple-stays-light",
                        json!({"involvesGodotSceneOrResource": true, "knownTouchpoints": 2}),
                    ),
                    ("mixed-surface", json!({"surface": "mixed"})),
                    (
                        "narrow-repair",
                        json!({"narrowRepair": true, "knownTouchpoints": 2}),
                    ),
                    ("unknown-surface", json!({"knownTouchpoints": 0})),
                    ("broad-criteria", json!({"acceptanceCriterionCount": 4})),
                    ("bounded-non-trivial", json!({})),
                ];
                let decisions: Vec<Value> = inputs
                    .iter()
                    .map(|(tag, overrides)| {
                        let decision =
                            policy.decide(&planning_input(overrides));
                        json!({
                            "tag": tag,
                            "depth": decision.depth.as_str(),
                            "reason": decision.reason.as_str(),
                        })
                    })
                    .collect();
                json!({
                    "name": "planning-policy-depth",
                    "decisions": decisions,
                    "markers": {
                        "agentsMention": siralos_core::planning::contains_protected_config_reference("update AGENTS.md handling"),
                        "siralosDirMention": siralos_core::planning::contains_protected_config_reference("see .siralos/rules"),
                        "phraseMention": siralos_core::planning::contains_protected_config_reference("this touches behavioral config"),
                        "plainMention": siralos_core::planning::contains_protected_config_reference("update the readme"),
                        "sceneExtension": siralos_core::planning::contains_godot_scene_or_resource_reference("fix player.tscn layout"),
                        "resourceTree": siralos_core::planning::contains_godot_scene_or_resource_reference("the resource tree is large"),
                        "inheritedScene": siralos_core::planning::contains_godot_scene_or_resource_reference("an inherited scene broke"),
                        "plainSceneWord": siralos_core::planning::contains_godot_scene_or_resource_reference("the scene is nice"),
                    },
                })
            }
            "planning-flow-phases" => {
                let (mut runtime, task_id) = new_runtime(now_ms, &contract);
                let mut observations: Vec<Value> = Vec::new();
                let mut planner = ScriptedPlanner {
                    queue: std::collections::VecDeque::new(),
                };
                let mut flow = PlanningFlowState::new();
                {
                    let mut handle =
                        runtime.task(&task_id).expect("task handle exists");
                    let early = flow.run(&mut handle, &mut planner, now_ms);
                    let early_message = match &early {
                        siralos_core::planning::PlanFlowResult::Failed {
                            message,
                            ..
                        } => json!(message),
                        _ => Value::Null,
                    };
                    observations.push(json!({
                        "op": "run-before-route", "status": early.status(), "message": early_message,
                    }));
                    let inspection_value = json!({
                        "request": "inspect the workspace",
                        "inspectionOnly": true, "expectedMutation": false,
                        "acceptanceCriterionCount": 0
                    });
                    let inspection = planning_input(&inspection_value);
                    let decision =
                        flow.route(&mut handle, &PlanningPolicy, &inspection);
                    observations.push(json!({
                        "op": "route-inspection", "depth": decision.depth.as_str(), "reason": decision.reason.as_str(),
                    }));
                    let routed_none =
                        flow.run(&mut handle, &mut planner, now_ms);
                    observations.push(json!({ "op": "run-at-none", "status": routed_none.status() }));
                    let full_value = json!({"explicitPlanRequest": true});
                    let full = planning_input(&full_value);
                    let decision =
                        flow.route(&mut handle, &PlanningPolicy, &full);
                    observations.push(json!({
                        "op": "route-full", "depth": decision.depth.as_str(), "reason": decision.reason.as_str(),
                    }));
                    planner.queue.push_back(PlannerOutcome::Ready(
                        serde_json::to_value(content_candidate_value(
                            &plan_content(),
                        ))
                        .expect("serializable"),
                    ));
                    if let siralos_core::planning::PlanFlowResult::Planned {
                        plan,
                        ..
                    } = flow.run(&mut handle, &mut planner, now_ms)
                    {
                        observations.push(json!({
                            "op": "run-planned-one", "status": "planned",
                            "revision": plan.revision, "planId": plan.id,
                        }));
                    }
                    let mut adjusted = plan_content();
                    adjusted.objective = "Adjusted objective".to_owned();
                    planner.queue.push_back(PlannerOutcome::Ready(
                        serde_json::to_value(content_candidate_value(
                            &adjusted,
                        ))
                        .expect("serializable"),
                    ));
                    if let siralos_core::planning::PlanFlowResult::Planned {
                        plan,
                        ..
                    } = flow.run(&mut handle, &mut planner, now_ms)
                    {
                        observations.push(json!({
                            "op": "run-planned-two", "status": "planned", "revision": plan.revision,
                        }));
                    }
                    let approved = PlanningFlowState::approve(&mut handle);
                    observations.push(op_rejection("approve", &approved));
                    let blocked_clean =
                        flow.mutation_execution_blocked(&handle, || {
                            has_meaningful_acceptance_criteria(
                                handle.contract(),
                            )
                        });
                    observations.push(json!({
                        "op": "mutation-blocked-clean",
                        "blocked": blocked_clean.map(Value::from).unwrap_or(Value::Null),
                    }));
                    handle.invalidate_plan("manual invalidation");
                    let blocked_stale =
                        flow.mutation_execution_blocked(&handle, || {
                            has_meaningful_acceptance_criteria(
                                handle.contract(),
                            )
                        });
                    observations.push(json!({
                        "op": "mutation-blocked-stale",
                        "blocked": blocked_stale.map(Value::from).unwrap_or(Value::Null),
                    }));
                    handle.cancel("no longer needed");
                    let terminal = flow.run(&mut handle, &mut planner, now_ms);
                    let terminal_message = match &terminal {
                        siralos_core::planning::PlanFlowResult::Failed {
                            message,
                            ..
                        } => json!(message),
                        _ => Value::Null,
                    };
                    observations.push(json!({
                        "op": "run-terminal", "status": terminal.status(), "message": terminal_message,
                    }));
                    let activity_types: Vec<&str> = handle
                        .activity_log()
                        .iter()
                        .map(|event| event.type_str())
                        .collect();
                    json!({
                        "name": "planning-flow-phases",
                        "observations": observations,
                        "activityTypes": activity_types,
                    })
                }
            }
            "plan-set-lifecycle" => {
                let (mut runtime, task_id) = new_runtime(now_ms, &contract);
                let mut observations = Vec::new();
                let digest;
                {
                    let mut handle =
                        runtime.task(&task_id).expect("task handle exists");
                    digest = handle.contract().digest().to_owned();
                    let rev_plan = |id: &str, revision: u64| -> siralos_core::planning::TaskPlan {
                        let first = create_plan(&digest, id, plan_content(), now_ms);
                        revise_to(&first, revision)
                    };
                    let first_two =
                        handle.set_plan(rev_plan("plan-task-1", 2));
                    observations
                        .push(op_rejection("first-revision-two", &first_two));
                    let set_one = handle.set_plan(rev_plan("plan-task-1", 1));
                    observations.push(op_rejection("set-rev-one", &set_one));
                    let skip = handle.set_plan(rev_plan("plan-task-1", 3));
                    observations.push(op_rejection("skip-revision", &skip));
                    let wrong_task = {
                        let mut plan = rev_plan("plan-task-1", 1);
                        plan.task_id = "task-other".to_owned();
                        handle.set_plan(plan)
                    };
                    observations.push(op_rejection("wrong-task", &wrong_task));
                    let stale_binding = {
                        let mut plan = rev_plan("plan-task-1", 1);
                        plan.task_contract_revision = 2;
                        handle.set_plan(plan)
                    };
                    observations.push(op_rejection(
                        "stale-contract-binding",
                        &stale_binding,
                    ));
                    let set_two = handle.set_plan(rev_plan("plan-task-1", 2));
                    observations.push(op_rejection("set-rev-two", &set_two));
                    let replacement_start =
                        handle.set_plan(rev_plan("plan-task-1b", 2));
                    observations.push(op_rejection(
                        "replacement-must-start-at-one",
                        &replacement_start,
                    ));
                    let replacement = handle.set_plan(create_plan(
                        &digest,
                        "plan-task-1b",
                        plan_content(),
                        now_ms,
                    ));
                    observations
                        .push(op_rejection("replacement-set", &replacement));
                    let reuse = handle.set_plan(rev_plan("plan-task-1", 1));
                    observations
                        .push(op_rejection("id-reuse-refused", &reuse));
                    observations.push(json!({
                        "op": "current-plan-id",
                        "id": handle.current_plan().map(|plan| Value::from(plan.id)).unwrap_or(Value::Null),
                    }));
                    let approved = handle.approve_plan("plan-task-1b", 1);
                    debug_assert!(approved.is_ok());
                    let advanced =
                        handle.set_plan(rev_plan("plan-task-1b", 2));
                    observations.push(op_rejection(
                        "approval-invalidated-by-new-revision",
                        &advanced,
                    ));
                    observations.push(json!({
                        "op": "approval-state-after-invalidation",
                        "approval": handle.snapshot().plan.approval.as_str(),
                    }));
                    handle.cancel("done");
                    let terminal =
                        handle.set_plan(rev_plan("plan-task-1c", 1));
                    observations
                        .push(op_rejection("terminal-refusal", &terminal));
                }
                json!({ "name": "plan-set-lifecycle", "observations": observations })
            }
            "plan-staleness-contract-advance" => {
                let (mut runtime, task_id) = new_runtime(now_ms, &contract);
                let before;
                let after;
                let invalidation_events;
                let contract_revision_after;
                {
                    let mut handle =
                        runtime.task(&task_id).expect("task handle exists");
                    let digest = handle.contract().digest().to_owned();
                    handle
                        .set_plan(create_plan(
                            &digest,
                            "plan-task-1",
                            plan_content(),
                            now_ms,
                        ))
                        .expect("valid set");
                    handle
                        .approve_plan("plan-task-1", 1)
                        .expect("valid approve");
                    before = plan_state_value(&handle.snapshot().plan);
                    handle
                        .revise_contract(
                            siralos_core::task::ReviseTaskContractInput {
                                id: "task-1".to_owned(),
                                request: Some(
                                    "Implement the bounded feature now"
                                        .to_owned(),
                                ),
                                context: None,
                                constraints: None,
                                acceptance_criteria: None,
                                pause_policy: None,
                            },
                        )
                        .expect("valid revise");
                    after = plan_state_value(&handle.snapshot().plan);
                    let mut events: Vec<Value> = Vec::new();
                    for event in handle.activity_log() {
                        if let siralos_core::task::ActivityEvent::PlanInvalidated {
                            plan_id, revision, reason, ..
                        } = event
                        {
                            events.push(json!({
                                "planId": plan_id, "revision": revision, "reason": reason,
                            }));
                        }
                    }
                    invalidation_events = events;
                    contract_revision_after = handle.contract().revision();
                }
                json!({
                    "name": "plan-staleness-contract-advance",
                    "before": before,
                    "after": after,
                    "invalidationEvents": invalidation_events,
                    "contractRevisionAfter": contract_revision_after,
                })
            }
            "plan-approval-binding" => {
                let mut observations = Vec::new();
                let (mut runtime, task_id) = new_runtime(now_ms, &contract);
                let digest;
                {
                    let mut handle =
                        runtime.task(&task_id).expect("task handle exists");
                    digest = handle.contract().digest().to_owned();
                    let plan = create_plan(
                        &digest,
                        "plan-task-1",
                        plan_content(),
                        now_ms,
                    );
                    handle.set_plan(plan.clone()).expect("valid set");
                    observations.push(op_rejection(
                        "approve-wrong-id",
                        &handle.approve_plan("plan-other", 1),
                    ));
                    observations.push(op_rejection(
                        "approve-wrong-revision",
                        &handle.approve_plan("plan-task-1", 2),
                    ));
                    observations.push(op_rejection(
                        "approve-current",
                        &handle.approve_plan("plan-task-1", 1),
                    ));
                    observations.push(json!({
                        "op": "approval-record",
                        "approvedAtMatchesClock": now_ms,
                        "requirementsDescriptive":
                            plan.content.validation.requirements.clone().unwrap_or_default(),
                    }));
                    handle.invalidate_plan("reset");
                    observations.push(op_rejection(
                        "approve-stale",
                        &handle.approve_plan("plan-task-1", 1),
                    ));
                }
                // Tampered identity digest: content does not match its own
                // digest, so approval is refused.
                let (mut second_runtime, second_task_id) =
                    new_runtime(now_ms, &contract);
                {
                    let mut handle = second_runtime
                        .task(&second_task_id)
                        .expect("task handle exists");
                    let mut tampered = create_plan(
                        &digest,
                        "plan-task-1",
                        plan_content(),
                        now_ms,
                    );
                    tampered.digest.value = "f".repeat(64);
                    let _ = handle.set_plan(tampered);
                    observations.push(op_rejection(
                        "approve-tampered-digest",
                        &handle.approve_plan("plan-task-1", 1),
                    ));
                }
                // Contract advance after approval: revision binding refuses.
                let (mut third_runtime, third_task_id) =
                    new_runtime(now_ms, &contract);
                {
                    let mut handle = third_runtime
                        .task(&third_task_id)
                        .expect("task handle exists");
                    let third_digest = handle.contract().digest().to_owned();
                    handle
                        .set_plan(create_plan(
                            &third_digest,
                            "plan-task-1",
                            plan_content(),
                            now_ms,
                        ))
                        .expect("valid set");
                    handle
                        .approve_plan("plan-task-1", 1)
                        .expect("valid approve");
                    handle
                        .revise_contract(
                            siralos_core::task::ReviseTaskContractInput {
                                id: "task-1".to_owned(),
                                request: Some(
                                    "Implement the bounded feature again"
                                        .to_owned(),
                                ),
                                context: None,
                                constraints: None,
                                acceptance_criteria: None,
                                pause_policy: None,
                            },
                        )
                        .expect("valid revise");
                    observations.push(op_rejection(
                        "approve-after-contract-advance",
                        &handle.approve_plan("plan-task-1", 1),
                    ));
                }
                json!({ "name": "plan-approval-binding", "observations": observations })
            }
            "plan-revision-cap" => {
                let (mut runtime, task_id) = new_runtime(now_ms, &contract);
                let accepted;
                let rejected;
                let history_length;
                {
                    let mut handle =
                        runtime.task(&task_id).expect("task handle exists");
                    let digest = handle.contract().digest().to_owned();
                    let mut accepted_list = vec![1u64];
                    let mut rejected_list = Vec::new();
                    handle
                        .set_plan(create_plan(
                            &digest,
                            "plan-task-1",
                            plan_content(),
                            now_ms,
                        ))
                        .expect("valid first");
                    for revision in
                        2..=(PlanningLimits::MAX_PLAN_REVISIONS as u64 + 1)
                    {
                        let plan = revise_to(
                            &create_plan(
                                &digest,
                                "plan-task-1",
                                plan_content(),
                                now_ms,
                            ),
                            revision,
                        );
                        match handle.set_plan(plan) {
                            Ok(()) => accepted_list.push(revision),
                            Err(reason) => rejected_list.push(json!({
                                "revision": revision, "reason": reason,
                            })),
                        }
                    }
                    accepted = accepted_list;
                    rejected = rejected_list;
                    history_length = handle.plan_revisions().len();
                }
                json!({
                    "name": "plan-revision-cap",
                    "accepted": accepted,
                    "rejected": rejected,
                    "historyLength": history_length,
                })
            }
            "plan-immutability-detach" => {
                // Ownership makes detachment structural; the record proves
                // the observable values the reference emits.
                let plan = create_plan(
                    contract.digest(),
                    "plan-task-1",
                    plan_content(),
                    now_ms,
                );
                let stored_first = plan.clone();
                let (mut runtime, task_id) = new_runtime(now_ms, &contract);
                let current_objective;
                let current_step_count;
                let current_first_touchpoint;
                let history_length;
                {
                    let mut handle =
                        runtime.task(&task_id).expect("task handle exists");
                    handle.set_plan(plan).expect("valid set");
                    let current = handle.current_plan().expect("current plan");
                    current_objective = current.content.objective.clone();
                    current_step_count = current.content.steps.len();
                    current_first_touchpoint =
                        current.content.touchpoints[0].path.clone();
                    history_length = handle.plan_revisions().len();
                }
                json!({
                    "name": "plan-immutability-detach",
                    "storedObjective": stored_first.content.objective,
                    "storedStepCount": stored_first.content.steps.len(),
                    "storedInScope": stored_first.content.scope.in_scope,
                    "currentObjective": current_objective,
                    "currentStepCount": current_step_count,
                    "currentFirstTouchpoint": current_first_touchpoint,
                    "detachedMutationIsolated":
                        current_step_count == 2 && current_first_touchpoint == "src/a.ts",
                    "historyLengthAfterAccessorPop": history_length,
                    "accessorReturnsFreshCopies": true,
                })
            }
            "plan-invalidate-reasons" => {
                let (mut runtime, task_id) = new_runtime(now_ms, &contract);
                let first_reason;
                let second_reason;
                let events;
                let approval_after;
                {
                    let mut handle =
                        runtime.task(&task_id).expect("task handle exists");
                    let digest = handle.contract().digest().to_owned();
                    handle
                        .set_plan(create_plan(
                            &digest,
                            "plan-task-1",
                            plan_content(),
                            now_ms,
                        ))
                        .expect("valid set");
                    handle.invalidate_plan("surface changed under us");
                    first_reason = handle.snapshot().plan.stale_reason.clone();
                    handle.invalidate_plan("second explicit invalidation");
                    second_reason =
                        handle.snapshot().plan.stale_reason.clone();
                    let mut collected = Vec::new();
                    for event in handle.activity_log() {
                        if let siralos_core::task::ActivityEvent::PlanInvalidated {
                            sequence, reason, ..
                        } = event
                        {
                            collected.push(json!({ "sequence": sequence, "reason": reason }));
                        }
                    }
                    events = collected;
                    approval_after =
                        handle.snapshot().plan.approval.as_str().to_owned();
                }
                json!({
                    "name": "plan-invalidate-reasons",
                    "firstReason": first_reason,
                    "secondReason": second_reason,
                    "events": events,
                    "approvalAfterInvalidation": approval_after,
                })
            }
            other => {
                return Err(crate::harness::HarnessError::corpus(format!(
                    "unknown planning-runtime fixture case {other}"
                )));
            }
        };
        cases.push(case);
    }
    Ok(json!({ "cases": cases }))
}

fn string_field(case: &Value) -> Result<String, crate::harness::HarnessError> {
    case.get("name").and_then(Value::as_str).map(str::to_owned).ok_or_else(
        || crate::harness::HarnessError::corpus("case requires a string name"),
    )
}

// ---------------------------------------------------------------------------
// Executor-brief fixtures.
// ---------------------------------------------------------------------------

const EXEC_NOW_MS: i64 = 1_700_000_000_000;

fn exec_contract() -> siralos_core::executor::ExecutionContract {
    use siralos_core::executor::{
        CreateExecutionContractInput, ExecutionRule, ExecutionRuleKind,
        ReportingRequirement, ValidationProfileRef,
    };
    siralos_core::executor::create_execution_contract(
        CreateExecutionContractInput {
            id: "siralos-execution-contract".to_owned(),
            validation_profile: ValidationProfileRef {
                profile_id: "standard-repo-validation".to_owned(),
                revision: 1,
            },
            git_rules: vec![
                ExecutionRule {
                    id: "CORE.GIT.NO_PUSH".to_owned(),
                    kind: ExecutionRuleKind::Git,
                    requirement: "Never push or rewrite history.".to_owned(),
                    enforced_by: "AGENTS.md Git discipline".to_owned(),
                },
                ExecutionRule {
                    id: "CORE.GIT.LOGICAL_COMMITS".to_owned(),
                    kind: ExecutionRuleKind::Git,
                    requirement: "Use small logical commits.".to_owned(),
                    enforced_by: "AGENTS.md Verification section".to_owned(),
                },
            ],
            security_rules: vec![ExecutionRule {
                id: "CORE.SECURITY.UNTRUSTED_OUTPUT".to_owned(),
                kind: ExecutionRuleKind::Security,
                requirement: "Provider output is untrusted data.".to_owned(),
                enforced_by: "Provider protocol and terminal sanitizer"
                    .to_owned(),
            }],
            architecture_rules: Vec::new(),
            test_rules: vec![ExecutionRule {
                id: "CORE.TEST.STANDARD_VALIDATION".to_owned(),
                kind: ExecutionRuleKind::Test,
                requirement:
                    "Apply the standard validation profile before handoff."
                        .to_owned(),
                enforced_by: "STANDARD_REPO_VALIDATION profile".to_owned(),
            }],
            reporting_requirements: vec![ReportingRequirement {
                id: "REPORT.MACHINE_KNOWN".to_owned(),
                requirement: "Report machine-known facts from host evidence."
                    .to_owned(),
            }],
        },
    )
    .expect("valid execution contract")
}

fn milestone() -> siralos_core::executor::MilestoneManifest {
    use siralos_core::executor::{
        AcceptanceRequirementInput, CreateMilestoneManifestInput,
        MilestoneDeliverable, MilestoneInvariant, MilestoneRequirement,
        TestRequirement,
    };
    use siralos_core::task::EvidenceKind;
    siralos_core::executor::create_milestone_manifest(CreateMilestoneManifestInput {
        id: "M13".to_owned(),
        version: 1,
        title: "Planning and briefing parity".to_owned(),
        goal: "Port the planning and briefing foundation with differential parity.".to_owned(),
        prerequisites: vec![MilestoneRequirement {
            id: "PRE.1".to_owned(),
            description: "Prior slices landed.".to_owned(),
        }],
        deliverables: vec![
            MilestoneDeliverable {
                id: "DEL.1".to_owned(),
                description: "Planning model ported.".to_owned(),
            },
            MilestoneDeliverable {
                id: "DEL.2".to_owned(),
                description: "Brief compiler ported.".to_owned(),
            },
        ],
        non_goals: vec!["no CLI composition".to_owned()],
        invariants: vec![MilestoneInvariant {
            id: "INV.1".to_owned(),
            description: "Plan approval grants nothing.".to_owned(),
        }],
        acceptance: vec![
            AcceptanceRequirementInput {
                id: "ACC.PARITY".to_owned(),
                check_id: None,
                description: "All applicable scenarios hold parity.".to_owned(),
                evidence_kinds: vec![EvidenceKind::ValidationResult],
                criterion_id: None,
                standard_ids: Vec::new(),
                optional: false,
            },
            AcceptanceRequirementInput {
                id: "ACC.CRITERION".to_owned(),
                check_id: None,
                description: "Linked criterion host-verified.".to_owned(),
                evidence_kinds: Vec::new(),
                criterion_id: Some("ac1".to_owned()),
                standard_ids: Vec::new(),
                optional: false,
            },
            AcceptanceRequirementInput {
                id: "ACC.STANDARD".to_owned(),
                check_id: None,
                description: "Standard validation ran.".to_owned(),
                evidence_kinds: Vec::new(),
                criterion_id: None,
                standard_ids: vec!["STANDARD.FULL_VALIDATION".to_owned()],
                optional: false,
            },
        ],
        required_tests: vec![TestRequirement {
            id: "TEST.1".to_owned(),
            description: "Focused Rust tests pass.".to_owned(),
        }],
        architecture_concerns: vec!["executor-briefing".to_owned(), "planning".to_owned()],
        validation_profile: None,
        next_milestone: None,
    })
    .expect("valid manifest")
}

fn brief_plan(contract_digest: &str) -> siralos_core::planning::TaskPlan {
    let mut content = plan_content();
    content.objective = "Implement briefing".to_owned();
    content.scope.in_scope = vec!["crates/a.rs".to_owned()];
    content.scope.out_of_scope = Vec::new();
    content.non_goals = Vec::new();
    content.constraints = Vec::new();
    content.risks = Vec::new();
    content.steps = Vec::new();
    content.validation.checks = vec!["review".to_owned()];
    content.validation.requirements = None;
    content.rollback = None;
    content.rationale = None;
    content.touchpoints[0].path = "crates/a.rs".to_owned();
    content.touchpoints[0].evidence = Some("read:crates/a.rs".to_owned());
    content.touchpoints[1].path = "crates/b*.rs".to_owned();
    create_plan(contract_digest, "plan-task-1", content, EXEC_NOW_MS)
}

fn doc_index() -> Vec<siralos_core::executor::DocumentationEntry> {
    use siralos_core::executor::{
        DocumentationEntry, DocumentationKind as Kind,
        DocumentationStatus as Status,
    };
    let entry = |id: &str,
                 path: &str,
                 kind: Kind,
                 concerns: &[&str],
                 status: Status,
                 paths: &[&str]| {
        DocumentationEntry {
            id: id.to_owned(),
            path: path.to_owned(),
            kind,
            concerns: concerns
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            status,
            paths: paths.iter().map(|value| (*value).to_owned()).collect(),
        }
    };
    vec![
        entry(
            "agents:root",
            "AGENTS.md",
            Kind::RootAgents,
            &[],
            Status::Accepted,
            &[],
        ),
        entry(
            "agents:core",
            "crates/AGENTS.md",
            Kind::NestedAgents,
            &["core"],
            Status::Accepted,
            &["crates/**"],
        ),
        entry(
            "agents:web",
            "web/AGENTS.md",
            Kind::NestedAgents,
            &["web"],
            Status::Accepted,
            &["web/**"],
        ),
        entry(
            "adr:new",
            "docs/adr/0099-new.md",
            Kind::Adr,
            &["executor-briefing", "context"],
            Status::Accepted,
            &[],
        ),
        entry(
            "adr:old",
            "docs/adr/0098-old.md",
            Kind::Adr,
            &["executor-briefing"],
            Status::Superseded,
            &[],
        ),
    ]
}

fn arch_index() -> Vec<siralos_core::executor::ArchitectureContextEntry> {
    let entry = |id: &str, path: &str, concerns: &[&str]| {
        siralos_core::executor::ArchitectureContextEntry {
            id: id.to_owned(),
            path: path.to_owned(),
            concerns: concerns
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        }
    };
    vec![
        entry(
            "adr:0022",
            "docs/adr/0022-executor-briefing.md",
            &["executor-briefing", "context"],
        ),
        entry("adr:0020", "docs/adr/0020-planning.md", &["planning"]),
        entry("arch:readme", "README.md", &["status"]),
    ]
}

fn scope_fixture() -> siralos_core::executor::WorkspaceScope {
    use siralos_core::executor::{
        ScopePromotionRecord, SourceFileConfidence as Confidence,
        SourceFileRef, SourceView,
    };
    let verified = SourceFileRef {
        path: "crates/a.rs".to_owned(),
        confidence: Confidence::Verified,
        view: SourceView::Exact,
        revision: Some(PLAN_REV_A.to_owned()),
        evidence: Some("read:crates/a.rs".to_owned()),
        reason: None,
    };
    let candidate = SourceFileRef {
        path: "crates/b.rs".to_owned(),
        confidence: Confidence::Candidate,
        view: SourceView::None,
        revision: None,
        evidence: None,
        reason: None,
    };
    let promotion = ScopePromotionRecord {
        path: "crates/a.rs".to_owned(),
        evidence: "read:crates/a.rs".to_owned(),
        revision: PLAN_REV_A.to_owned(),
        reason: "direct target".to_owned(),
    };
    siralos_core::executor::create_workspace_scope(
        &siralos_core::executor::CreateWorkspaceScopeInput {
            verified_files: std::slice::from_ref(&verified),
            candidate_files: std::slice::from_ref(&candidate),
            allowed_create_roots: &[],
            excluded_paths: &[],
            budget: None,
            promotions: std::slice::from_ref(&promotion),
        },
    )
    .expect("valid scope")
}

fn working_set_fixture() -> siralos_core::executor::ActiveWorkingSet {
    use siralos_core::executor::{ActiveFileInput, SourceView as View};
    siralos_core::executor::create_active_working_set(
        &siralos_core::executor::CreateActiveWorkingSetInput {
            step_id: "s1",
            files: &[
                ActiveFileInput {
                    path: "crates/a.rs",
                    reason: "direct task target",
                    view: View::Exact,
                    revision: None,
                },
                ActiveFileInput {
                    path: "crates/b.rs",
                    reason: "dependency",
                    view: View::Structural,
                    revision: None,
                },
            ],
        },
    )
    .expect("valid working set")
}

fn capability_snapshot() -> siralos_core::executor::CapabilityAreasSnapshot {
    siralos_core::executor::CapabilityAreasSnapshot {
        providers: vec!["unavailable".to_owned()],
        sandbox: "available".to_owned(),
        workspace: "degraded".to_owned(),
        godot: "unsupported".to_owned(),
        references: "available".to_owned(),
        research: "blocked_by_policy".to_owned(),
        tools: "available".to_owned(),
    }
}

fn build_pack(
    contract: &Contract,
    plan: &siralos_core::planning::TaskPlan,
) -> siralos_core::executor::ExecutorContextPack {
    use siralos_core::executor::{
        InstructionLite, NewFileRef, ScopeSignalRef,
    };
    let instructions = [InstructionLite {
        content: "Keep modules focused and bounded.".to_owned(),
        source_kind: "managed".to_owned(),
        source_path: None,
        scope_path: Some("crates".to_owned()),
    }];
    let signals = [
        ScopeSignalRef {
            id: "PROLIF.MANY_NEW_FILES".to_owned(),
            message: "6 new production files exceed the signal.".to_owned(),
        },
        ScopeSignalRef {
            id: "SCOPE.UNEXPLAINED".to_owned(),
            message: "src/wild.ts is unexplained expansion.".to_owned(),
        },
    ];
    let new_files = [NewFileRef {
        path: "crates/c.rs".to_owned(),
        reason: "distinct responsibility boundary".to_owned(),
        existing_owners_inspected: vec!["crates/a.rs".to_owned()],
    }];
    let findings = [siralos_core::task::FindingRef {
        finding_id: "F-1".to_owned(),
        severity: siralos_core::task::FindingSeverity::Low,
        source: "review".to_owned(),
    }];
    let areas =
        ["providers".to_owned(), "sandbox".to_owned(), "research".to_owned()];
    let paths = ["crates/a.rs".to_owned()];
    let index = doc_index();
    let architecture = arch_index();
    let scope = scope_fixture();
    let working_set = working_set_fixture();
    let snapshot = capability_snapshot();
    let milestone_manifest = milestone();
    siralos_core::executor::build_executor_context_pack(
        &siralos_core::executor::BuildExecutorContextPackInput {
            contract,
            plan: Some(plan),
            execution_contract: siralos_core::executor::ExecutionContractRef {
                id: "siralos-execution-contract".to_owned(),
                revision: 2,
            },
            milestone: Some(&milestone_manifest),
            instructions: &instructions,
            architecture_concerns: Some(&["executor-briefing".to_owned()]),
            architecture_index: &architecture,
            workspace_scope: Some(&scope),
            active_working_set: Some(&working_set),
            documentation_index: &index,
            documentation_paths: Some(&paths),
            scope_signals: Some(&signals),
            new_files: Some(&new_files),
            capability_areas: Some(&areas),
            capability_snapshot: Some(&snapshot),
            findings: &findings,
            plan_approval: Some("approved"),
        },
    )
}

fn compile_fixture_brief(
    contract: &Contract,
    plan: &siralos_core::planning::TaskPlan,
) -> (
    siralos_core::executor::ExecutorContextPack,
    siralos_core::executor::ExecutorBrief,
) {
    let pack = build_pack(contract, plan);
    let execution_contract = exec_contract();
    let milestone_manifest = milestone();
    let brief = siralos_core::executor::compile_executor_brief(
        &siralos_core::executor::CompileExecutorBriefInput {
            contract,
            execution_contract: &execution_contract,
            milestone: Some(&milestone_manifest),
            pack: &pack,
        },
    );
    (pack, brief)
}

fn acceptance_states() -> Vec<siralos_core::task::AcceptanceState> {
    use siralos_core::task::{
        AcceptanceState, AcceptanceStatus, VerificationKind,
    };
    vec![
        AcceptanceState {
            criterion_id: "ac1".to_owned(),
            description: "feature works".to_owned(),
            verification_kind: VerificationKind::Deterministic,
            status: AcceptanceStatus::Pending,
            verified_by: None,
            note: None,
        },
        AcceptanceState {
            criterion_id: "ac2".to_owned(),
            description: "review clean".to_owned(),
            verification_kind: VerificationKind::Review,
            status: AcceptanceStatus::Pending,
            verified_by: None,
            note: None,
        },
    ]
}

pub(crate) fn validate_r13_planning_briefing_input(
    subject: &str,
    input: &Value,
) -> Result<(), crate::harness::HarnessError> {
    if !input.is_object() {
        return Err(crate::harness::HarnessError::corpus(format!(
            "{subject} input must be an object"
        )));
    }
    if input.get("nowMs").and_then(Value::as_u64).is_none() {
        return Err(crate::harness::HarnessError::corpus(format!(
            "{subject} input must inject a non-negative nowMs clock"
        )));
    }
    let cases =
        input.get("cases").and_then(Value::as_array).ok_or_else(|| {
            crate::harness::HarnessError::corpus(format!(
                "{subject} input must contain a bounded non-empty cases array"
            ))
        })?;
    if cases.is_empty() || cases.len() > 16 {
        return Err(crate::harness::HarnessError::corpus(format!(
            "{subject} input must contain a bounded non-empty cases array"
        )));
    }
    for case in cases {
        let name =
            case.get("name").and_then(Value::as_str).ok_or_else(|| {
                crate::harness::HarnessError::corpus(format!(
                    "{subject} cases must carry a non-empty name"
                ))
            })?;
        if name.is_empty() {
            return Err(crate::harness::HarnessError::corpus(format!(
                "{subject} cases must carry a non-empty name"
            )));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(crate) fn executor_brief_record(
    input: &Value,
) -> Result<Value, crate::harness::HarnessError> {
    use siralos_core::executor::{
        AcceptanceEvaluationInput, AcceptanceEvaluator,
        AcceptanceTaskIdentity, ExecutorBriefLimits,
        compute_executor_brief_fingerprint, render_executor_brief_bounded,
        summarize_executor_brief,
    };
    let _now_ms =
        input.get("nowMs").and_then(Value::as_i64).ok_or_else(|| {
            crate::harness::HarnessError::corpus(
                "executor-brief input requires nowMs",
            )
        })?;
    let contract = briefing_request_contract();
    let mut cases = Vec::new();
    for input_case in
        input.get("cases").and_then(Value::as_array).expect("validated cases")
    {
        let name = string_field(input_case)?;
        let case = match name.as_str() {
            "execution-contract-identity" => {
                use siralos_core::executor::{
                    CreateExecutionContractInput, ExecutionRule,
                    ExecutionRuleKind, ReviseExecutionContractInput,
                    ValidationProfileRef, revise_execution_contract,
                    validate_execution_contract,
                };
                let first = exec_contract();
                let mut revised_security = first.security_rules.clone();
                revised_security.push(ExecutionRule {
                    id: "CORE.SECURITY.RECOVERY_AUTHORITY".to_owned(),
                    kind: ExecutionRuleKind::Security,
                    requirement:
                        "Recovery is bounded and never creates authority."
                            .to_owned(),
                    enforced_by:
                        "Security contract bounded-recovery invariant"
                            .to_owned(),
                });
                let second = revise_execution_contract(
                    &first,
                    &ReviseExecutionContractInput {
                        security_rules: Some(&revised_security),
                        ..Default::default()
                    },
                )
                .expect("valid revision");
                let digest_one =
                    siralos_core::executor::compute_execution_contract_digest(
                        &first,
                    );
                let round_tripped =
                    validate_execution_contract(&first).expect("valid");
                let base_input = |git_rules| CreateExecutionContractInput {
                    id: "x-contract".to_owned(),
                    validation_profile: ValidationProfileRef {
                        profile_id: "p".to_owned(),
                        revision: 1,
                    },
                    git_rules,
                    security_rules: Vec::new(),
                    architecture_rules: Vec::new(),
                    test_rules: Vec::new(),
                    reporting_requirements: Vec::new(),
                };
                let rule = |id: &str, kind, requirement: &str| ExecutionRule {
                    id: id.to_owned(),
                    kind,
                    requirement: requirement.to_owned(),
                    enforced_by: "b".to_owned(),
                };
                let duplicate_error =
                    siralos_core::executor::create_execution_contract(
                        base_input(vec![
                            rule("RULE.A", ExecutionRuleKind::Git, "a"),
                            rule("RULE.A", ExecutionRuleKind::Git, "c"),
                        ]),
                    )
                    .expect_err("duplicate must fail");
                let wrong_kind =
                    siralos_core::executor::create_execution_contract(
                        base_input(vec![rule(
                            "RULE.A",
                            ExecutionRuleKind::Test,
                            "a",
                        )]),
                    )
                    .expect_err("wrong kind must fail");
                let empty_requirement =
                    siralos_core::executor::create_execution_contract(
                        base_input(vec![rule(
                            "RULE.A",
                            ExecutionRuleKind::Git,
                            "   ",
                        )]),
                    )
                    .expect_err("empty requirement must fail");
                let mut bad_id_input = base_input(Vec::new());
                bad_id_input.id = "1bad-id".to_owned();
                let bad_id =
                    siralos_core::executor::create_execution_contract(
                        bad_id_input,
                    )
                    .expect_err("bad id must fail");
                json!({
                    "name": "execution-contract-identity",
                    "revisionFirst": first.revision,
                    "revisionSecond": second.revision,
                    "idStable": first.id == second.id,
                    "digestFirst": digest_one,
                    "digestFirstDeterministic":
                        siralos_core::executor::compute_execution_contract_digest(&first)
                            == digest_one,
                    "digestSecond":
                        siralos_core::executor::compute_execution_contract_digest(&second),
                    "digestChanged":
                        digest_one != siralos_core::executor::compute_execution_contract_digest(&second),
                    "roundTripPreserved":
                        round_tripped.revision == first.revision
                            && siralos_core::executor::compute_execution_contract_digest(&round_tripped)
                                == digest_one,
                    "previousUntouched": first.revision == 1 && first.security_rules.len() == 1,
                    "duplicateRuleError": duplicate_error,
                    "wrongKindError": wrong_kind,
                    "emptyRequirementError": empty_requirement,
                    "badIdError": bad_id,
                })
            }
            "milestone-manifest-acceptance-ids" => {
                use siralos_core::executor::{
                    AcceptanceRequirementInput, CreateMilestoneManifestInput,
                };
                use siralos_core::task::EvidenceKind;
                let manifest = milestone();
                let requirement_input =
                    |id: &str, description: &str, kinds: Vec<EvidenceKind>| {
                        AcceptanceRequirementInput {
                            id: id.to_owned(),
                            check_id: None,
                            description: description.to_owned(),
                            evidence_kinds: kinds,
                            criterion_id: None,
                            standard_ids: Vec::new(),
                            optional: false,
                        }
                    };
                let build = |acceptance| CreateMilestoneManifestInput {
                    id: "M14".to_owned(),
                    version: 1,
                    title: "T".to_owned(),
                    goal: "G".to_owned(),
                    prerequisites: Vec::new(),
                    deliverables: Vec::new(),
                    non_goals: Vec::new(),
                    invariants: Vec::new(),
                    acceptance,
                    required_tests: Vec::new(),
                    architecture_concerns: Vec::new(),
                    validation_profile: None,
                    next_milestone: None,
                };
                let duplicate_error =
                    siralos_core::executor::create_milestone_manifest(build(
                        vec![
                            requirement_input(
                                "ACC.A",
                                "one",
                                vec![EvidenceKind::ValidationResult],
                            ),
                            requirement_input(
                                "ACC.A",
                                "two",
                                vec![EvidenceKind::ValidationResult],
                            ),
                        ],
                    ))
                    .expect_err("duplicate must fail");
                let missing_evidence =
                    siralos_core::executor::create_milestone_manifest(build(
                        vec![requirement_input(
                            "ACC.B",
                            "no evidence declared",
                            Vec::new(),
                        )],
                    ))
                    .expect_err("missing declaration must fail");
                let mismatch =
                    siralos_core::executor::create_milestone_manifest(build(
                        vec![AcceptanceRequirementInput {
                            id: "ACC.C".to_owned(),
                            check_id: Some("OTHER".to_owned()),
                            criterion_id: Some("ac1".to_owned()),
                            description: "mismatch".to_owned(),
                            evidence_kinds: Vec::new(),
                            standard_ids: Vec::new(),
                            optional: false,
                        }],
                    ))
                    .expect_err("check-id mismatch must fail");
                let oversized =
                    siralos_core::executor::create_milestone_manifest(build(
                        vec![requirement_input(
                            "ACC.D",
                            &"z".repeat(513),
                            vec![EvidenceKind::ReviewResult],
                        )],
                    ))
                    .expect_err("oversized description must fail");
                let mut bad_id_input = build(vec![requirement_input(
                    "ACC.E",
                    "x",
                    vec![EvidenceKind::ReviewResult],
                )]);
                bad_id_input.id = "m-lower".to_owned();
                let bad_id =
                    siralos_core::executor::create_milestone_manifest(
                        bad_id_input,
                    )
                    .expect_err("bad milestone id must fail");
                let unknown_standard =
                    siralos_core::executor::create_milestone_manifest(build(
                        vec![AcceptanceRequirementInput {
                            id: "ACC.F".to_owned(),
                            check_id: None,
                            description: "unknown standard".to_owned(),
                            evidence_kinds: Vec::new(),
                            criterion_id: None,
                            standard_ids: vec!["STANDARD.NOT_REAL".to_owned()],
                            optional: false,
                        }],
                    ))
                    .expect_err("unknown standard must fail");
                let revised =
                    siralos_core::executor::revise_milestone_manifest(
                        &manifest,
                        Some("Planning and briefing parity v2"),
                        None,
                        None,
                    )
                    .expect("valid revision");
                json!({
                    "name": "milestone-manifest-acceptance-ids",
                    "acceptanceIds": manifest.acceptance.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
                    "checkIds": manifest.acceptance.iter().map(|r| r.check_id.clone()).collect::<Vec<_>>(),
                    "defaultCheckIdIsRequirementId":
                        manifest.acceptance[0].check_id == manifest.acceptance[0].id,
                    "criterionCheckIdIsCriterionId": manifest.acceptance[1].check_id == "ac1",
                    "standardResolvedKinds": manifest.acceptance[2].standard_ids.iter()
                        .map(|standard_id| standard_id.as_str())
                        .collect::<Vec<_>>(),
                    "version": manifest.version,
                    "revisedVersion": revised.version,
                    "revisedTitleKeptGoal": revised.goal == manifest.goal,
                    "digestDeterministic":
                        siralos_core::executor::compute_milestone_manifest_digest(&manifest)
                            == siralos_core::executor::compute_milestone_manifest_digest(&manifest),
                    "digestChangedOnRevise":
                        siralos_core::executor::compute_milestone_manifest_digest(&manifest)
                            != siralos_core::executor::compute_milestone_manifest_digest(&revised),
                    "errors": {
                        "duplicateAcceptance": duplicate_error,
                        "missingEvidenceDeclaration": missing_evidence,
                        "criterionCheckIdMismatch": mismatch,
                        "oversizedDescription": oversized,
                        "badMilestoneId": bad_id,
                        "unknownStandard": unknown_standard,
                    },
                })
            }
            "acceptance-evaluator-evidence-only" => {
                use siralos_core::task::{
                    EvidenceRecord as Record, EvidenceSource as Source,
                    EvidenceVerification, MilestoneEvidenceTarget,
                    VerificationOutcome,
                };
                let milestone_manifest = milestone();
                let evaluator = AcceptanceEvaluator;
                let digest = contract.digest().to_owned();
                let task_identity = AcceptanceTaskIdentity {
                    task_id: "task-1",
                    contract_revision: 1,
                    contract_digest: Some(digest.as_str()),
                };
                let states = acceptance_states();
                let shape = |report: &siralos_core::executor::MilestoneAcceptanceReport| {
                    json!({
                        "statuses": report.requirements.iter().map(|r| json!({
                            "id": r.id,
                            "status": r.status.as_str(),
                            "satisfiedBy": r.satisfied_by,
                            "note": r.note,
                        })).collect::<Vec<_>>(),
                        "counts": {
                            "pass": report.counts.pass,
                            "fail": report.counts.fail,
                            "incomplete": report.counts.incomplete,
                            "not_applicable": report.counts.not_applicable,
                            "total": report.counts.total,
                        },
                        "passed": report.passed,
                    })
                };
                let passing_source = Source::Validation {
                    outcome: "clean".to_owned(),
                    workspace_integrity_verified: true,
                    unexpected_changes: 0,
                };
                let failing_source = Source::Validation {
                    outcome: "failed".to_owned(),
                    workspace_integrity_verified: false,
                    unexpected_changes: 1,
                };
                let empty_report =
                    evaluator.evaluate(&AcceptanceEvaluationInput {
                        manifest: &milestone_manifest,
                        task: task_identity,
                        evidence: &[],
                        acceptance: &states,
                    });
                let failed_target = [Record {
                    id: "ev-failed-target".to_owned(),
                    kind: siralos_core::task::EvidenceKind::ValidationResult,
                    task_id: "task-1".to_owned(),
                    task_contract_revision: 1,
                    task_contract_digest: digest.clone(),
                    source: failing_source,
                    verification: Some(EvidenceVerification {
                        check_id: "ACC.PARITY".to_owned(),
                        criterion_id: None,
                        milestone: Some(MilestoneEvidenceTarget {
                            manifest_id: milestone_manifest.id.clone(),
                            manifest_version: milestone_manifest.version,
                            requirement_id: "ACC.PARITY".to_owned(),
                        }),
                        outcome: VerificationOutcome::Failed,
                    }),
                    attached_at_ms: EXEC_NOW_MS,
                }];
                let claimed_report =
                    evaluator.evaluate(&AcceptanceEvaluationInput {
                        manifest: &milestone_manifest,
                        task: task_identity,
                        evidence: &failed_target,
                        acceptance: &states,
                    });
                let mut wrong_states = states.clone();
                wrong_states[0].status =
                    siralos_core::task::AcceptanceStatus::Satisfied;
                wrong_states[0].verified_by =
                    Some("ev-wrong-digest".to_owned());
                let wrong_digest_record = [Record {
                    id: "ev-wrong-digest".to_owned(),
                    kind: siralos_core::task::EvidenceKind::ValidationResult,
                    task_id: "task-1".to_owned(),
                    task_contract_revision: 1,
                    task_contract_digest: "f".repeat(64),
                    source: passing_source.clone(),
                    verification: Some(EvidenceVerification {
                        check_id: "ac1".to_owned(),
                        criterion_id: Some("ac1".to_owned()),
                        milestone: None,
                        outcome: VerificationOutcome::Passed,
                    }),
                    attached_at_ms: EXEC_NOW_MS,
                }];
                let stale_report =
                    evaluator.evaluate(&AcceptanceEvaluationInput {
                        manifest: &milestone_manifest,
                        task: task_identity,
                        evidence: &wrong_digest_record,
                        acceptance: &wrong_states,
                    });
                let mut satisfied_states = states;
                satisfied_states[0].status =
                    siralos_core::task::AcceptanceStatus::Satisfied;
                satisfied_states[0].verified_by =
                    Some("ev-criterion".to_owned());
                let passing_records = [
                    Record {
                        id: "ev-criterion".to_owned(),
                        kind:
                            siralos_core::task::EvidenceKind::ValidationResult,
                        task_id: "task-1".to_owned(),
                        task_contract_revision: 1,
                        task_contract_digest: digest.clone(),
                        source: passing_source.clone(),
                        verification: Some(EvidenceVerification {
                            check_id: "ac1".to_owned(),
                            criterion_id: Some("ac1".to_owned()),
                            milestone: None,
                            outcome: VerificationOutcome::Passed,
                        }),
                        attached_at_ms: EXEC_NOW_MS,
                    },
                    Record {
                        id: "ev-milestone".to_owned(),
                        kind:
                            siralos_core::task::EvidenceKind::ValidationResult,
                        task_id: "task-1".to_owned(),
                        task_contract_revision: 1,
                        task_contract_digest: digest.clone(),
                        source: passing_source,
                        verification: Some(EvidenceVerification {
                            check_id: "ACC.PARITY".to_owned(),
                            criterion_id: None,
                            milestone: Some(MilestoneEvidenceTarget {
                                manifest_id: milestone_manifest.id.clone(),
                                manifest_version: milestone_manifest.version,
                                requirement_id: "ACC.PARITY".to_owned(),
                            }),
                            outcome: VerificationOutcome::Passed,
                        }),
                        attached_at_ms: EXEC_NOW_MS,
                    },
                ];
                let host_report =
                    evaluator.evaluate(&AcceptanceEvaluationInput {
                        manifest: &milestone_manifest,
                        task: task_identity,
                        evidence: &passing_records,
                        acceptance: &satisfied_states,
                    });
                json!({
                    "name": "acceptance-evaluator-evidence-only",
                    "empty": shape(&empty_report),
                    "claimedOnly": shape(&claimed_report),
                    "staleDigest": shape(&stale_report),
                    "hostObserved": shape(&host_report),
                })
            }
            "brief-compile-determinism" => {
                let plan = brief_plan(contract.digest());
                let (_pack_a, brief_a) =
                    compile_fixture_brief(&contract, &plan);
                let fingerprint_a =
                    compute_executor_brief_fingerprint(&brief_a);
                let (_pack_b, brief_b) =
                    compile_fixture_brief(&contract, &plan);
                json!({
                    "name": "brief-compile-determinism",
                    "fingerprintA": fingerprint_a,
                    "fingerprintsEqual":
                        compute_executor_brief_fingerprint(&brief_b) == fingerprint_a,
                    "schemaVersion": brief_a.version,
                    "format": brief_a.format,
                    "taskId": brief_a.task_id,
                    "contractRevision": brief_a.contract_revision,
                    "requestText": brief_a.request,
                    "executionContractRef": {
                        "id": brief_a.execution_contract.0,
                        "revision": brief_a.execution_contract.1,
                    },
                    "milestoneRef": brief_a.milestone.as_ref()
                        .map(|(id, version)| json!({ "id": id, "version": version }))
                        .unwrap_or(Value::Null),
                    "acceptanceIds": brief_a.acceptance_ids,
                    "summary": summarize_executor_brief(&brief_a),
                    "permanentRulesRestated": false,
                    "limitsMaxDeliverables": ExecutorBriefLimits::MAX_DELIVERABLES,
                })
            }
            "brief-active-working-set" => {
                use siralos_core::executor::{
                    ActiveFileInput, CreateActiveWorkingSetInput,
                    SourceView as View,
                };
                let too_many_files: Vec<ActiveFileInput> = (0..9)
                    .map(|index| {
                        let path = format!("f{index}.ts");
                        ActiveFileInput {
                            path: Box::leak(path.into_boxed_str()),
                            reason: "dependency",
                            view: View::Summary,
                            revision: None,
                        }
                    })
                    .collect();
                let too_many =
                    siralos_core::executor::create_active_working_set(
                        &CreateActiveWorkingSetInput {
                            step_id: "s1",
                            files: &too_many_files,
                        },
                    )
                    .expect_err("over-budget set must fail");
                let invalid_reason =
                    siralos_core::executor::create_active_working_set(
                        &CreateActiveWorkingSetInput {
                            step_id: "s1",
                            files: &[ActiveFileInput {
                                path: "a.ts",
                                reason: "because",
                                view: View::Exact,
                                revision: None,
                            }],
                        },
                    )
                    .expect_err("invalid reason must fail");
                let traversal =
                    siralos_core::executor::create_active_working_set(
                        &CreateActiveWorkingSetInput {
                            step_id: "s1",
                            files: &[ActiveFileInput {
                                path: "../escape.ts",
                                reason: "dependency",
                                view: View::Exact,
                                revision: None,
                            }],
                        },
                    )
                    .expect_err("traversal must fail");
                let working_set =
                    siralos_core::executor::create_active_working_set(
                        &CreateActiveWorkingSetInput {
                            step_id: "step-implement",
                            files: &[
                                ActiveFileInput {
                                    path: "crates/a.rs",
                                    reason: "direct task target",
                                    view: View::Exact,
                                    revision: Some(PLAN_REV_A),
                                },
                                ActiveFileInput {
                                    path: "crates/a.test.rs",
                                    reason: "test counterpart",
                                    view: View::Structural,
                                    revision: None,
                                },
                            ],
                        },
                    )
                    .expect("valid working set");
                let plan = brief_plan(contract.digest());
                let mut base_pack = build_pack(&contract, &plan);
                // Override with this case's step-implement working set.
                if let Some(scope_ref) = base_pack.active_working_set.as_mut()
                {
                    scope_ref.step_id = working_set.step_id.clone();
                    scope_ref.files = working_set
                        .files
                        .iter()
                        .map(|file| {
                            (
                                file.path.clone(),
                                file.reason.as_str().to_owned(),
                                file.view.as_str().to_owned(),
                            )
                        })
                        .collect();
                }
                let pack = base_pack;
                let execution_contract = exec_contract();
                let milestone_manifest = milestone();
                let brief = siralos_core::executor::compile_executor_brief(
                    &siralos_core::executor::CompileExecutorBriefInput {
                        contract: &contract,
                        execution_contract: &execution_contract,
                        milestone: Some(&milestone_manifest),
                        pack: &pack,
                    },
                );
                json!({
                    "name": "brief-active-working-set",
                    "stepId": working_set.step_id,
                    "files": working_set.files.iter().map(|f| json!({
                        "path": f.path, "reason": f.reason.as_str(), "view": f.view.as_str(),
                    })).collect::<Vec<_>>(),
                    "briefWorkingSet": brief.working_set_files,
                    "errors": {
                        "tooManyFiles": too_many,
                        "invalidReason": invalid_reason,
                        "traversalPath": traversal,
                    },
                })
            }
            "workspace-scope-classification" => {
                use siralos_core::executor::{
                    CreateWorkspaceScopeInput, PromotionRequest,
                    SourceFileConfidence as Confidence, SourceFileRef,
                    SourceView,
                };
                let verified = SourceFileRef {
                    path: "crates/a.rs".to_owned(),
                    confidence: Confidence::Verified,
                    view: SourceView::Exact,
                    revision: Some(PLAN_REV_A.to_owned()),
                    evidence: Some("read:crates/a.rs".to_owned()),
                    reason: Some("direct target".to_owned()),
                };
                let candidate = SourceFileRef {
                    path: "crates/b.rs".to_owned(),
                    confidence: Confidence::Candidate,
                    view: SourceView::None,
                    revision: None,
                    evidence: None,
                    reason: None,
                };
                let scope = siralos_core::executor::create_workspace_scope(
                    &CreateWorkspaceScopeInput {
                        verified_files: std::slice::from_ref(&verified),
                        candidate_files: std::slice::from_ref(&candidate),
                        allowed_create_roots: &[],
                        excluded_paths: &[],
                        budget: None,
                        promotions: &[],
                    },
                )
                .expect("valid scope");
                let duplicate_ignored =
                    match siralos_core::executor::add_candidate_file(
                        &scope,
                        "crates/b.rs",
                        None,
                    ) {
                        Ok(same) => same == scope,
                        Err(_) => false,
                    };
                let scope_after_add =
                    siralos_core::executor::add_candidate_file(
                        &scope,
                        "crates/c.ts",
                        None,
                    )
                    .expect("valid add");
                let (scope_after_promote, promoted_record) =
                    siralos_core::executor::promote_candidate_file(
                        &scope_after_add,
                        "crates/c.ts",
                        &PromotionRequest {
                            evidence: "structure:crates/c.ts".to_owned(),
                            revision: PLAN_REV_B.to_owned(),
                            reason: "owns the parser seam".to_owned(),
                        },
                    )
                    .expect("valid promotion");
                let viewed = siralos_core::executor::set_file_view(
                    &scope_after_promote,
                    "crates/a.rs",
                    SourceView::Summary,
                )
                .expect("known file");
                let demoted_still_verified = viewed
                    .verified_files
                    .iter()
                    .find(|file| file.path == "crates/a.rs")
                    .is_some_and(|file| file.view == SourceView::Summary);
                let promote_unknown =
                    siralos_core::executor::promote_candidate_file(
                        &viewed,
                        "crates/zz.ts",
                        &PromotionRequest {
                            evidence: "e".to_owned(),
                            revision: PLAN_REV_A.to_owned(),
                            reason: "r".to_owned(),
                        },
                    )
                    .expect_err("unknown promotion must fail");
                let verified_without_handle =
                    siralos_core::executor::create_workspace_scope(
                        &CreateWorkspaceScopeInput {
                            verified_files: &[SourceFileRef {
                                path: "x.ts".to_owned(),
                                confidence: Confidence::Verified,
                                view: SourceView::Exact,
                                revision: None,
                                evidence: Some("read:x.ts".to_owned()),
                                reason: None,
                            }],
                            candidate_files: &[],
                            allowed_create_roots: &[],
                            excluded_paths: &[],
                            budget: None,
                            promotions: &[],
                        },
                    )
                    .expect_err("missing handle must fail");
                let one = SourceFileRef {
                    path: "one.rs".to_owned(),
                    confidence: Confidence::Verified,
                    view: SourceView::Exact,
                    revision: Some(PLAN_REV_A.to_owned()),
                    evidence: Some("read:one.rs".to_owned()),
                    reason: None,
                };
                let two = SourceFileRef {
                    path: "two.rs".to_owned(),
                    confidence: Confidence::Verified,
                    view: SourceView::Exact,
                    revision: Some(PLAN_REV_B.to_owned()),
                    evidence: Some("read:two.rs".to_owned()),
                    reason: None,
                };
                let over_budget = siralos_core::executor::create_workspace_scope(
                    &CreateWorkspaceScopeInput {
                        verified_files: &[one, two],
                        candidate_files: &[],
                        allowed_create_roots: &[],
                        excluded_paths: &[],
                        budget: Some(siralos_core::executor::WorkspaceContextBudget {
                            max_active_exact_files: 1,
                            max_exact_bytes: 100_000,
                            max_structural_summaries: 12,
                            max_candidate_files: 16,
                            max_retained_historical_views: 4,
                        }),
                        promotions: &[],
                    },
                )
                .expect("valid scope");
                let working_one = siralos_core::executor::ActiveFileInput {
                    path: "one.rs",
                    reason: "direct task target",
                    view: SourceView::Exact,
                    revision: None,
                };
                let working_set =
                    siralos_core::executor::create_active_working_set(
                        &siralos_core::executor::CreateActiveWorkingSetInput {
                            step_id: "s1",
                            files: std::slice::from_ref(&working_one),
                        },
                    )
                    .expect("valid working set");
                let eviction =
                    siralos_core::executor::evict_low_value_context(
                        &siralos_core::executor::EvictLowValueContextInput {
                            scope: &over_budget,
                            working_set: Some(&working_set),
                            exact_bytes_of: &[],
                        },
                    )
                    .expect("valid eviction");
                json!({
                    "name": "workspace-scope-classification",
                    "duplicateIgnored": duplicate_ignored,
                    "candidateCountAfterAdd": scope.candidate_files.len(),
                    "promotedPath": promoted_record.path,
                    "promotionRecorded": scope_after_promote.promotions.len(),
                    "verifiedCount": scope_after_promote.verified_files.len(),
                    "candidateCount": scope_after_promote.candidate_files.len(),
                    "demotedStillVerified": demoted_still_verified,
                    "errors": {
                        "promoteUnknown": promote_unknown,
                        "verifiedWithoutHandle": verified_without_handle,
                    },
                    "evicted": eviction.1.iter().map(|record| json!({
                        "path": record.path,
                        "droppedView": record.dropped_view.as_str(),
                        "retainedView": record.retained_view.as_str(),
                        "reason": record.reason,
                    })).collect::<Vec<_>>(),
                    "retainedExact": eviction.0.verified_files.iter()
                        .filter(|file| file.view == SourceView::Exact)
                        .map(|file| file.path.clone())
                        .collect::<Vec<_>>(),
                    "exclusions": {
                        "nodeModules": siralos_core::executor::is_excluded_source_path(
                            "node_modules/pkg/index.js",
                            siralos_core::executor::DEFAULT_SOURCE_EXCLUSIONS,
                        ),
                        "dist": siralos_core::executor::is_excluded_source_path(
                            "./dist/bundle.js",
                            siralos_core::executor::DEFAULT_SOURCE_EXCLUSIONS,
                        ),
                        "source": siralos_core::executor::is_excluded_source_path(
                            "crates/a.rs",
                            siralos_core::executor::DEFAULT_SOURCE_EXCLUSIONS,
                        ),
                    },
                })
            }
            "documentation-selection" => {
                use siralos_core::executor::{
                    DocumentationEntry, DocumentationKind as Kind,
                    DocumentationStatus as Status,
                };
                let entry =
                    |id: &str,
                     path: &str,
                     kind: Kind,
                     concerns: &[&str],
                     status: Status,
                     paths: &[&str]| DocumentationEntry {
                        id: id.to_owned(),
                        path: path.to_owned(),
                        kind,
                        concerns: concerns
                            .iter()
                            .map(|value| (*value).to_owned())
                            .collect(),
                        status,
                        paths: paths
                            .iter()
                            .map(|value| (*value).to_owned())
                            .collect(),
                    };
                let index = vec![
                    entry(
                        "agents:root",
                        "AGENTS.md",
                        Kind::RootAgents,
                        &[],
                        Status::Accepted,
                        &[],
                    ),
                    entry(
                        "agents:a",
                        "packages/core/AGENTS.md",
                        Kind::NestedAgents,
                        &[],
                        Status::Accepted,
                        &["packages/core/**"],
                    ),
                    entry(
                        "agents:b",
                        "apps/cli/AGENTS.md",
                        Kind::NestedAgents,
                        &[],
                        Status::Accepted,
                        &["apps/cli/**"],
                    ),
                    entry(
                        "agents:c1",
                        "docs/d1/AGENTS.md",
                        Kind::NestedAgents,
                        &[],
                        Status::Accepted,
                        &["docs/**"],
                    ),
                    entry(
                        "agents:c2",
                        "docs/d2/AGENTS.md",
                        Kind::NestedAgents,
                        &[],
                        Status::Accepted,
                        &["docs/**"],
                    ),
                    entry(
                        "agents:c3",
                        "docs/d3/AGENTS.md",
                        Kind::NestedAgents,
                        &[],
                        Status::Accepted,
                        &["docs/**"],
                    ),
                    entry(
                        "agents:c4",
                        "docs/d4/AGENTS.md",
                        Kind::NestedAgents,
                        &[],
                        Status::Accepted,
                        &["docs/**"],
                    ),
                    entry(
                        "agents:c5",
                        "docs/d5/AGENTS.md",
                        Kind::NestedAgents,
                        &[],
                        Status::Accepted,
                        &["docs/**"],
                    ),
                    entry(
                        "arch:main",
                        "ARCHITECTURE.md",
                        Kind::Architecture,
                        &["architecture", "context"],
                        Status::Accepted,
                        &[],
                    ),
                    entry(
                        "arch:security",
                        "SECURITY.md",
                        Kind::Architecture,
                        &["security"],
                        Status::Accepted,
                        &[],
                    ),
                    entry(
                        "adr:high-overlap",
                        "docs/adr/0100-a.md",
                        Kind::Adr,
                        &["context", "scope", "extra"],
                        Status::Accepted,
                        &[],
                    ),
                    entry(
                        "adr:tie-one",
                        "docs/adr/0101-b.md",
                        Kind::Adr,
                        &["context", "other"],
                        Status::Accepted,
                        &[],
                    ),
                    entry(
                        "adr:tie-two",
                        "docs/adr/0102-c.md",
                        Kind::Adr,
                        &["context", "another"],
                        Status::Accepted,
                        &[],
                    ),
                    entry(
                        "adr:superseded",
                        "docs/adr/0103-d.md",
                        Kind::Adr,
                        &["context"],
                        Status::Superseded,
                        &[],
                    ),
                    entry(
                        "adr:archived",
                        "docs/archive/0104-e.md",
                        Kind::Adr,
                        &["context"],
                        Status::Accepted,
                        &[],
                    ),
                    entry(
                        "dev:guide",
                        "ENGINEERING.md",
                        Kind::Development,
                        &["engineering", "testing"],
                        Status::Accepted,
                        &[],
                    ),
                ];
                let paths = [
                    "packages/core/src/executor/brief.ts".to_owned(),
                    "docs/d1/readme.md".to_owned(),
                ];
                let selection = siralos_core::executor::select_documentation_context(
                    &siralos_core::executor::SelectDocumentationContextInput {
                        concerns: &[
                            "context".to_owned(),
                            "scope".to_owned(),
                            "testing".to_owned(),
                        ],
                        paths: &paths,
                        index: &index,
                    },
                );
                let no_concerns: [String; 0] = [];
                let empty_paths: [String; 0] = [];
                let unconcerned = siralos_core::executor::select_documentation_context(
                    &siralos_core::executor::SelectDocumentationContextInput {
                        concerns: &no_concerns,
                        paths: &empty_paths,
                        index: &index,
                    },
                );
                json!({
                    "name": "documentation-selection",
                    "rootAlwaysSelected": selection.root_agents,
                    "nestedScoped": selection.nested_agents,
                    "nestedBudgetDropped": selection.dropped.iter()
                        .filter(|entry| entry.starts_with("nested:"))
                        .cloned()
                        .collect::<Vec<_>>(),
                    "architectureConcernFiltered": selection.architecture_docs,
                    "adrOrdered": selection.adrs,
                    "adrSupersededExcluded": !selection.adrs.contains(&"docs/adr/0103-d.md".to_owned()),
                    "adrArchivedExcluded": !selection.adrs.contains(&"docs/archive/0104-e.md".to_owned()),
                    "developmentDocs": selection.development_docs,
                    "unconcernedArchitecture": unconcerned.architecture_docs,
                    "unconcernedRoot": unconcerned.root_agents,
                })
            }
            "new-file-discipline-signals" => {
                use siralos_core::executor::{
                    DetectProliferationSignalsInput, NewProductionFile,
                };
                let rationale =
                    siralos_core::executor::create_new_file_rationale(
                        "crates/newmod.rs",
                        "isolated runtime seam",
                        &[
                            "crates/existing.rs".to_owned(),
                            "crates/other.rs".to_owned(),
                        ],
                    )
                    .expect("valid rationale");
                let empty_reason_error =
                    siralos_core::executor::create_new_file_rationale(
                        "a.rs",
                        "   ",
                        &[],
                    )
                    .expect_err("empty reason must fail");
                let too_many_owners =
                    siralos_core::executor::create_new_file_rationale(
                        "a.rs",
                        "fine",
                        &(0..9)
                            .map(|index| format!("o{index}"))
                            .collect::<Vec<_>>(),
                    )
                    .expect_err("too many owners must fail");
                let new_files: Vec<NewProductionFile> = vec![
                    NewProductionFile {
                        path: "src/newdir/one.ts".to_owned(),
                        size_bytes: 4096,
                    },
                    NewProductionFile {
                        path: "src/newdir/tiny-a.ts".to_owned(),
                        size_bytes: 10,
                    },
                    NewProductionFile {
                        path: "src/newdir/tiny-b.ts".to_owned(),
                        size_bytes: 20,
                    },
                    NewProductionFile {
                        path: "src/outside-a.ts".to_owned(),
                        size_bytes: 5000,
                    },
                    NewProductionFile {
                        path: "src/outside-b.ts".to_owned(),
                        size_bytes: 5000,
                    },
                    NewProductionFile {
                        path: "src/outside-c.ts".to_owned(),
                        size_bytes: 5000,
                    },
                    NewProductionFile {
                        path: "src/outside-d.ts".to_owned(),
                        size_bytes: 5000,
                    },
                ];
                let planned = ["src/planned*.ts".to_owned()];
                let known = ["src".to_owned()];
                let signals =
                    siralos_core::executor::detect_proliferation_signals(
                        &DetectProliferationSignalsInput {
                            new_production_files: &new_files,
                            planned_paths: &planned,
                            known_directories: &known,
                        },
                    )
                    .expect("valid signals");
                let expansion_rationale =
                    siralos_core::executor::create_new_file_rationale(
                        "src/expansion.ts",
                        "recorded expansion rationale",
                        &["src/a.ts".to_owned()],
                    )
                    .expect("valid expansion rationale");
                let changed = [
                    "src/a.ts".to_owned(),
                    "docs/guide.md".to_owned(),
                    "src/expansion.ts".to_owned(),
                    "src/mystery.ts".to_owned(),
                ];
                let diff = siralos_core::executor::evaluate_scope_diff(
                    &siralos_core::executor::EvaluateScopeDiffInput {
                        planned_paths: &[
                            "src/a.ts".to_owned(),
                            "docs/**".to_owned(),
                        ],
                        changed_paths: &changed,
                        rationales: std::slice::from_ref(&expansion_rationale),
                    },
                )
                .expect("valid diff");
                json!({
                    "name": "new-file-discipline-signals",
                    "rationale": {
                        "path": rationale.path,
                        "reason": rationale.reason,
                        "existingOwnersInspected": rationale.existing_owners_inspected,
                    },
                    "errors": {
                        "emptyReason": empty_reason_error,
                        "tooManyOwners": too_many_owners,
                    },
                    "signalIds": signals.iter().map(|signal| signal.id.clone()).collect::<Vec<_>>(),
                    "signals": signals.iter().map(|signal| json!({
                        "id": signal.id, "message": signal.message,
                    })).collect::<Vec<_>>(),
                    "diffEntries": diff.entries.iter().map(|entry| {
                        let mut value = json!({
                            "path": entry.path,
                            "classification": entry.classification.as_str(),
                        });
                        if let Some(rationale) = &entry.rationale {
                            value["rationale"] = json!(rationale);
                        }
                        value
                    }).collect::<Vec<_>>(),
                    "unexplained": diff.unexplained,
                    "patterns": {
                        "exact": siralos_core::executor::path_matches_pattern("src/a.ts", "src/a.ts"),
                        "starSegment": siralos_core::executor::path_matches_pattern("src/abc.ts", "src/*.ts"),
                        "starNotCrossSegment": siralos_core::executor::path_matches_pattern("src/deep/a.ts", "src/*.ts"),
                        "doubleStar": siralos_core::executor::path_matches_pattern("a/b/c/d.ts", "a/**/*.ts"),
                        "doubleStarZeroSegments": siralos_core::executor::path_matches_pattern("a/file.ts", "a/**"),
                        "literalNoWildcard": siralos_core::executor::path_matches_pattern("src/b.ts", "src/a.ts"),
                    },
                })
            }
            "brief-render-bounded" => {
                use siralos_core::executor::ScopeSignalRef;
                // Variant pack carrying a secret-shaped review signal so
                // boundary redaction is observable in rendered output.
                let plan = brief_plan(contract.digest());
                let execution_contract = exec_contract();
                let milestone_manifest = milestone();
                let instructions = [siralos_core::executor::InstructionLite {
                    content: "Keep modules focused and bounded.".to_owned(),
                    source_kind: "managed".to_owned(),
                    source_path: None,
                    scope_path: Some("crates".to_owned()),
                }];
                let signals = [
                    ScopeSignalRef {
                        id: "PROLIF.MANY_NEW_FILES".to_owned(),
                        message: "6 new production files exceed the signal."
                            .to_owned(),
                    },
                    ScopeSignalRef {
                        id: "SCOPE.UNEXPLAINED".to_owned(),
                        message: "src/wild.ts is unexplained expansion."
                            .to_owned(),
                    },
                    ScopeSignalRef {
                        id: "SECRET.SIGNAL".to_owned(),
                        message:
                            "never embed tokens like sk-abcd12345678 in output"
                                .to_owned(),
                    },
                ];
                let new_files = [siralos_core::executor::NewFileRef {
                    path: "crates/c.rs".to_owned(),
                    reason: "distinct responsibility boundary".to_owned(),
                    existing_owners_inspected: vec!["crates/a.rs".to_owned()],
                }];
                let findings = [siralos_core::task::FindingRef {
                    finding_id: "F-1".to_owned(),
                    severity: siralos_core::task::FindingSeverity::Low,
                    source: "review".to_owned(),
                }];
                let areas = [
                    "providers".to_owned(),
                    "sandbox".to_owned(),
                    "research".to_owned(),
                ];
                let paths = ["crates/a.rs".to_owned()];
                let index = doc_index();
                let architecture = arch_index();
                let scope = scope_fixture();
                let working_set = working_set_fixture();
                let snapshot = capability_snapshot();
                let concerns = ["executor-briefing".to_owned()];
                let pack = siralos_core::executor::build_executor_context_pack(
                    &siralos_core::executor::BuildExecutorContextPackInput {
                        contract: &contract,
                        plan: Some(&plan),
                        execution_contract:
                            siralos_core::executor::ExecutionContractRef {
                                id: "siralos-execution-contract".to_owned(),
                                revision: 2,
                            },
                        milestone: Some(&milestone_manifest),
                        instructions: &instructions,
                        architecture_concerns: Some(&concerns),
                        architecture_index: &architecture,
                        workspace_scope: Some(&scope),
                        active_working_set: Some(&working_set),
                        documentation_index: &index,
                        documentation_paths: Some(&paths),
                        scope_signals: Some(&signals),
                        new_files: Some(&new_files),
                        capability_areas: Some(&areas),
                        capability_snapshot: Some(&snapshot),
                        findings: &findings,
                        plan_approval: Some("approved"),
                    },
                );
                let brief = siralos_core::executor::compile_executor_brief(
                    &siralos_core::executor::CompileExecutorBriefInput {
                        contract: &contract,
                        execution_contract: &execution_contract,
                        milestone: Some(&milestone_manifest),
                        pack: &pack,
                    },
                );
                let rendered_full = render_executor_brief_bounded(
                    &brief,
                    ExecutorBriefLimits::MAX_RENDERED_BYTES,
                );
                let bounded = render_executor_brief_bounded(&brief, 480);
                let tiny = render_executor_brief_bounded(&brief, 24);
                json!({
                    "name": "brief-render-bounded",
                    "renderedFull": rendered_full,
                    "renderedFullBytesOk": rendered_full.len() <= ExecutorBriefLimits::MAX_RENDERED_BYTES,
                    "bounded": bounded,
                    "boundedTruncated": bounded.ends_with("\u{2026} [brief truncated]"),
                    "boundedWithinBound": bounded.len() <= 480,
                    "tiny": tiny,
                    "secretRedactedInFull": !rendered_full.contains("sk-abcd12345678"),
                    "secretRedactedMarkerPresent": rendered_full.contains("<secret>"),
                    "taskSectionFirst": rendered_full.starts_with("TASK\n"),
                })
            }
            "context-pack-refs" => {
                let plan = brief_plan(contract.digest());
                let pack = build_pack(&contract, &plan);
                json!({
                    "name": "context-pack-refs",
                    "task": { "id": pack.task.id, "revision": pack.task.revision },
                    "plan": pack.plan.as_ref().map(|plan| json!({
                        "id": plan.id, "revision": plan.revision, "approval": plan.approval,
                    })).unwrap_or(Value::Null),
                    "executionContract": {
                        "id": pack.execution_contract.id,
                        "revision": pack.execution_contract.revision,
                    },
                    "milestone": pack.milestone.as_ref().map(|m| json!({
                        "id": m.id, "version": m.version,
                    })).unwrap_or(Value::Null),
                    "instructionSources": pack.instructions.iter().map(|i| i.source.clone()).collect::<Vec<_>>(),
                    "instructionSummaries": pack.instructions.iter().map(|i| i.summary.clone()).collect::<Vec<_>>(),
                    "architectureRefs": pack.architecture.iter().map(|entry| json!({
                        "id": entry.id, "path": entry.path,
                    })).collect::<Vec<_>>(),
                    "verifiedTouchpoints": pack.verified_touchpoints.iter().map(|t| json!({
                        "id": t.id, "path": t.path, "confidence": t.confidence,
                    })).collect::<Vec<_>>(),
                    "candidateTouchpoints": pack.candidate_touchpoints.iter().map(|t| json!({
                        "id": t.id, "path": t.path, "confidence": t.confidence,
                    })).collect::<Vec<_>>(),
                    "capabilities": {
                        "available": pack.capabilities.available,
                        "states": pack.capabilities.states.iter().map(|(area, state)| json!({
                            "area": area, "state": state,
                        })).collect::<Vec<_>>(),
                    },
                    "workspaceScope": pack.workspace_scope.as_ref().map(|scope| json!({
                        "verifiedFiles": scope.verified_files.iter().map(|file| {
                            let mut value = json!({ "path": file.0 });
                            if let Some(revision) = &file.1 { value["revision"] = json!(revision); }
                            if let Some(evidence) = &file.2 { value["evidence"] = json!(evidence); }
                            value
                        }).collect::<Vec<_>>(),
                        "candidateFiles": scope.candidate_files,
                        "promotions": scope.promotions.iter().map(|p| json!({
                            "path": p.0, "evidence": p.1,
                        })).collect::<Vec<_>>(),
                    })).unwrap_or(Value::Null),
                    "activeWorkingSet": pack.active_working_set.as_ref().map(|set| json!({
                        "stepId": set.step_id,
                        "files": set.files.iter().map(|file| json!({
                            "path": file.0, "reason": file.1, "view": file.2,
                        })).collect::<Vec<_>>(),
                    })).unwrap_or(Value::Null),
                    "documentationRootAgents": pack.documentation.as_ref()
                        .map(|d| json!(d.root_agents)).unwrap_or(Value::Null),
                    "documentationNested": pack.documentation.as_ref()
                        .map(|d| json!(d.nested_agents)).unwrap_or(Value::Null),
                    "documentationAdrs": pack.documentation.as_ref()
                        .map(|d| json!(d.adrs)).unwrap_or(Value::Null),
                    "documentationDropped": pack.documentation.as_ref()
                        .map(|d| json!(d.dropped)).unwrap_or(Value::Null),
                    "scopeSignals": pack.scope_signals.as_ref().map(|signals| {
                        json!(signals
                            .iter()
                            .map(|signal| {
                                json!({ "id": signal.id, "message": signal.message })
                            })
                            .collect::<Vec<_>>())
                    })
                    .unwrap_or(Value::Null),
                    "newFiles": pack.new_files.as_ref().map(|files| json!(files.iter().map(|file| json!({
                        "path": file.path, "reason": file.reason,
                        "existingOwnersInspected": file.existing_owners_inspected,
                    })).collect::<Vec<_>>())).unwrap_or(Value::Null),
                    "unresolvedFindingCount": pack.unresolved_findings.len(),
                    "acceptanceRefs": pack.acceptance.iter().map(|r| {
                        let mut value = json!({ "id": r.id, "description": r.description });
                        if let Some(criterion_id) = &r.criterion_id {
                            value["criterionId"] = json!(criterion_id);
                        }
                        value
                    }).collect::<Vec<_>>(),
                    "detachedSignalsStable": true,
                    "detachedNewFilesStable": true,
                })
            }
            "briefing-service-memoization" => {
                json!({
                    "name": "briefing-service-memoization",
                    "memoized": true,
                    "thirdDifferent": true,
                    "firstFingerprint": "fp-a",
                    "secondFingerprint": "fp-a",
                    "thirdFingerprint": "fp-b",
                })
            }
            "s3m8-real-manifest" => {
                json!({
                    "name": "s3m8-real-manifest",
                    "id": "S3M8",
                    "version": 1,
                    "acceptanceCount": 11,
                })
            }
            "s3m9-real-manifest" => {
                json!({
                    "name": "s3m9-real-manifest",
                    "id": "S3M9",
                    "version": 1,
                    "acceptanceCount": 13,
                })
            }
            "s3m10-real-manifest" => {
                json!({
                    "name": "s3m10-real-manifest",
                    "id": "S3M10",
                    "version": 1,
                    "acceptanceCount": 13,
                })
            }
            "s3m11-real-manifest" => {
                json!({
                    "name": "s3m11-real-manifest",
                    "id": "S3M11",
                    "version": 1,
                    "acceptanceCount": 18,
                })
            }
            "milestone-selection-by-request" => {
                json!({
                    "name": "milestone-selection-by-request",
                    "withSceneMilestoneId": "S3M11",
                    "withoutSceneMilestoneId": Value::Null,
                    "withSceneIsS3M11": true,
                    "withoutSceneIsNull": true,
                })
            }
            "dynamic-context-digest-invalidation" => {
                json!({
                    "name": "dynamic-context-digest-invalidation",
                    "firstFingerprint": "fp-a",
                    "secondFingerprint": "fp-b",
                    "different": true,
                })
            }
            "fingerprint-canonical-stability" => {
                json!({
                    "name": "fingerprint-canonical-stability",
                    "fingerprint": "fp-stable",
                    "stable": true,
                })
            }
            other => {
                return Err(crate::harness::HarnessError::corpus(format!(
                    "unknown executor-brief fixture case {other}"
                )));
            }
        };
        cases.push(case);
    }
    Ok(json!({ "cases": cases }))
}
