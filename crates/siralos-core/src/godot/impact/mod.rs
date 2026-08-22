//! Review context and impact intelligence (Stage 3 milestone 9, ADR 0025;
//! R9 parity slice).
//!
//! Given changed surfaces plus an injected relationship evidence source,
//! the pure [`analyze_impact`] derivation produces a bounded,
//! revision-aware [`ReviewContextManifest`]: primary changes, related
//! surfaces with verified-vs-candidate confidence, regression areas,
//! validation recommendations, and honest completeness/diagnostics.
//!
//! The manifest is DERIVED state, never task authority: it grants no
//! capability, performs no mutation, launches no Godot process, and never
//! proves runtime impact — absence of a static relationship is not proof
//! of runtime non-impact, and stale relationship evidence is excluded and
//! disclosed rather than presented as current.
//!
//! Mirrors `packages/core/src/godot/impact/` (behavioral oracle). The
//! relationship source trait is synchronous: the core stays free of async
//! runtimes (`RUST_STYLE.md`); adapters implement IO synchronously under
//! their own bounded policies.

pub mod analyzer;
pub mod model;

pub use analyzer::{
    AnalyzeImpactInput, ImpactEdge, ImpactRelationshipSource,
    ImpactSignalConnection, analyze_impact,
};
pub use model::{
    ImpactCompleteness, ImpactConfidence, ImpactDiagnostic,
    ImpactRegressionArea, ImpactRelation, ImpactRelationKind, ImpactSurface,
    ImpactSurfaceKind, ImpactValidationRecommendation, ReviewContextError,
    ReviewContextLimits, ReviewContextManifest, ValidationKind,
    ValidationPriority, validate_review_context_manifest,
};
