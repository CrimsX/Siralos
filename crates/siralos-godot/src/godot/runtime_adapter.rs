//! Stage 4.3 Godot Runtime Adapter (decision 41) — the first specialization
//! of the generic runtime boundary.
//!
//! Consumes `siralos_core::runtime` (Stage 4.1, decision 36): launch requests
//! route through the generic host decision table, and engine runs carry the
//! generic bounded `RuntimeEvidence` projection plus Godot-specific
//! structured detail. The adapter never spawns a process, never touches the
//! filesystem, and never reads ambient state: the identity-bound launch
//! primitive is absent on this platform, so every otherwise-valid launch is
//! typed `UNAVAILABLE` with the generic reason verbatim.
//!
//! Boundary rules frozen by decision 41 (C1–C6):
//! - engine selection is consumed as runtime input (id + version), never
//!   re-discovered here;
//! - the command handed to the generic table is the engine identifier (never
//!   a filesystem path and never a spawnable command line): the absent
//!   identity-bound primitive plus the architecture-owned runner modules
//!   would materialize any real invocation tuple;
//! - Godot-shaped failure classification uses the closed
//!   `RuntimeFailureKind` vocabulary only — the domain can never extend it;
//! - LSP-only launches never accept project arguments (mirrors
//!   `FORBIDDEN_GODOT_PROJECT_ARGUMENTS`); every other launch mode requires
//!   a validated workspace-relative project path.

use std::collections::BTreeMap;

use siralos_core::identity::{CanonicalValue, compute_artifact_digest};
use siralos_core::runtime::{
    IDENTITY_BOUND_UNAVAILABLE_REASON, MAX_OPERATION_ID_BYTES,
    MAX_RUN_ID_BYTES, RuntimeBudget, RuntimeError, RuntimeEvidence,
    RuntimeEvidenceInput, RuntimeExecutionOutcome, RuntimeExecutionRequest,
    RuntimeFailureKind, create_runtime_evidence,
    decide_runtime_execution_with_flag, render_runtime_evidence,
};
use siralos_core::tool::capability::CapabilityId;
use siralos_core::tool::permission::{
    PermissionPolicy, PermissionRule, PolicyRule,
};
use siralos_core::workspace::WorkspaceRelativePath;

/// Maximum engine-id length in bytes (identifier bound).
pub const MAX_ENGINE_ID_BYTES: usize = 128;

/// Maximum engine-version length in bytes.
pub const MAX_ENGINE_VERSION_BYTES: usize = 64;

/// The Godot launch shape. The modes mirror the structurally paired
/// invocation tuples owned by the architecture runner modules
/// (project run, check-only diagnostics, recovery project, LSP-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GodotLaunchMode {
    /// Launch the project (editor or game run tuple).
    Project,
    /// GDScript check-only diagnostics tuple.
    CheckOnly,
    /// Recovery project tuple.
    RecoveryProject,
    /// LSP-only tuple; never accepts project arguments.
    LspOnly,
}

impl GodotLaunchMode {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::CheckOnly => "check-only",
            Self::RecoveryProject => "recovery-project",
            Self::LspOnly => "lsp-only",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "project" => Some(Self::Project),
            "check-only" => Some(Self::CheckOnly),
            "recovery-project" => Some(Self::RecoveryProject),
            "lsp-only" => Some(Self::LspOnly),
            _ => None,
        }
    }
}

/// Validated inputs for [`decide_godot_launch`]. Engine selection
/// (id + version) is consumed as runtime input; the adapter never
/// discovers, probes, or resolves engine installations itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotLaunchRequest {
    /// Selected engine identifier (bounded, no NUL, never a path).
    pub engine_id: String,
    /// Selected engine version label (bounded, no NUL).
    pub engine_version: String,
    /// Workspace-relative project path; required for every mode except
    /// [`GodotLaunchMode::LspOnly`], which must leave it empty.
    pub project_path: String,
    /// Launch shape; determines the project-path pairing rule.
    pub mode: GodotLaunchMode,
    /// Owning run id (non-empty, bounded).
    pub run_id: String,
    /// Optional operation id; the generic table derives one when absent.
    pub operation_id: Option<String>,
    /// Whether the observed revision is stale.
    pub is_stale: bool,
    /// Requested artifact bytes for the generic budget gate.
    pub requested_bytes: u64,
}

