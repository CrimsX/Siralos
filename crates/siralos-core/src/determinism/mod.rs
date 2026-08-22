//! Deterministic execution & reproducibility contracts (Stage 3 —
//! Deterministic Execution & Reproducibility, ADR 0029; R10a H2).
//!
//! Mirrors `packages/core/src/determinism/**`. Same authoritative inputs
//! → same host decision. Nondeterministic observations are recorded or
//! typed `unreplayable` — never silently replayed. Adapters own external
//! nondeterminism; core consumes it through explicit ports.
//!
//! Submodules:
//! - [`decisions`] — validation/acceptance/retry/concurrency/lease
//! - [`discovery`] — baseline discovery + ownership index
//! - [`doctor`] — determinism doctor surface
//! - [`environment`] — environment manifest
//! - [`ports`] — clock/random/ordering boundaries
//! - [`reproducibility`] — reproducibility reference set

pub mod decisions;
pub mod discovery;
pub mod doctor;
pub mod environment;
pub mod helpers;
pub mod ownership;
pub mod ports;
pub mod reproducibility;

pub use decisions::{
    AcceptanceInput, AcceptanceOutcome, AcceptanceResult,
    ActiveWorkingSetEntry, AvailableEvidence, DEFAULT_RETRY_POLICY,
    ImpactRelationship, LeaseEvaluation, RetryCategory, RetryClassification,
    RetryDecision, RetryPolicy, ValidationItem, ValidationPlan,
    ValidationPlanInput, ValidationRegistryEntry, WorkingSetCandidate,
    classify_retry, default_backoff_ms, derive_active_working_set,
    derive_validation_plan, evaluate_acceptance, evaluate_lease,
    normalize_concurrent_results,
};
pub use discovery::{
    DiscoveryCandidate, DiscoveryCandidateInput, DiscoveryInput,
    DiscoveryRelevance, DiscoveryResult, discover_repository,
};
pub use doctor::{
    DeterminismDiagnosticResult, DeterminismStaticGuarantees,
    RuntimeEnvironmentObservations, build_runtime_environment_manifest,
};
pub use environment::{
    ENVIRONMENT_SECTIONS, EnvironmentDelta, EnvironmentManifest,
    EnvironmentManifestInput, compute_environment_delta,
    create_environment_manifest,
};
pub use ownership::{
    OWNERSHIP_INDEX, OwnershipEntry, list_ownership, resolve_owner,
};
pub use ports::{
    Clock, FixedClock, IdKeyed, RandomSource, SeededRandomSource, SystemClock,
    compare_code_units, normalize_keyed_results, stable_sort_by_key,
};
pub use reproducibility::{
    ClockPolicy, ProviderInputIdentity, ReproducibilityManifest,
    ReproducibilityManifestInput, RngPolicy, SourceRevision,
    compute_provider_input_identity_digest, create_reproducibility_manifest,
};
