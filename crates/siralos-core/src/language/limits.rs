//! Immutable generic language-intelligence limits (Stage 3R R5).
//!
//! Every R5 result collection is explicitly bounded; providers and users
//! cannot raise these ceilings. The values are extracted from the
//! reference host limits (the check-only and LSP document bounds)
//! where real reference semantics exist; the symbol/reference
//! ceilings are the generic R5 contract and domains may impose lower
//! limits later.

/// Immutable generic language-intelligence limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageLimits {
    /// Maximum normalized diagnostics retained per document set.
    pub max_diagnostics_per_set: usize,
    /// Maximum normalized diagnostics retained per run.
    pub max_diagnostics_per_run: usize,
    /// Maximum retained length of one normalized diagnostic message.
    pub max_diagnostic_message_bytes: usize,
    /// Maximum definition locations returned per query.
    pub max_definition_locations: usize,
    /// Maximum symbols returned per document.
    pub max_symbols_per_document: usize,
    /// Maximum reference locations returned per query.
    pub max_references_per_query: usize,
    /// Maximum structural declarations retained per document.
    pub max_structural_declarations: usize,
    /// Maximum structural dependencies retained per document.
    pub max_structural_dependencies: usize,
    /// Default advisory summary byte budget.
    pub max_summary_bytes: usize,
    /// Default number of notable function names in a summary.
    pub default_notable_methods: usize,
}

/// The generic language-intelligence limits (reference-extracted).
pub const LANGUAGE_LIMITS: LanguageLimits = LanguageLimits {
    // Reference: lspMaxDiagnosticsPerDocument.
    max_diagnostics_per_set: 2_000,
    // Reference: maxDiagnosticsPerRun.
    max_diagnostics_per_run: 10_000,
    // Reference: maxDiagnosticMessageBytes.
    max_diagnostic_message_bytes: 8 * 1024,
    // Reference: lspMaxDefinitionLocations.
    max_definition_locations: 100,
    // Generic R5 host ceiling (domains may lower it).
    max_symbols_per_document: 4_096,
    // Generic R5 host ceiling (domains may lower it).
    max_references_per_query: 4_096,
    // Reference: the structural declaration cap.
    max_structural_declarations: 256,
    // Reference: the structural dependency cap.
    max_structural_dependencies: 32,
    // Reference: DEFAULT_SUMMARY_MAX_BYTES.
    max_summary_bytes: 4_096,
    // Reference: DEFAULT_SUMMARY_NOTABLE_METHODS.
    default_notable_methods: 12,
};
