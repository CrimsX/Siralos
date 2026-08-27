//! Generic host-authorized controlled runtime execution (Stage 4.1, ADR 0035).
//!
//! Minimal deterministic host decision table for `process.execute`. No
//! process is ever launched, no filesystem is touched, and no ambient
//! environment is read. The table is pure: equivalent inputs produce the
//! same outcome. The identity-bound handle primitive is absent on this
//! platform, so the success path is typed `UNAVAILABLE` with the exact
//! reason `"identity-bound launch primitive not available"`.
//!
//! Decision order (first match wins):
//! 1. Input validation — typed [`RuntimeError`] (command/run-id bounds,
//!    argument bounds).
//! 2. Capability — `process.execute` via [`PermissionPolicy`] →
//!    `COMMAND_DENIED`.
//! 3. Staleness — stale revision → `STALE`.
//! 4. Budget — requested bytes exceed [`RuntimeBudget::artifact_bytes`] →
//!    `RESOURCE_EXCEEDED`.
//! 5. Cancellation — [`CancellationSignal`] cancelled → `CANCELLED`.
//! 6. Primitive — identity-bound handle absent → `UNAVAILABLE`.
//! 7. Otherwise `Success` (still `UNAVAILABLE` on this platform because the
//!    primitive is absent, so step 6 is the terminal success guard).

use std::collections::BTreeMap;

use crate::identity::{CanonicalValue, compute_artifact_digest};
use crate::provider::{CancellationSignal, CancellationToken};
use crate::runtime::budget::RuntimeBudget;
use crate::runtime::identity::create_operation_id;
use crate::tool::capability::CapabilityId;
use crate::tool::permission::{PermissionPolicy, evaluate_permission};

use super::{RuntimeError, runtime_error};

/// Capability required for any runtime execution.
pub const PROCESS_EXECUTE_CAPABILITY: &str = "process.execute";

/// The exact typed unavailability reason when the identity-bound primitive
/// is absent. Mirrors the oracle string verbatim.
pub const IDENTITY_BOUND_UNAVAILABLE_REASON: &str =
    "identity-bound launch primitive not available";

/// Maximum command length in bytes (UTF-8).
pub const MAX_COMMAND_BYTES: usize = 8192;

/// Maximum argument count.
pub const MAX_ARGS: usize = 64;

/// Maximum bytes per argument (UTF-8).
pub const MAX_ARG_BYTES: usize = 4096;

/// Maximum run-id length in bytes.
pub const MAX_RUN_ID_BYTES: usize = 128;

/// Maximum operation-id length in bytes.
pub const MAX_OPERATION_ID_BYTES: usize = 128;

/// Whether the identity-bound launch primitive is available on this
/// platform.
///
/// Stage 4.1 is fail-closed: the primitive is absent, so execution always
/// reports typed `UNAVAILABLE` without mutation or spawn. No filesystem
/// probe, no ambient check, no wall clock is consulted.
#[must_use]
pub const fn is_identity_bound_launch_primitive_available() -> bool {
    false
}

/// Terminal disposition of the host execution decision table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeExecutionDisposition {
    /// All gates passed; execution would proceed if the primitive were
    /// available.
    Success,
    /// Capability policy denied `process.execute`.
    CommandDenied,
    /// Revision is stale.
    Stale,
    /// Request exceeds [`RuntimeBudget`].
    ResourceExceeded,
    /// Host cancellation observed.
    Cancelled,
    /// Identity-bound primitive absent.
    Unavailable,
}

impl RuntimeExecutionDisposition {
    /// Canonical protocol string (deterministic, oracle-matched).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::CommandDenied => "COMMAND_DENIED",
            Self::Stale => "STALE",
            Self::ResourceExceeded => "RESOURCE_EXCEEDED",
            Self::Cancelled => "CANCELLED",
            Self::Unavailable => "UNAVAILABLE",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "COMMAND_DENIED" => Some(Self::CommandDenied),
            "STALE" => Some(Self::Stale),
            "RESOURCE_EXCEEDED" => Some(Self::ResourceExceeded),
            "CANCELLED" => Some(Self::Cancelled),
            "UNAVAILABLE" => Some(Self::Unavailable),
            _ => None,
        }
    }
}