/// Godot-shaped engine detail echoed onto a launch decision. Paths are
/// echoed in their exact validated spelling; the detail carries no
/// absolute path, executable path, or credential material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotLaunchEngineDetail {
    /// Selected engine identifier (verbatim input).
    pub engine_id: String,
    /// Selected engine version label (verbatim input).
    pub engine_version: String,
    /// Validated project path (verbatim spelling; empty for LSP-only).
    pub project_path: String,
    /// Launch shape.
    pub mode: GodotLaunchMode,
}

/// Deterministic Godot launch decision: the generic decision-table outcome
/// plus the engine detail and a failure-kind slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotLaunchDecision {
    /// Outcome of the generic host decision table.
    pub outcome: RuntimeExecutionOutcome,
    /// Engine detail echoed onto the decision.
    pub engine: GodotLaunchEngineDetail,
    /// Engine-specific failure classification. Always `None` in this
    /// slice: no engine ever runs while the launch primitive is absent.
    /// The type is the closed generic vocabulary; the domain can never
    /// extend it (decision 41 C2).
    pub failure_kind: Option<RuntimeFailureKind>,
}

fn validate_engine_identity(
    engine_id: &str,
    engine_version: &str,
) -> Result<(), RuntimeError> {
    if engine_id.is_empty() {
        return Err(RuntimeError {
            message: "A Godot runtime launch requires an engine id."
                .to_owned(),
        });
    }
    if engine_id.len() > MAX_ENGINE_ID_BYTES {
        return Err(RuntimeError {
            message: format!(
                "The Godot runtime launch engine id exceeds the {MAX_ENGINE_ID_BYTES}-byte bound."
            ),
        });
    }
    if engine_id.contains('\0') {
        return Err(RuntimeError {
            message:
                "The Godot runtime launch engine id contains a null byte."
                    .to_owned(),
        });
    }
    if engine_version.is_empty() {
        return Err(RuntimeError {
            message: "A Godot runtime launch requires an engine version."
                .to_owned(),
        });
    }
    if engine_version.len() > MAX_ENGINE_VERSION_BYTES {
        return Err(RuntimeError {
            message: format!(
                "The Godot runtime launch engine version exceeds the {MAX_ENGINE_VERSION_BYTES}-byte bound."
            ),
        });
    }
    if engine_version.contains('\0') {
        return Err(RuntimeError {
            message:
                "The Godot runtime launch engine version contains a null byte."
                    .to_owned(),
        });
    }
    Ok(())
}

fn validate_mode_project_path_pairing(
    mode: GodotLaunchMode,
    project_path: &str,
) -> Result<(), RuntimeError> {
    if mode == GodotLaunchMode::LspOnly {
        if !project_path.is_empty() {
            return Err(RuntimeError {
                message:
                    "LSP-only Godot launches never accept project arguments."
                        .to_owned(),
            });
        }
        return Ok(());
    }
    if project_path.is_empty() {
        return Err(RuntimeError {
            message: format!(
                "A Godot runtime launch requires a project path for mode {}.",
                mode.as_str()
            ),
        });
    }
    if let Err(error) = WorkspaceRelativePath::parse(project_path) {
        return Err(RuntimeError {
            message: format!(
                "The Godot runtime launch project path is invalid: {error}"
            ),
        });
    }
    Ok(())
}

fn validate_ids(
    run_id: &str,
    operation_id: Option<&str>,
) -> Result<(), RuntimeError> {
    if run_id.is_empty() {
        return Err(RuntimeError {
            message: "A Godot runtime launch requires a run id.".to_owned(),
        });
    }
    if run_id.len() > MAX_RUN_ID_BYTES {
        return Err(RuntimeError {
            message: format!(
                "The Godot runtime launch run id exceeds the {MAX_RUN_ID_BYTES}-byte bound."
            ),
        });
    }
    if let Some(operation_id) = operation_id {
        if operation_id.is_empty() {
            return Err(RuntimeError {
                message:
                    "A Godot runtime launch operation id cannot be empty."
                        .to_owned(),
            });
        }
        if operation_id.len() > MAX_OPERATION_ID_BYTES {
            return Err(RuntimeError {
                message: format!(
                    "The Godot runtime launch operation id exceeds the {MAX_OPERATION_ID_BYTES}-byte bound."
                ),
            });
        }
    }
    Ok(())
}

