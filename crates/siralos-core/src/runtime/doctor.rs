//! Runtime readiness doctor surface (Stage 3 — Runtime Readiness &
//! Operational Resilience, ADR 0031; R10c H3).
//!
//! Mirrors `packages/core/src/runtime/doctor.ts`. Read-only, offline,
//! never launches anything: the doctor projects readiness manifests for
//! both runtime modes from the same declared capability inputs.

use super::readiness::{
    RuntimeMode, RuntimeReadinessInput, RuntimeReadinessManifest,
    evaluate_runtime_readiness,
};

/// Declared capability inputs for the diagnostic. The mode field is
/// absent because the doctor evaluates BOTH modes.
#[derive(Debug, Clone, Default)]
pub struct DoctorCapabilities {
    /// Engine executable presence + fingerprint.
    pub godot_executable_available: bool,
    /// Engine fingerprint when bound.
    pub godot_executable_fingerprint: Option<String>,
    /// Project/workspace identity when resolved.
    pub project_identity: Option<String>,
    /// Sandbox backend presence.
    pub sandbox_available: bool,
    /// Process supervision support.
    pub process_supervision_supported: bool,
    /// Filesystem isolation availability.
    pub filesystem_isolation_available: bool,
    /// User-data redirect availability.
    pub user_data_redirect_available: bool,
    /// Network policy resolvability.
    pub network_policy_resolvable: bool,
    /// Artifact storage availability.
    pub artifact_storage_available: bool,
    /// Display availability (`None` = unknown).
    pub display_available: Option<bool>,
}

/// Readiness manifests for headless and visual modes from one declared
/// capability set.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeReadinessDiagnosticResult {
    /// Headless-mode readiness manifest.
    pub headless: RuntimeReadinessManifest,
    /// Visual-mode readiness manifest.
    pub visual: RuntimeReadinessManifest,
}

/// Build the runtime readiness diagnostic (headless + visual).
///
/// # Errors
///
/// Propagates only the digest primitive's infallible-for-constants
/// error type from readiness evaluation.
pub fn build_runtime_readiness_diagnostic(
    capabilities: &DoctorCapabilities,
) -> Result<RuntimeReadinessDiagnosticResult, super::RuntimeError> {
    let base = || RuntimeReadinessInput {
        runtime_mode: None,
        godot_executable_available: capabilities.godot_executable_available,
        godot_executable_fingerprint: capabilities
            .godot_executable_fingerprint
            .clone(),
        project_identity: capabilities.project_identity.clone(),
        sandbox_backend_available: capabilities.sandbox_available,
        sandbox_supports_process_supervision: capabilities
            .process_supervision_supported,
        filesystem_isolation_available: capabilities
            .filesystem_isolation_available,
        user_data_redirect_available: capabilities
            .user_data_redirect_available,
        network_policy_resolvable: capabilities.network_policy_resolvable,
        artifact_storage_available: capabilities.artifact_storage_available,
        display_available: capabilities.display_available,
        memory_limit_enforced: false,
        cpu_limit_enforced: false,
    };
    let mut headless_input = base();
    headless_input.runtime_mode = Some(RuntimeMode::Headless);
    let mut visual_input = base();
    visual_input.runtime_mode = Some(RuntimeMode::Visual);
    Ok(RuntimeReadinessDiagnosticResult {
        headless: evaluate_runtime_readiness(&headless_input)?,
        visual: evaluate_runtime_readiness(&visual_input)?,
    })
}
