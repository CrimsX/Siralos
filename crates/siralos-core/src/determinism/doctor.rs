//! Determinism doctor surface (Stage 3 — Deterministic Execution &
//! Reproducibility, ADR 0029; R10a H2).
//!
//! Mirrors `packages/core/src/determinism/doctor.ts`. Read-only,
//! offline, no mutation.

use super::environment::EnvironmentManifestInput;

/// One determinism doctor diagnostic result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeterminismDiagnosticResult {
    /// Clock mode in effect ("system" | "fixed" | "unknown").
    pub clock_mode: &'static str,
    /// Randomness mode in effect
    /// ("none" | "seeded" | "system" | "unknown").
    pub randomness_mode: &'static str,
    /// Locale policy when known.
    pub locale_policy: Option<String>,
    /// Timezone policy when known.
    pub timezone_policy: Option<String>,
    /// Digest of the environment snapshot when present.
    pub environment_digest: Option<String>,
    /// Digest of the recorded reproducibility manifest when present.
    pub reproducibility_digest: Option<String>,
    /// Static host guarantees provided by core policy modules.
    pub static_guarantees: DeterminismStaticGuarantees,
}

/// Static host guarantees — all true by construction in core policy
/// modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterminismStaticGuarantees {
    /// File ordering normalized before decisions.
    pub file_ordering_normalized: bool,
    /// Documentation selection deterministic.
    pub documentation_selection_deterministic: bool,
    /// Workspace scope deterministic.
    pub workspace_scope_deterministic: bool,
    /// Validation selection deterministic.
    pub validation_selection_deterministic: bool,
    /// Tool surface fingerprinted.
    pub tool_surface_fingerprinted: bool,
    /// Acceptance evaluation deterministic.
    pub acceptance_deterministic: bool,
    /// Nondeterminism audit clean.
    pub nondeterminism_audit_clean: bool,
}

/// Inputs for building a runtime environment manifest from explicit
/// observations.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeEnvironmentObservations {
    /// Siralos build version.
    pub siralos_version: Option<String>,
    /// Node runtime version.
    pub node_version: Option<String>,
    /// npm version.
    pub npm_version: Option<String>,
    /// Platform identifier.
    pub platform: Option<String>,
    /// Architecture identifier.
    pub arch: Option<String>,
    /// OS release string.
    pub os_release: Option<String>,
    /// Verified Godot executable fingerprint.
    pub godot_executable_fingerprint: Option<String>,
    /// Sandbox backend id.
    pub sandbox_backend_id: Option<String>,
    /// Sandbox version.
    pub sandbox_version: Option<String>,
    /// Explicit locale policy; defaults to "C".
    pub locale_policy: Option<String>,
    /// Explicit timezone policy; defaults to "UTC".
    pub timezone_policy: Option<String>,
    /// Environment allowlist — names only.
    pub environment_allowlist: Vec<String>,
}

/// Build the environment manifest input from explicit runtime
/// observations. Locale defaults to `"C"` and timezone to `"UTC"` —
/// never ambient values.
#[must_use]
pub fn build_runtime_environment_manifest(
    observations: &RuntimeEnvironmentObservations,
) -> EnvironmentManifestInput {
    EnvironmentManifestInput {
        siralos_version: observations.siralos_version.clone(),
        node_version: observations.node_version.clone(),
        npm_version: observations.npm_version.clone(),
        platform: observations.platform.clone(),
        arch: observations.arch.clone(),
        os_release: observations.os_release.clone(),
        godot_executable_fingerprint: observations
            .godot_executable_fingerprint
            .clone(),
        sandbox_backend_id: observations.sandbox_backend_id.clone(),
        sandbox_version: observations.sandbox_version.clone(),
        locale_policy: Some(
            observations
                .locale_policy
                .clone()
                .unwrap_or_else(|| "C".to_owned()),
        ),
        timezone_policy: Some(
            observations
                .timezone_policy
                .clone()
                .unwrap_or_else(|| "UTC".to_owned()),
        ),
        environment_allowlist: observations.environment_allowlist.clone(),
        tool_identities: Vec::new(),
    }
}
