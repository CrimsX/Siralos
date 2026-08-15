//! Generic reference-result model (Stage 3R R5).
//!
//! A reference result is a bounded, deterministic query result over
//! source locations: input order is preserved (the same deterministic
//! ordering the definition normalization uses), the per-query bound is
//! applied with explicit truncation, and the result binds to the exact
//! source revision it describes. No semantic graph, global index, or
//! cross-repository database is built here.

use crate::language::limits::LANGUAGE_LIMITS;
use crate::language::position::Location;

/// Limits for one reference query normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceLimits {
    /// Maximum locations retained per query.
    pub max_locations: usize,
}

/// A bounded reference-result for one document query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceResult {
    /// Workspace-relative path of the query document.
    pub path: String,
    /// R4 revision handle of the exact source state observed.
    pub revision: Option<String>,
    /// Reference locations in input order (deterministic).
    pub locations: Vec<Location>,
    /// True when the per-query bound was applied.
    pub truncated: bool,
}

/// Normalize one reference collection: preserve input order, apply the
/// per-query bound with explicit truncation, and bind the revision.
pub fn normalize_references(
    path: &str,
    revision: Option<String>,
    locations: Vec<Location>,
    limits: ReferenceLimits,
) -> ReferenceResult {
    let truncated = locations.len() > limits.max_locations;
    let mut locations = locations;
    locations.truncate(limits.max_locations);
    ReferenceResult { path: path.to_owned(), revision, locations, truncated }
}

impl Default for ReferenceLimits {
    fn default() -> Self {
        Self { max_locations: LANGUAGE_LIMITS.max_references_per_query }
    }
}
