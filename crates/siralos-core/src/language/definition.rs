//! Generic go-to-definition result normalization (Stage 3R R5).
//!
//! LocationLink (`targetUri`/`targetRange`) and plain Location
//! (`uri`/`range`) forms are accepted, 0-based positions are converted
//! to the 1-based Siralos convention, input order is preserved, results
//! are bounded with explicit truncation, and locations outside the served
//! workspace are represented conservatively (basename only,
//! `external = true`) without absolute paths. A definition location is
//! data, never permission: external locations never widen workspace read
//! authority.

use crate::language::limits::LANGUAGE_LIMITS;
use crate::language::position::{LanguageRange, RawRange, to_one_based_range};
use crate::language::sanitize::sanitize_control_characters;

/// One definition location; `external` marks out-of-workspace targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionLocation {
    /// Workspace-relative path (or a conservative basename when external).
    pub path: String,
    /// One-based source range.
    pub range: LanguageRange,
    /// True for out-of-workspace targets represented conservatively.
    pub external: bool,
}

/// A normalized definition query result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionResult {
    /// Workspace-relative path of the query document.
    pub path: String,
    /// Normalized definition locations in input order.
    pub locations: Vec<DefinitionLocation>,
    /// True when the per-query bound was applied.
    pub truncated: bool,
}

/// Limits for one definition query normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefinitionLimits {
    /// Maximum locations retained per query.
    pub max_locations: usize,
}

/// One raw definition entry before normalization: the service URI (as
/// reported by the adapter) and the raw 0-based range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDefinitionEntry {
    /// Service URI (`targetUri` or `uri`); None when absent.
    pub uri: Option<String>,
    /// Raw 0-based range; None when absent or malformed.
    pub range: Option<RawRange>,
}

/// The conservative external basename for an out-of-workspace URI,
/// matching the reference (`uri.split("/")` last non-empty segment, or
/// `"external"`).
pub fn external_basename(uri: &str) -> String {
    match uri.split('/').rfind(|part| !part.is_empty()) {
        Some(part) => sanitize_control_characters(part),
        None => "external".to_owned(),
    }
}

/// Normalize one raw LSP definition response. `map_uri` maps a service
/// URI to a workspace-relative path or None when outside the served
/// workspace; `query_path` is the workspace-relative path of the query
/// document (adapter-computed). Malformed entries are skipped;
/// out-of-workspace targets become conservative external basenames.
pub fn normalize_definition_locations(
    entries: &[RawDefinitionEntry],
    query_path: &str,
    map_uri: impl Fn(&str) -> Option<String>,
    limits: DefinitionLimits,
) -> DefinitionResult {
    let mut locations = Vec::new();
    let mut truncated = false;
    for entry in entries {
        if locations.len() >= limits.max_locations {
            truncated = true;
            break;
        }
        let Some(uri) = entry.uri.as_deref() else {
            continue;
        };
        let Some(range) = entry.range.and_then(to_one_based_range) else {
            continue;
        };
        match map_uri(uri) {
            // Only in-workspace targets map back to workspace-relative
            // paths.
            Some(path) => locations.push(DefinitionLocation {
                path,
                range,
                external: false,
            }),
            // Out-of-workspace and engine-internal URIs are represented
            // conservatively without absolute paths.
            None => locations.push(DefinitionLocation {
                path: external_basename(uri),
                range,
                external: true,
            }),
        }
    }
    DefinitionResult { path: query_path.to_owned(), locations, truncated }
}

impl Default for DefinitionLimits {
    fn default() -> Self {
        Self { max_locations: LANGUAGE_LIMITS.max_definition_locations }
    }
}