/// Decide a Godot launch through the generic host decision table.
///
/// Decision order is the generic table's own order (validation, capability,
/// staleness, budget, cancellation, primitive). The command handed to the
/// table is the engine identifier; arguments are always empty because the
/// adapter never constructs an invocation tuple.
pub fn decide_godot_launch(
    request: &GodotLaunchRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    is_cancelled: bool,
) -> Result<GodotLaunchDecision, RuntimeError> {
    validate_engine_identity(&request.engine_id, &request.engine_version)?;
    validate_mode_project_path_pairing(request.mode, &request.project_path)?;
    validate_ids(&request.run_id, request.operation_id.as_deref())?;
    let generic_request = RuntimeExecutionRequest {
        command: request.engine_id.clone(),
        args: Vec::new(),
        run_id: request.run_id.clone(),
        operation_id: request.operation_id.clone(),
        is_stale: request.is_stale,
        requested_bytes: request.requested_bytes,
    };
    let outcome = decide_runtime_execution_with_flag(
        &generic_request,
        policy,
        budget,
        is_cancelled,
    )?;
    Ok(GodotLaunchDecision {
        outcome,
        engine: GodotLaunchEngineDetail {
            engine_id: request.engine_id.clone(),
            engine_version: request.engine_version.clone(),
            project_path: request.project_path.clone(),
            mode: request.mode,
        },
        failure_kind: None,
    })
}

/// The unavailable-reason string surfaced by every otherwise-valid launch
/// on this platform (the generic constant, verbatim).
#[must_use]
pub const fn godot_launch_unavailable_reason() -> &'static str {
    IDENTITY_BOUND_UNAVAILABLE_REASON
}

/// Godot-specific structured detail bound to a runtime evidence projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotRuntimeEvidenceDetail {
    /// Engine identifier (verbatim, validated).
    pub engine_id: String,
    /// Engine version label (verbatim, validated).
    pub engine_version: String,
    /// Validated project path (empty for LSP-only).
    pub project_path: String,
    /// Launch shape.
    pub mode: GodotLaunchMode,
}

/// Generic bounded runtime evidence plus Godot-structured detail, bound by
/// a domain-separated digest over the generic evidence digest and detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotRuntimeEvidence {
    /// Generic bounded evidence (1 MiB scalar-safe bounds, explicit
    /// truncation flag, content artifact digest).
    pub evidence: RuntimeEvidence,
    /// Godot-shaped detail (never raw engine streams).
    pub detail: GodotRuntimeEvidenceDetail,
    /// Domain-separated digest over `GodotRuntimeEvidence v1`.
    pub godot_digest: String,
}

/// Create Godot runtime evidence: the generic projection plus structured
/// engine detail under a domain-separated digest.
pub fn create_godot_runtime_evidence(
    evidence_input: &RuntimeEvidenceInput,
    detail: &GodotRuntimeEvidenceDetail,
) -> Result<GodotRuntimeEvidence, RuntimeError> {
    let evidence = create_runtime_evidence(evidence_input)?;
    validate_engine_identity(&detail.engine_id, &detail.engine_version)?;
    validate_mode_project_path_pairing(detail.mode, &detail.project_path)?;
    let payload = CanonicalValue::Object(BTreeMap::from([
        (
            "detail".to_owned(),
            CanonicalValue::Object(BTreeMap::from([
                (
                    "engineId".to_owned(),
                    CanonicalValue::Str(detail.engine_id.clone()),
                ),
                (
                    "engineVersion".to_owned(),
                    CanonicalValue::Str(detail.engine_version.clone()),
                ),
                (
                    "projectPath".to_owned(),
                    CanonicalValue::Str(detail.project_path.clone()),
                ),
                (
                    "mode".to_owned(),
                    CanonicalValue::Str(detail.mode.as_str().to_owned()),
                ),
            ])),
        ),
        (
            "evidenceDigest".to_owned(),
            CanonicalValue::Str(evidence.digest.clone()),
        ),
    ]));
    let godot_digest =
        compute_artifact_digest("GodotRuntimeEvidence", 1, &payload)
            .map_err(|error| RuntimeError { message: error.message })?
            .value;
    Ok(GodotRuntimeEvidence {
        evidence,
        detail: GodotRuntimeEvidenceDetail {
            engine_id: detail.engine_id.clone(),
            engine_version: detail.engine_version.clone(),
            project_path: detail.project_path.clone(),
            mode: detail.mode,
        },
        godot_digest,
    })
}