/// Validated inputs for [`decide_runtime_execution`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeExecutionRequest {
    /// Command to execute (non-empty, bounded UTF-8, no NUL).
    pub command: String,
    /// Arguments (bounded count and length).
    pub args: Vec<String>,
    /// Owning run id (non-empty, bounded).
    pub run_id: String,
    /// Optional operation id; when absent a deterministic id is derived.
    pub operation_id: Option<String>,
    /// Whether the observed revision is stale.
    pub is_stale: bool,
    /// Requested artifact bytes for budget comparison.
    pub requested_bytes: u64,
}

impl RuntimeExecutionRequest {
    /// Create a validated request; validation is also performed by
    /// [`decide_runtime_execution`] so callers may construct directly.
    pub fn new(
        command: String,
        args: Vec<String>,
        run_id: String,
        operation_id: Option<String>,
        is_stale: bool,
        requested_bytes: u64,
    ) -> Result<Self, RuntimeError> {
        let request = Self {
            command,
            args,
            run_id,
            operation_id,
            is_stale,
            requested_bytes,
        };
        validate_runtime_execution_request(&request)?;
        Ok(request)
    }
}

/// Deterministic host execution outcome. Every non-success variant carries
/// the exact typed reason that the differential harness matches verbatim;
/// `Success` is the only outcome without a reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeExecutionOutcome {
    /// All gates passed; execution would proceed if the primitive were
    /// available (on this platform this is still guarded by the
    /// `UNAVAILABLE` step, so `Success` is only observable when the
    /// primitive reports available).
    Success {
        /// Owning run id.
        run_id: String,
        /// Operation within the run.
        operation_id: String,
    },
    /// Capability denied `process.execute`.
    Denied {
        /// Typed reason, prefixed with `COMMAND_DENIED`.
        reason: String,
    },
    /// Stale revision.
    Stale {
        /// Typed reason, prefixed with `STALE`.
        reason: String,
    },
    /// Budget exceeded.
    ResourceExceeded {
        /// Typed reason, prefixed with `RESOURCE_EXCEEDED`.
        reason: String,
    },
    /// Host cancellation observed.
    Cancelled {
        /// Typed reason, prefixed with `CANCELLED`.
        reason: String,
    },
    /// Identity-bound primitive absent.
    Unavailable {
        /// Typed reason, prefixed with `UNAVAILABLE`.
        reason: String,
    },
}

impl RuntimeExecutionOutcome {
    /// Terminal disposition for this outcome.
    #[must_use]
    pub fn disposition(&self) -> RuntimeExecutionDisposition {
        match self {
            Self::Success { .. } => RuntimeExecutionDisposition::Success,
            Self::Denied { .. } => RuntimeExecutionDisposition::CommandDenied,
            Self::Stale { .. } => RuntimeExecutionDisposition::Stale,
            Self::ResourceExceeded { .. } => {
                RuntimeExecutionDisposition::ResourceExceeded
            }
            Self::Cancelled { .. } => RuntimeExecutionDisposition::Cancelled,
            Self::Unavailable { .. } => {
                RuntimeExecutionDisposition::Unavailable
            }
        }
    }

    /// Typed reason for non-success outcomes; `None` for success.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Success { .. } => None,
            Self::Denied { reason }
            | Self::Stale { reason }
            | Self::ResourceExceeded { reason }
            | Self::Cancelled { reason }
            | Self::Unavailable { reason } => Some(reason),
        }
    }

    /// Whether the outcome is the typed unavailable primitive-absent case.
    #[must_use]
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}

