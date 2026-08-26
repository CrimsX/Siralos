//! Host-owned planning flow (Stage 3 milestone 7, ADR 0020; Stage 3R
//! R13.4).
//!
//! The flow is the application-owned control phase between the
//! TaskContract and execution. It routes planning depth through the
//! deterministic policy, invokes the read-only planner through the
//! injected port, revalidates the candidate host-side, binds the
//! immutable plan to the current contract revision, and records approval
//! against the exact revision. It never grants capability and never
//! touches the workspace; TaskState remains authoritative.

use crate::task::{TaskHandle, is_terminal_phase};

use super::model::{
    PlanningDepth, TaskPlan, create_task_plan, revise_task_plan,
};
use super::policy::{
    PlanningDecision, PlanningDecisionInput, PlanningDecisionReason,
    PlanningPolicy,
};
use super::validation::{
    PlanCandidateContext, PlanCandidateResult, validate_plan_candidate,
};

/// Request passed to a read-only planner.
pub struct PlannerRequest<'a> {
    /// The task request text.
    pub request: &'a str,
    /// The exact current contract.
    pub contract: &'a crate::task::TaskContract,
    /// The routed depth.
    pub depth: PlanningDepth,
}

/// Outcome returned by a read-only planner (raw candidate JSON).
#[derive(Debug)]
pub enum PlannerOutcome {
    /// A ready candidate in raw planner-JSON form.
    Ready(serde_json::Value),
    /// The planner failed with a message.
    Failed(String),
    /// The planner was cancelled.
    Cancelled,
    /// The planner timed out with a message.
    TimedOut(String),
}

/// Provider-driven read-only planner boundary (implemented by adapters or
/// scripted harness fakes).
pub trait PlannerPort {
    /// Produce one planner outcome for the request.
    fn plan(&mut self, input: &PlannerRequest<'_>) -> PlannerOutcome;
}

/// Result of one flow run.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum PlanFlowResult {
    /// Routed at depth none: no planner invocation happened.
    Routed(PlanningDecision),
    /// A plan revision was stored.
    Planned {
        /// The routed decision.
        decision: PlanningDecision,
        /// The stored immutable plan.
        plan: TaskPlan,
    },
    /// Typed failure with an exact reason.
    Failed {
        /// The routed decision.
        decision: Option<PlanningDecision>,
        /// Exact failure reason.
        message: String,
    },
    /// The planner was cancelled.
    Cancelled(Option<PlanningDecision>),
    /// The planner timed out.
    TimedOut {
        /// The routed decision.
        decision: Option<PlanningDecision>,
        /// Exact timeout message.
        message: String,
    },
}

impl PlanFlowResult {
    /// Status spelling used by canonical records.
    pub fn status(&self) -> &'static str {
        match self {
            PlanFlowResult::Routed(_) => "routed",
            PlanFlowResult::Planned { .. } => "planned",
            PlanFlowResult::Failed { .. } => "failed",
            PlanFlowResult::Cancelled(_) => "cancelled",
            PlanFlowResult::TimedOut { .. } => "timed_out",
        }
    }
}

/// Mutable routing state for one planning flow instance. Methods take the
/// task handle explicitly so direct host operations can interleave.
#[derive(Debug, Default)]
pub struct PlanningFlowState {
    routed: Option<PlanningDecision>,
}

impl PlanningFlowState {
    /// Create the flow state before any route.
    pub fn new() -> Self {
        Self::default()
    }

    /// Route planning depth (host-owned) and record the routing activity.
    pub fn route(
        &mut self,
        handle: &mut TaskHandle<'_>,
        policy: &PlanningPolicy,
        input: &PlanningDecisionInput<'_>,
    ) -> PlanningDecision {
        let decision = policy.decide(input);
        handle.route_planning(decision.depth, decision.reason.as_str());
        self.routed = Some(decision.clone());
        decision
    }

    /// The most recent routing decision, if any.
    pub fn decision(&self) -> Option<&PlanningDecision> {
        self.routed.as_ref()
    }