/// Bounded deterministic evidence rendering: the generic projection line
/// plus the engine detail (lengths and digests only, never streams).
#[must_use]
pub fn render_godot_runtime_evidence(
    evidence: &GodotRuntimeEvidence,
) -> String {
    let project = if evidence.detail.project_path.is_empty() {
        "-"
    } else {
        &evidence.detail.project_path
    };
    format!(
        "{} engine={} version={} mode={} project={}",
        render_runtime_evidence(&evidence.evidence),
        evidence.detail.engine_id,
        evidence.detail.engine_version,
        evidence.detail.mode.as_str(),
        project
    )
}

/// Convenience policy used by tests and the harness driver: a single
/// `process.execute` rule.
#[must_use]
pub fn godot_launch_policy(rule: PermissionRule) -> PermissionPolicy {
    let capability =
        CapabilityId::parse(siralos_core::runtime::PROCESS_EXECUTE_CAPABILITY)
            .expect("process.execute is a valid capability id");
    PermissionPolicy::from_rules(vec![PolicyRule { capability, rule }])
}

#[cfg(test)]
mod tests {
    use super::{
        GodotLaunchMode, GodotLaunchRequest, RuntimeError,
        create_godot_runtime_evidence, decide_godot_launch,
        godot_launch_policy, godot_launch_unavailable_reason,
        render_godot_runtime_evidence,
    };
    use siralos_core::runtime::{
        RuntimeBudgetInput, RuntimeEvidenceInput, create_runtime_budget,
        is_identity_bound_launch_primitive_available,
    };
    use siralos_core::tool::permission::PermissionRule;

    fn launch_request() -> GodotLaunchRequest {
        GodotLaunchRequest {
            engine_id: "godot-4.3-stable".to_owned(),
            engine_version: "4.3.stable".to_owned(),
            project_path: "game/project.godot".to_owned(),
            mode: GodotLaunchMode::Project,
            run_id: "run_godot_abc".to_owned(),
            operation_id: Some("op_g1".to_owned()),
            is_stale: false,
            requested_bytes: 1024,
        }
    }

    fn budget() -> siralos_core::runtime::RuntimeBudget {
        create_runtime_budget(&RuntimeBudgetInput {
            artifact_bytes: Some(64 * 1024 * 1024),
            ..Default::default()
        })
    }

    #[test]
    fn launch_dispositions_follow_the_generic_table_order() {
        let request = launch_request();
        // Capability denial fires first.
        let denied = decide_godot_launch(
            &request,
            &godot_launch_policy(PermissionRule::Deny),
            &budget(),
            false,
        )
        .expect("valid");
        assert_eq!(denied.outcome.disposition().as_str(), "COMMAND_DENIED");
        assert_eq!(denied.engine.engine_id, "godot-4.3-stable");
        assert!(denied.failure_kind.is_none());
        // Staleness fires before budget and cancellation.
        let mut stale = launch_request();
        stale.is_stale = true;
        let stale = decide_godot_launch(
            &stale,
            &godot_launch_policy(PermissionRule::Allow),
            &budget(),
            false,
        )
        .expect("valid");
        assert_eq!(stale.outcome.disposition().as_str(), "STALE");
        // Budget fires before cancellation.
        let mut over = launch_request();
        over.requested_bytes = 64 * 1024 * 1024 + 1;
        let over = decide_godot_launch(
            &over,
            &godot_launch_policy(PermissionRule::Allow),
            &budget(),
            false,
        )
        .expect("valid");
        assert_eq!(over.outcome.disposition().as_str(), "RESOURCE_EXCEEDED");
        // Cancellation fires before the primitive gate.
        let cancelled = decide_godot_launch(
            &request,
            &godot_launch_policy(PermissionRule::Allow),
            &budget(),
            true,
        )
        .expect("valid");
        assert_eq!(cancelled.outcome.disposition().as_str(), "CANCELLED");
        // Otherwise the absent primitive gates: typed UNAVAILABLE, 0 spawn.
        let unavailable = decide_godot_launch(
            &request,
            &godot_launch_policy(PermissionRule::Allow),
            &budget(),
            false,
        )
        .expect("valid");
        assert_eq!(unavailable.outcome.disposition().as_str(), "UNAVAILABLE");
        assert!(unavailable.outcome.is_unavailable());
        assert!(unavailable.outcome.reason().is_some_and(|reason| {
            reason.contains(godot_launch_unavailable_reason())
        }));
        assert!(!is_identity_bound_launch_primitive_available());
    }

