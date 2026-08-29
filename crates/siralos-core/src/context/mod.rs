//! Interpretable Context Architecture (Stage 3 — ICM, ADR 0030;
//! Stage 3R R10b).
//!
//! Mirrors `packages/core/src/context/**`: typed phase contracts with
//! narrowing-only authority, digest-bound dependency manifests,
//! targeted content-addressed staleness, and deterministic provenance /
//! why-diagnostics. Context is reconstructable from host-owned state;
//! provenance and authority are carried explicitly, never inferred.
//!
//! Submodules:
//! - [`artifacts`] — digest-bound artifact dependency manifests
//! - [`phase_contract`] — PhaseContract and the fixed authority vocabulary
//! - [`provenance`] — provenance references and why-diagnostics
//! - [`staleness`] — targeted incremental staleness propagation
//!
//! Scope boundaries (decision 15): the projection extension
//! (`projection.ts`) is covered by the existing `context-projection`
//! subjects; the executor briefing family (`executor/brief-compiler.ts`)
//! waits on S3M8-11 manifests; artifact envelope identity and lineage
//! rendering are not exercised by any wired subject yet.

mod artifacts;
mod controls;
mod phase_contract;
mod provenance;
mod staleness;

pub use artifacts::{
    ArtifactDependency, ArtifactDependencyManifest, HighValueDependency,
    build_dependency_manifest, create_artifact_dependency_manifest,
    high_value_dependencies,
};
pub use controls::{
    ContextControlEvidence, ContextControlOutcome, ContextPolicy,
    create_context_control_evidence, evaluate_context_policy,
    render_context_control_evidence,
};
pub use phase_contract::{
    CONTEXT_CLASS_ARTIFACT_KINDS, CONTEXT_CLASSES, ContextClass,
    CreatePhaseContractInput, PhaseAuthorityProfile,
    PhaseAuthorityProfileInput, PhaseContract, PhaseContractId,
    PhaseInputRequirement, PhaseMutation, PhaseOperation,
    PhaseOutputRequirement, PhaseVerificationRequirement,
    class_artifact_kinds, context_classes_for_phase, create_phase_contract,
    phase_contracts, validate_authority_profile,
};
pub use provenance::{
    ContextProvenanceRef, ContextProvenanceRefSource, WhyAcceptanceFailed,
    WhyBlocked, WhyStale, WhyValidationRequired, compute_provenance_digest,
    create_context_provenance_ref, render_why_acceptance_failed,
    render_why_blocked, render_why_stale, render_why_validation_required,
    why_validation_required,
};
pub use staleness::{
    ArtifactStalenessInput, ArtifactStalenessResult,
    PreparedMutationStaleness, compute_staleness_digest,
    derive_artifact_staleness, is_prepared_mutation_stale,
};

/// Validation failure at an ICM context boundary. The message mirrors
/// the TypeScript oracle exactly so both implementations reject
/// malformed contracts identically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextError {
    /// Bounded truthful message.
    pub message: String,
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ContextError {}

pub(crate) fn context_error(message: impl Into<String>) -> ContextError {
    ContextError { message: message.into() }
}