/// Validate a runtime execution request. Mirrors the oracle's exact
/// rejection messages so both implementations reject malformed inputs
/// identically.
fn validate_runtime_execution_request(
    request: &RuntimeExecutionRequest,
) -> Result<(), RuntimeError> {
    if request.command.is_empty() || request.command.trim().is_empty() {
        return Err(runtime_error("A runtime execution requires a command."));
    }
    if request.command.len() > MAX_COMMAND_BYTES {
        return Err(runtime_error(format!(
            "The runtime command exceeds the {MAX_COMMAND_BYTES}-byte bound."
        )));
    }
    if request.command.contains('\0') {
        return Err(runtime_error(
            "The runtime command must not contain NUL.",
        ));
    }
    if request.args.len() > MAX_ARGS {
        return Err(runtime_error(format!(
            "The runtime args exceed the {MAX_ARGS} entry bound."
        )));
    }
    for arg in &request.args {
        if arg.len() > MAX_ARG_BYTES {
            return Err(runtime_error(format!(
                "A runtime arg exceeds the {MAX_ARG_BYTES}-byte bound."
            )));
        }
        if arg.contains('\0') {
            return Err(runtime_error("A runtime arg must not contain NUL."));
        }
    }
    if request.run_id.is_empty() {
        return Err(runtime_error("A runtime execution requires a run id."));
    }
    if request.run_id.len() > MAX_RUN_ID_BYTES {
        return Err(runtime_error(format!(
            "The runtime run id exceeds the {MAX_RUN_ID_BYTES}-byte bound."
        )));
    }
    if let Some(operation_id) = &request.operation_id {
        if operation_id.is_empty() {
            return Err(runtime_error(
                "A runtime operation id must not be empty.",
            ));
        }
        if operation_id.len() > MAX_OPERATION_ID_BYTES {
            return Err(runtime_error(format!(
                "The runtime operation id exceeds the {MAX_OPERATION_ID_BYTES}-byte bound."
            )));
        }
    }
    Ok(())
}

/// Host-authorized execution decision table.
///
/// The function is pure and deterministic: equivalent inputs produce the
/// same [`RuntimeExecutionOutcome`]. It performs no I/O, no spawn, no
/// filesystem access, and no wall-clock read.
///
/// # Errors
///
/// Returns [`RuntimeError`] only for malformed inputs; all policy/capability,
/// staleness, budget, cancellation, and primitive-absent cases are typed
/// [`RuntimeExecutionOutcome`] variants, never errors.
pub fn decide_runtime_execution(
    request: &RuntimeExecutionRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    cancellation: CancellationSignal<'_>,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    validate_runtime_execution_request(request)?;

    // 2. Capability gate: process.execute must be allow.
    let capability = CapabilityId::parse(PROCESS_EXECUTE_CAPABILITY)
        .expect("process.execute is a valid capability id");
    let decision = evaluate_permission(&capability, policy);
    match decision {
        crate::tool::permission::PermissionDecision::Allow => {}
        crate::tool::permission::PermissionDecision::Ask { reason } => {
            return Ok(RuntimeExecutionOutcome::Denied {
                reason: format!("COMMAND_DENIED: {reason}"),
            });
        }
        crate::tool::permission::PermissionDecision::Deny { reason } => {
            return Ok(RuntimeExecutionOutcome::Denied {
                reason: format!("COMMAND_DENIED: {reason}"),
            });
        }
    }

    // 3. Staleness gate.
    if request.is_stale {
        return Ok(RuntimeExecutionOutcome::Stale {
            reason: "STALE: revision is stale.".to_owned(),
        });
    }

    // 4. Budget gate: requested bytes exceed the host budget.
    if request.requested_bytes > budget.artifact_bytes {
        return Ok(RuntimeExecutionOutcome::ResourceExceeded {
            reason: format!(
                "RESOURCE_EXCEEDED: requested {} exceeds budget {}.",
                request.requested_bytes, budget.artifact_bytes
            ),
        });
    }

    // 5. Cancellation gate.
    if cancellation.is_cancelled() {
        return Ok(RuntimeExecutionOutcome::Cancelled {
            reason: "CANCELLED: execution was cancelled.".to_owned(),
        });
    }

    // 6. Identity-bound primitive gate.
    if !is_identity_bound_launch_primitive_available() {
        return Ok(RuntimeExecutionOutcome::Unavailable {
            reason: format!(
                "UNAVAILABLE: {IDENTITY_BOUND_UNAVAILABLE_REASON}"
            ),
        });
    }

    // 7. Success (only observable when the primitive is available).
    let operation_id = request.operation_id.clone().unwrap_or_else(|| {
        create_operation_id(&request.run_id, &request.command)
    });
    Ok(RuntimeExecutionOutcome::Success {
        run_id: request.run_id.clone(),
        operation_id,
    })
}

