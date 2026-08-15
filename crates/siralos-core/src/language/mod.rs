//! Generic language intelligence (Stage 3R R5).
//!
//! Domain-neutral host semantics for source locations and ranges,
//! bounded sanitized diagnostics with deterministic ordering, symbols,
//! definitions and references as bounded query results, the generic
//! structural-document representation with the deterministic advisory
//! summary formatter, and typed validation result semantics.
//!
//! This module is language-neutral by construction: it contains no
//! language-domain grammar, LSP transport behavior, process
//! execution, or language-server sessions. Language implementations
//! may later provide the same observable semantics through their own
//! parsers, scanners, and services. Positions are
//! 1-based (line and column start at 1), matching the external Siralos
//! convention; 0-based LSP positions are converted explicitly at the
//! adapter boundary, never silently mixed.
//!
//! Authority model: language results are evidence, never authority.
//! A diagnostic, symbol, definition, or reference grants no
//! permission, capability, write authority, or completion authority,
//! and locations outside the workspace never widen read scope.
//! Everything here is read-only and deterministic: no workspace
//! mutation, no process launch, no wall clock, no randomness.

pub mod definition;
pub mod diagnostic;
pub mod limits;
pub mod position;
pub mod reference;
pub mod sanitize;
pub mod structure;
pub mod symbol;
pub mod truncate;
pub mod validation;

#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod tests;

pub use definition::{
    DefinitionLimits, DefinitionLocation, DefinitionResult,
    RawDefinitionEntry, normalize_definition_locations,
};
pub use diagnostic::{
    Diagnostic, DiagnosticSet, DiagnosticSeverity,
    NormalizedDiagnosticPayload, RawDiagnostic, RawDiagnosticCode,
    bind_diagnostic_set, map_lsp_severity, normalize_diagnostic_payload,
    normalize_diagnostic_set,
};
pub use limits::{LANGUAGE_LIMITS, LanguageLimits};
pub use position::{
    LanguagePosition, LanguageRange, Location, PositionError,
    to_one_based_position, to_one_based_range, validate_range,
};
pub use reference::{ReferenceLimits, ReferenceResult, normalize_references};
pub use sanitize::sanitize_control_characters;
pub use structure::{
    AnnotationInfo, ConstantInfo, EnumInfo, FunctionInfo, ParameterInfo,
    PropertyInfo, SignalInfo, StructuralDocument, StructuralIssue,
    StructuralSummary, StructureStatus, SummaryOptions,
    build_structural_summary, counted_label,
};
pub use symbol::{Symbol, SymbolKind, SymbolResult, normalize_symbols};
pub use truncate::{
    truncate_utf8_bytes, utf16_len, utf16_prefix_byte_len, utf16_prefix_lossy,
};
pub use validation::{ValidationResult, ValidationStatus};

/// Schema version of the generic language-intelligence result models.
///
/// Externally meaningful durable boundaries (differential records,
/// future cache identity) may bind to this constant; internal structs
/// are not individually versioned.
pub const LANGUAGE_INTELLIGENCE_SCHEMA_VERSION: u64 = 1;