    #[test]
    fn lsp_only_launches_never_accept_project_arguments() {
        let mut request = launch_request();
        request.mode = GodotLaunchMode::LspOnly;
        let refused = decide_godot_launch(
            &request,
            &godot_launch_policy(PermissionRule::Allow),
            &budget(),
            false,
        )
        .expect_err("pairing violation is refused");
        assert_eq!(
            refused.message,
            "LSP-only Godot launches never accept project arguments."
        );
        // LSP-only with an empty project path is well-formed and reaches
        // the generic table.
        request.project_path = String::new();
        let decided = decide_godot_launch(
            &request,
            &godot_launch_policy(PermissionRule::Allow),
            &budget(),
            false,
        )
        .expect("valid");
        assert_eq!(decided.outcome.disposition().as_str(), "UNAVAILABLE");
        assert_eq!(decided.engine.project_path, "");
    }

    #[test]
    fn other_launch_modes_require_a_validated_project_path() {
        let mut request = launch_request();
        request.project_path = String::new();
        let missing = decide_godot_launch(
            &request,
            &godot_launch_policy(PermissionRule::Allow),
            &budget(),
            false,
        )
        .expect_err("project mode requires a path");
        assert!(missing.message.contains("requires a project path"));
        request.project_path = "../escape/project.godot".to_owned();
        let escape = decide_godot_launch(
            &request,
            &godot_launch_policy(PermissionRule::Allow),
            &budget(),
            false,
        )
        .expect_err("traversal is rejected");
        assert!(escape.message.contains("invalid"));
    }

    #[test]
    fn evidence_binds_detail_under_a_domain_separated_digest() {
        let evidence_input = RuntimeEvidenceInput {
            run_id: "run_godot_abc".to_owned(),
            operation_id: "op_g1".to_owned(),
            exit_code: Some(0),
            duration_ms: 1500,
            stdout: "godot headless ok\n".to_owned(),
            stderr: String::new(),
        };
        let detail = super::GodotRuntimeEvidenceDetail {
            engine_id: "godot-4.3-stable".to_owned(),
            engine_version: "4.3.stable".to_owned(),
            project_path: "game/project.godot".to_owned(),
            mode: GodotLaunchMode::CheckOnly,
        };
        let first = create_godot_runtime_evidence(&evidence_input, &detail)
            .expect("valid");
        assert_eq!(first.godot_digest.len(), 64);
        let again = create_godot_runtime_evidence(&evidence_input, &detail)
            .expect("valid");
        assert_eq!(first.godot_digest, again.godot_digest);
        // A different engine id binds to a different digest.
        let mut other_detail = detail.clone();
        other_detail.engine_id = "godot-4.2-stable".to_owned();
        let other =
            create_godot_runtime_evidence(&evidence_input, &other_detail)
                .expect("valid");
        assert_ne!(first.godot_digest, other.godot_digest);
        // Rendering is deterministic and names the detail.
        let rendered = render_godot_runtime_evidence(&first);
        assert!(rendered.contains("engine=godot-4.3-stable"));
        assert!(rendered.contains("mode=check-only"));
        assert!(rendered.contains("project=game/project.godot"));
        // Detail violations surface as typed errors.
        let mut bad = detail.clone();
        bad.mode = GodotLaunchMode::LspOnly;
        let refused = create_godot_runtime_evidence(&evidence_input, &bad)
            .expect_err("pairing violation is refused");
        assert_eq!(
            refused.message,
            "LSP-only Godot launches never accept project arguments."
        );
    }

    #[test]
    fn engine_identity_and_id_bounds_are_typed() {
        let mut request = launch_request();
        request.engine_id = String::new();
        let missing: RuntimeError = decide_godot_launch(
            &request,
            &godot_launch_policy(PermissionRule::Allow),
            &budget(),
            false,
        )
        .expect_err("engine id is required");
        assert_eq!(
            missing.message,
            "A Godot runtime launch requires an engine id."
        );
        let mut request = launch_request();
        request.run_id = String::new();
        let missing = decide_godot_launch(
            &request,
            &godot_launch_policy(PermissionRule::Allow),
            &budget(),
            false,
        )
        .expect_err("run id is required");
        assert_eq!(
            missing.message,
            "A Godot runtime launch requires a run id."
        );
    }
}