/// Convenience wrapper that accepts a simple cancellation flag. Useful for
/// unit tests and for callers that already materialized a boolean.
pub fn decide_runtime_execution_with_flag(
    request: &RuntimeExecutionRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    is_cancelled: bool,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    let token = CancellationToken::new();
    if is_cancelled {
        token.cancel();
    }
    decide_runtime_execution(request, policy, budget, token.signal())
}

/// Deterministic digest over a runtime execution outcome (for evidence
/// identity). Uses the single artifact-digest primitive.
#[must_use]
pub fn digest_runtime_execution_outcome(
    outcome: &RuntimeExecutionOutcome,
) -> String {
    let mut map = BTreeMap::new();
    map.insert(
        "disposition".to_owned(),
        CanonicalValue::Str(outcome.disposition().as_str().to_owned()),
    );
    match outcome.reason() {
        Some(reason) => {
            map.insert(
                "reason".to_owned(),
                CanonicalValue::Str(reason.to_owned()),
            );
        }
        None => {
            map.insert("reason".to_owned(), CanonicalValue::Null);
        }
    }
    if let RuntimeExecutionOutcome::Success { run_id, operation_id } = outcome
    {
        map.insert("runId".to_owned(), CanonicalValue::Str(run_id.clone()));
        map.insert(
            "operationId".to_owned(),
            CanonicalValue::Str(operation_id.clone()),
        );
    }
    let payload = CanonicalValue::Object(map);
    compute_artifact_digest("RuntimeExecutionOutcome", 1, &payload)
        .expect("runtime execution digest inputs are structurally valid")
        .value
}

