//! Host-owned planning foundation (Stage 3 milestone 7, ADR 0020;
//! Stage 3R R13.4 parity).
//!
//! Planning is host-routed and structurally read-only: the immutable
//! revisioned plan artifact, the deterministic depth policy, strict
//! untrusted-candidate validation, and the application flow live here.
//! Plans never grant capability; approval binds only to the exact plan
//! and contract revisions and authorizes nothing.

pub mod flow;
pub mod model;
pub mod policy;
pub mod validation;

pub use flow::{
    PlanFlowResult, PlannerOutcome, PlannerPort, PlannerRequest,
    PlanningFlowState,
};
pub use model::{
    CreateTaskPlanInput, NO_TASK_PLAN, PlanApproval, PlanConstraint, PlanRisk,
    PlanRiskSeverity, PlanRollbackStrategy, PlanScope, PlanStep,
    PlanTouchpoint, PlanValidationStrategy, PlanningDepth, PlanningLimits,
    ReviseTaskPlanInput, TaskPlan, TaskPlanApprovalKind, TaskPlanContent,
    TaskPlanState, TaskPlanStateKind, TouchpointConfidence,
    compute_plan_revision_digest, content_candidate_value, create_task_plan,
    has_meaningful_acceptance_criteria, is_valid_plan_element_id,
    is_valid_plan_id, is_valid_revision_handle, plan_content_payload,
    revise_task_plan, stored_plan_digest, summarize_plan,
};
pub use policy::{
    PlanningDecision, PlanningDecisionInput, PlanningDecisionReason,
    PlanningPolicy, RequestedDepth, SurfaceKind,
    contains_godot_scene_or_resource_reference,
    contains_protected_config_reference,
};
pub use validation::{
    PlanCandidateContext, PlanCandidateResult, extract_plan_candidate_json,
    is_safe_plan_path, reject_plan_policy_claims, validate_plan_candidate,
};
