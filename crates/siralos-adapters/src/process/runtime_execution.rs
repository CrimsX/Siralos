//! Generic runtime execution adapter (Stage 4.1).
//!
//! Thin delegation over [`siralos_core::runtime::execution`]. The adapter
//! owns no spawn primitive, no filesystem mutation, and no ambient
//! authority: it forwards the host-authorized decision table and truthfully
//! reports the same typed `UNAVAILABLE` that the core reports when the
//! identity-bound handle primitive is absent. No `Command::new().spawn()`
//! exists in this module.

use siralos_core::provider::CancellationSignal;
use siralos_core::runtime::RuntimeError;
use siralos_core::runtime::budget::RuntimeBudget;
use siralos_core::runtime::execution::{
    IDENTITY_BOUND_UNAVAILABLE_REASON, RuntimeExecutionOutcome,
    RuntimeExecutionRequest, decide_runtime_execution,
    is_identity_bound_launch_primitive_available,
};
use siralos_core::tool::permission::PermissionPolicy;

/// Whether runtime execution is available on this platform.
///
/// Delegates to the core identity-bound handle check. On this platform the
/// primitive is absent, so availability is `false`.
#[must_use]
pub fn is_runtime_execution_available() -> bool {
    is_identity_bound_launch_primitive_available()
}

/// The exact typed reason when runtime execution is unavailable.
///
/// Mirrors [`IDENTITY_BOUND_UNAVAILABLE_REASON`] verbatim.
pub const RUNTIME_EXECUTION_UNAVAILABLE_REASON: &str =
    IDENTITY_BOUND_UNAVAILABLE_REASON;

/// Host-authorized runtime execution decision via the core table.
///
/// This is the adapter's execution seam: it delegates to the core decision
/// and never launches a process. The outcome is the same typed
/// `UNAVAILABLE` that the core reports when the primitive is absent, with
/// the exact reason `"identity-bound launch primitive not available"`.
///
/// # Errors
///
/// Returns [`RuntimeError`] only for malformed inputs; all policy/capability,
/// staleness, budget, cancellation, and primitive-absent cases are typed
/// [`RuntimeExecutionOutcome`] variants.
pub fn decide_adapter_runtime_execution(
    request: &RuntimeExecutionRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    cancellation: CancellationSignal<'_>,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    decide_runtime_execution(request, policy, budget, cancellation)
}

#[cfg(test)]
mod tests {
    use super::{
        RUNTIME_EXECUTION_UNAVAILABLE_REASON,
        decide_adapter_runtime_execution, is_runtime_execution_available,
    };
    use siralos_core::provider::CancellationToken;
    use siralos_core::runtime::budget::{
        RuntimeBudgetInput, create_runtime_budget,
    };
    use siralos_core::runtime::execution::RuntimeExecutionRequest;
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };

    fn allow_policy() -> PermissionPolicy {
        PermissionPolicy::from_rules([PolicyRule {
            capability: CapabilityId::parse("process.execute").unwrap(),
            rule: PermissionRule::Allow,
        }])
    }

    #[test]
    fn adapter_reports_unavailable_without_spawn() {
        assert!(!is_runtime_execution_available());
        assert_eq!(
            RUNTIME_EXECUTION_UNAVAILABLE_REASON,
            "identity-bound launch primitive not available"
        );
        let request = RuntimeExecutionRequest {
            command: "echo".to_owned(),
            args: vec![],
            run_id: "run_runtime_abc".to_owned(),
            operation_id: None,
            is_stale: false,
            requested_bytes: 0,
        };
        let budget = create_runtime_budget(&RuntimeBudgetInput::default());
        let token = CancellationToken::new();
        let outcome = decide_adapter_runtime_execution(
            &request,
            &allow_policy(),
            &budget,
            token.signal(),
        )
        .expect("valid request");
        assert!(outcome.is_unavailable());
        assert_eq!(
            outcome.reason(),
            Some("UNAVAILABLE: identity-bound launch primitive not available")
        );
    }
}
