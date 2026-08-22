//! Unified Godot-native development core (Stage 3 milestone 11,
//! ADR 0027; R9 parity slice).
//!
//! Mirrors `packages/core/src/godot/development/` for the deterministic,
//! host-owned pieces frozen by the R9 entry review
//! (`docs/wayfinder/decisions/12-r9-entry-review.md`): surface routing,
//! dependency-based apply ordering, and structured blocked dispositions.
//! The interactive provider-facing session loop is not part of this
//! module.

pub mod disposition;
pub mod order;
pub mod surface;

pub use disposition::{
    BlockedDisposition, BlockedReasonKind, blocked_reason_text,
    create_blocked_disposition,
};
pub use order::{
    UnifiedApplyOrder, UnifiedOrderEdge, UnifiedOrderTarget,
    UnresolvedReference, derive_unified_apply_order,
    derive_unified_order_edges,
};
pub use surface::{
    DevelopmentSurfaceDecision, DevelopmentSurfaceInput,
    DevelopmentSurfaceKind, DevelopmentSurfaceTouchpoint, ProjectSurfaces,
    classify_development_surface, classify_development_surface_path,
};

/// Shared error type for the development core (bounded truthful
/// messages mirroring the oracle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentError {
    /// Bounded truthful message.
    pub message: String,
}

impl std::fmt::Display for DevelopmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for DevelopmentError {}