    /// Run the planner for the routed depth, revalidate, and store the
    /// immutable plan revision.
    ///
    /// `now` supplies the host clock reading for a fresh plan id stamp.
    pub fn run(
        &mut self,
        handle: &mut TaskHandle<'_>,
        planner: &mut dyn PlannerPort,
        now: i64,
    ) -> PlanFlowResult {
        let Some(routed) = self.routed.clone() else {
            return PlanFlowResult::Failed {
                decision: Some(PlanningDecision {
                    depth: PlanningDepth::None,
                    reason: PlanningDecisionReason::InspectionOrNoMutation,
                }),
                message:
                    "Planning was not routed before run; the host must route planning first."
                        .to_owned(),
            };
        };
        if routed.depth == PlanningDepth::None {
            return PlanFlowResult::Routed(routed);
        }
        let task_state = handle.snapshot();
        if is_terminal_phase(task_state.phase) {
            return PlanFlowResult::Failed {
                decision: Some(routed),
                message: format!(
                    "Planning cannot run for a terminal task ({}).",
                    task_state.phase.as_str()
                ),
            };
        }
        let contract = handle.contract().clone();
        let outcome = planner.plan(&PlannerRequest {
            request: contract.request(),
            contract: &contract,
            depth: routed.depth,
        });
        match outcome {
            PlannerOutcome::Cancelled => {
                PlanFlowResult::Cancelled(self.routed.clone())
            }
            PlannerOutcome::TimedOut(message) => {
                let decision = self.routed.clone();
                PlanFlowResult::TimedOut { decision, message }
            }
            PlannerOutcome::Failed(message) => {
                handle.reject_plan(&message);
                let decision = self.routed.clone();
                PlanFlowResult::Failed { decision, message }
            }
            PlannerOutcome::Ready(content) => {
                let validated = validate_plan_candidate(
                    &content,
                    &PlanCandidateContext {
                        contract: &contract,
                        depth: routed.depth,
                    },
                );
                let content = match validated {
                    PlanCandidateResult::Ok(content) => *content,
                    PlanCandidateResult::Rejected(reasons) => {
                        let message = format!(
                            "The planner returned an invalid plan: {}",
                            reasons.join(" ")
                        );
                        handle.reject_plan(&message);
                        let decision = self.routed.clone();
                        return PlanFlowResult::Failed { decision, message };
                    }
                };
                let plan_id = format!("plan-{}", handle.task_id());
                let task_contract_digest = contract.digest().to_owned();
                let previous = handle.current_plan();
                let plan = if previous
                    .as_ref()
                    .is_some_and(|plan| plan.id == plan_id)
                {
                    revise_task_plan(
                        previous.as_ref().expect("checked some"),
                        &super::model::ReviseTaskPlanInput {
                            content,
                            task_contract_digest: Some(task_contract_digest),
                        },
                    )
                } else {
                    create_task_plan(super::model::CreateTaskPlanInput {
                        id: plan_id,
                        task_id: handle.task_id().to_owned(),
                        task_contract_revision: contract.revision(),
                        task_contract_digest,
                        depth: routed.depth,
                        content,
                        created_at: now,
                    })
                };
                let plan = match plan {
                    Ok(plan) => plan,
                    Err(message) => {
                        handle.reject_plan(&message);
                        let decision = self.routed.clone();
                        return PlanFlowResult::Failed { decision, message };
                    }
                };
                if let Err(reason) = handle.set_plan(plan.clone()) {
                    handle.reject_plan(&reason);
                    let decision = self.routed.clone();
                    PlanFlowResult::Failed { decision, message: reason }
                } else {
                    PlanFlowResult::Planned { decision: routed, plan }
                }
            }
        }
    }

    /// Host approval after user review; binds to the exact plan revision.
    pub fn approve(handle: &mut TaskHandle<'_>) -> Result<(), String> {
        let Some(current) = handle.current_plan() else {
            return Err("No current plan exists to approve.".to_owned());
        };
        handle.approve_plan(&current.id, current.revision)
    }

    /// Full-plan mutation-execution gate: `Some(reason)` blocks execution;
    /// `None` allows it.
    pub fn mutation_execution_blocked(
        &self,
        handle: &TaskHandle<'_>,
        contract_has_meaningful_criteria: impl FnOnce() -> bool,
    ) -> Option<String> {
        let state = handle.snapshot().plan;
        if state.state == super::model::TaskPlanStateKind::Stale {
            return Some(format!(
                "The current plan is stale ({}); revalidate or replan before mutation.",
                state
                    .stale_reason
                    .unwrap_or_else(|| "plan invalidated".to_owned())
            ));
        }
        if self
            .routed
            .as_ref()
            .is_some_and(|decision| decision.depth == PlanningDepth::Full)
            && !contract_has_meaningful_criteria()
        {
            return Some(
                "Full-plan execution requires explicit acceptance criteria in the TaskContract (at least two criteria, one host-verifiable); the contract does not meet that bar, so mutation execution is blocked."
                    .to_owned(),
            );
        }
        if self
            .routed
            .as_ref()
            .is_some_and(|decision| decision.depth == PlanningDepth::Full)
        {
            if state.state != super::model::TaskPlanStateKind::Current
                || state.plan_id.is_none()
                || state.depth != PlanningDepth::Full
            {
                return Some(
                    "Full-plan execution requires a current full plan; no matching current plan exists, so mutation execution is blocked."
                        .to_owned(),
                );
            }
            if state.approval != super::model::TaskPlanApprovalKind::Approved {
                return Some(
                    "Full-plan execution requires approval of the exact current plan revision; approval is absent or invalidated, so mutation execution is blocked."
                        .to_owned(),
                );
            }
        }
        None
    }
}