#[cfg(test)]
mod tests {
    use super::{
        IDENTITY_BOUND_UNAVAILABLE_REASON, RuntimeExecutionOutcome,
        RuntimeExecutionRequest, decide_runtime_execution_with_flag,
        is_identity_bound_launch_primitive_available,
    };
    use crate::provider::CancellationToken;
    use crate::runtime::budget::{RuntimeBudgetInput, create_runtime_budget};
    use crate::tool::capability::CapabilityId;
    use crate::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };

    fn allow_policy() -> PermissionPolicy {
        PermissionPolicy::from_rules([PolicyRule {
            capability: CapabilityId::parse("process.execute").unwrap(),
            rule: PermissionRule::Allow,
        }])
    }

    fn deny_policy() -> PermissionPolicy {
        PermissionPolicy::from_rules([PolicyRule {
            capability: CapabilityId::parse("process.execute").unwrap(),
            rule: PermissionRule::Deny,
        }])
    }

    fn default_budget() -> crate::runtime::budget::RuntimeBudget {
        create_runtime_budget(&RuntimeBudgetInput::default())
    }

    fn valid_request() -> RuntimeExecutionRequest {
        RuntimeExecutionRequest {
            command: "echo".to_owned(),
            args: vec!["hello".to_owned()],
            run_id: "run_runtime_abc".to_owned(),
            operation_id: Some("op_123".to_owned()),
            is_stale: false,
            requested_bytes: 1024,
        }
    }

    #[test]
    fn primitive_is_absent_on_this_platform() {
        assert!(!is_identity_bound_launch_primitive_available());
        assert_eq!(
            IDENTITY_BOUND_UNAVAILABLE_REASON,
            "identity-bound launch primitive not available"
        );
    }

    #[test]
    fn validates_command_and_run_id_with_oracle_messages() {
        let mut request = valid_request();
        request.command = String::new();
        let error = decide_runtime_execution_with_flag(
            &request,
            &allow_policy(),
            &default_budget(),
            false,
        )
        .expect_err("empty command must be rejected");
        assert_eq!(error.message, "A runtime execution requires a command.");

        let mut request = valid_request();
        request.run_id = String::new();
        let error = decide_runtime_execution_with_flag(
            &request,
            &allow_policy(),
            &default_budget(),
            false,
        )
        .expect_err("empty run_id must be rejected");
        assert_eq!(error.message, "A runtime execution requires a run id.");

        let mut request = valid_request();
        request.args = vec!["a".repeat(5000)];
        let error = decide_runtime_execution_with_flag(
            &request,
            &allow_policy(),
            &default_budget(),
            false,
        )
        .expect_err("overlong arg must be rejected");
        assert!(error.message.contains("runtime arg exceeds"));
    }

    #[test]
    fn decision_table_covers_denied_stale_resource_cancelled_and_unavailable()
    {
        // COMMAND_DENIED via process.execute capability.
        let request = valid_request();
        let denied = decide_runtime_execution_with_flag(
            &request,
            &deny_policy(),
            &default_budget(),
            false,
        )
        .expect("valid");
        assert_eq!(
            denied,
            RuntimeExecutionOutcome::Denied {
                reason: "COMMAND_DENIED: Policy denies process.execute."
                    .to_owned()
            }
        );

        // STALE via revision.
        let mut stale = valid_request();
        stale.is_stale = true;
        let outcome = decide_runtime_execution_with_flag(
            &stale,
            &allow_policy(),
            &default_budget(),
            false,
        )
        .expect("valid");
        assert_eq!(
            outcome,
            RuntimeExecutionOutcome::Stale {
                reason: "STALE: revision is stale.".to_owned()
            }
        );

        // RESOURCE_EXCEEDED via RuntimeBudget.
        let mut exceeded = valid_request();
        exceeded.requested_bytes = default_budget().artifact_bytes + 1;
        let outcome = decide_runtime_execution_with_flag(
            &exceeded,
            &allow_policy(),
            &default_budget(),
            false,
        )
        .expect("valid");
        assert!(matches!(
            outcome,
            RuntimeExecutionOutcome::ResourceExceeded { .. }
        ));
        assert!(outcome.reason().unwrap().starts_with("RESOURCE_EXCEEDED"));

        // CANCELLED via CancellationSignal.
        let cancelled = {
            let token = CancellationToken::new();
            token.cancel();
            super::decide_runtime_execution(
                &valid_request(),
                &allow_policy(),
                &default_budget(),
                token.signal(),
            )
            .expect("valid")
        };
        assert_eq!(
            cancelled,
            RuntimeExecutionOutcome::Cancelled {
                reason: "CANCELLED: execution was cancelled.".to_owned()
            }
        );

        // UNAVAILABLE when primitive absent (success path on this platform).
        let unavailable = decide_runtime_execution_with_flag(
            &valid_request(),
            &allow_policy(),
            &default_budget(),
            false,
        )
        .expect("valid");
        assert_eq!(
            unavailable,
            RuntimeExecutionOutcome::Unavailable {
                reason: format!(
                    "UNAVAILABLE: {IDENTITY_BOUND_UNAVAILABLE_REASON}"
                )
            }
        );
        assert!(unavailable.is_unavailable());
    }

    #[test]
    fn digests_are_deterministic_and_domain_separated() {
        let outcome = RuntimeExecutionOutcome::Unavailable {
            reason: format!(
                "UNAVAILABLE: {IDENTITY_BOUND_UNAVAILABLE_REASON}"
            ),
        };
        let first = super::digest_runtime_execution_outcome(&outcome);
        let second = super::digest_runtime_execution_outcome(&outcome);
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        let success = RuntimeExecutionOutcome::Success {
            run_id: "run_runtime_abc".to_owned(),
            operation_id: "op_123".to_owned(),
        };
        assert_ne!(first, super::digest_runtime_execution_outcome(&success));
    }
}
