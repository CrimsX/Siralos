//! Generic structural-document representation and the deterministic
//! advisory summary formatter (Stage 3R R5, ADR 0016).
//!
//! A structural document carries only language-neutral facts: typed
//! declarations with names, signatures, annotations, and lines;
//! dependencies; a complete/partial status with typed issues; an
//! explicit truncated flag; and the R4 revision binding. The Host
//! never understands any language grammar here: a language
//! implementation turns exact source at a known revision into a
//! bounded structural document through its own parser. The advisory
//! summary formatter is the deterministic language-neutral renderer
//! (byte-bounded, revision-stating, always advisory; the footer is
//! never truncated away). A summary is advisory and never
//! authoritative source.

use crate::language::limits::LANGUAGE_LIMITS;
use crate::language::truncate::{
    utf16_len, utf16_prefix_byte_len, utf16_prefix_lossy,
};

/// Structure parse status: complete (no issues) or partial (issues).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureStatus {
    /// The structure was parsed without issues.
    Complete,
    /// The structure is partial and carries typed issues.
    Partial,
}

impl StructureStatus {
    /// The canonical protocol string for this status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
        }
    }
}

/// One function/signal parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterInfo {
    /// Parameter name.
    pub name: String,
    /// Optional type annotation.
    pub type_name: Option<String>,
}

/// One file-level annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationInfo {
    /// Annotation name.
    pub name: String,
    /// Bounded annotation arguments.
    pub arguments: Vec<String>,
    /// One-based line.
    pub line: u64,
}

/// One signal declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalInfo {
    /// Signal name.
    pub name: String,
    /// Signal parameters.
    pub parameters: Vec<ParameterInfo>,
    /// One-based line.
    pub line: u64,
}

/// One enum declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumInfo {
    /// Enum name (None for anonymous enums).
    pub name: Option<String>,
    /// Bounded member names.
    pub members: Vec<String>,
    /// One-based line.
    pub line: u64,
    /// True when the declaration spans multiple lines.
    pub multiline: bool,
}

/// One constant declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantInfo {
    /// Constant name.
    pub name: String,
    /// Optional type annotation.
    pub type_name: Option<String>,
    /// One-based line.
    pub line: u64,
    /// True when the declaration spans multiple lines.
    pub multiline: bool,
}

/// One property declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyInfo {
    /// Property name.
    pub name: String,
    /// Optional type annotation.
    pub type_name: Option<String>,
    /// Bounded declaration annotations.
    pub annotations: Vec<String>,
    /// One-based line.
    pub line: u64,
    /// True when the declaration spans multiple lines.
    pub multiline: bool,
}

/// One function declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionInfo {
    /// Function name.
    pub name: String,
    /// Bounded parameters.
    pub parameters: Vec<ParameterInfo>,
    /// Optional return type annotation.
    pub return_type: Option<String>,
    /// True for static functions.
    pub is_static: bool,
    /// Bounded declaration annotations.
    pub annotations: Vec<String>,
    /// One-based line.
    pub line: u64,
    /// True when the signature spans multiple lines.
    pub multiline_signature: bool,
}

/// One typed structural issue (parser/structure error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralIssue {
    /// One-based line.
    pub line: u64,
    /// Bounded issue message.
    pub message: String,
}

/// A bounded structural document for one exact source state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralDocument {
    /// Workspace-relative path with `/` separators.
    pub path: String,
    /// R4 revision handle of the exact source state observed.
    pub revision: Option<String>,
    /// Declared base type (reference `extends` target).
    pub base_type: Option<String>,
    /// Declared type name (reference `class_name`).
    pub declared_name: Option<String>,
    /// File-level annotations.
    pub file_annotations: Vec<AnnotationInfo>,
    /// Signal declarations (source order).
    pub signals: Vec<SignalInfo>,
    /// Enum declarations (source order).
    pub enums: Vec<EnumInfo>,
    /// Constant declarations (source order).
    pub constants: Vec<ConstantInfo>,
    /// Property declarations (source order).
    pub properties: Vec<PropertyInfo>,
    /// Function declarations (source order).
    pub functions: Vec<FunctionInfo>,
    /// Bounded structural dependencies.
    pub dependencies: Vec<String>,
    /// Parse status: complete or partial.
    pub status: StructureStatus,
    /// Typed parser/structure issues (source order).
    pub issues: Vec<StructuralIssue>,
    /// True when the declaration cap was reached (output is bounded).
    pub truncated: bool,
}

impl StructuralDocument {
    /// Total declaration count (bounded by the declaration cap).
    pub fn declaration_count(&self) -> usize {
        self.signals.len()
            + self.enums.len()
            + self.constants.len()
            + self.properties.len()
            + self.functions.len()
    }
}

/// Summary formatting options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryOptions {
    /// Hard cap on the summary text (UTF-8 bytes).
    pub max_bytes: Option<usize>,
    /// Number of notable function names to list.
    pub notable_methods: Option<usize>,
}

impl Default for SummaryOptions {
    fn default() -> Self {
        Self {
            max_bytes: Some(LANGUAGE_LIMITS.max_summary_bytes),
            notable_methods: Some(LANGUAGE_LIMITS.default_notable_methods),
        }
    }
}

/// Default advisory summary byte budget.
pub const DEFAULT_SUMMARY_MAX_BYTES: usize = 4096;

/// Default number of notable function names in a summary.
pub const DEFAULT_SUMMARY_NOTABLE_METHODS: usize = 12;

/// The advisory footer; it is never truncated away and is what stops a
/// summary from being mistaken for authoritative source.
pub const SUMMARY_FOOTER: &str = "\nadvisory structural summary \u{2014} not authoritative source; read exact before editing.";

/// The explicit truncation marker.
pub const SUMMARY_TRUNCATION_MARKER: &str = "\n\u{2026} [summary truncated]";

/// A rendered advisory structural summary (bounded, never authoritative).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralSummary {
    /// Workspace-relative path summarized.
    pub path: String,
    /// R4 revision handle the summary describes (None when unknown).
    pub revision: Option<String>,
    /// Bounded advisory text (footer always present).
    pub text: String,
    /// True when the body was byte-truncated to fit the budget.
    pub truncated: bool,
    /// UTF-8 byte length of the returned text.
    pub bytes: usize,
}

/// Render one count entry (`"N label"`, empty when zero).
pub fn counted_label(count: usize, label: &str) -> String {
    if count == 0 { String::new() } else { format!("{count} {label}") }
}

/// Build the deterministic advisory structural summary, byte-identical
/// to the reference `buildWorkspaceSummary` for equivalent inputs.
pub fn build_structural_summary(
    document: &StructuralDocument,
    revision: Option<&str>,
    options: &SummaryOptions,
) -> StructuralSummary {
    // The advisory footer and the truncation marker always fit: the
    // summary of a very small file may carry this constant overhead
    // (documented), and the byte accounting below reports the
    // effective bound.
    let footer_bytes = SUMMARY_FOOTER.len();
    let marker_bytes = SUMMARY_TRUNCATION_MARKER.len();
    let max_bytes = options
        .max_bytes
        .unwrap_or(DEFAULT_SUMMARY_MAX_BYTES)
        .max(footer_bytes + marker_bytes);
    let notable_methods =
        options.notable_methods.unwrap_or(DEFAULT_SUMMARY_NOTABLE_METHODS);
    let mut lines = Vec::new();
    let name =
        document.path.rsplit('/').next().unwrap_or(document.path.as_str());
    let header = match revision {
        None => format!("{name} (summary no revision)"),
        Some(revision) => format!("{name} (summary @ {revision})"),
    };
    lines.push(header);
    if let Some(base_type) = document.base_type.as_deref() {
        lines.push(format!("- extends {base_type}"));
    }
    if let Some(declared_name) = document.declared_name.as_deref() {
        lines.push(format!("- class_name {declared_name}"));
    }
    if !document.file_annotations.is_empty() {
        let annotations = document
            .file_annotations
            .iter()
            .map(|annotation| format!("@{}", annotation.name))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("- annotations: {annotations}"));
    }
    let counts = [
        counted_label(document.signals.len(), "signals"),
        counted_label(document.properties.len(), "properties"),
        counted_label(document.functions.len(), "functions"),
        counted_label(document.constants.len(), "constants"),
        counted_label(document.enums.len(), "enums"),
    ]
    .into_iter()
    .filter(|entry| !entry.is_empty())
    .collect::<Vec<_>>();
    if !counts.is_empty() {
        lines.push(format!("- {}", counts.join(", ")));
    }
    if !document.properties.is_empty() {
        let exported = document
            .properties
            .iter()
            .filter(|property| {
                property.annotations.iter().any(|annotation| {
                    annotation == "export" || annotation.starts_with("export_")
                })
            })
            .map(|property| property.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if !exported.is_empty() {
            lines.push(format!("- exported properties: {exported}"));
        }
    }
    if !document.functions.is_empty() {
        let notable = document
            .functions
            .iter()
            .take(notable_methods)
            .map(|function| {
                if function.is_static {
                    format!("{} (static)", function.name)
                } else {
                    function.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let total = document.functions.len();
        if total > notable_methods {
            lines.push(format!("- functions: {notable}, ... ({total} total)"));
        } else {
            lines.push(format!("- functions: {notable}"));
        }
    }
    if !document.dependencies.is_empty() {
        lines.push(format!(
            "- dependencies: {}",
            document.dependencies.join(", ")
        ));
    }
    if document.status == StructureStatus::Partial {
        lines.push(format!(
            "- structural_status: partial ({} parser error(s))",
            document.issues.len(),
        ));
    }
    if document.truncated {
        lines.push(
            "- structural output truncated (declaration cap reached)"
                .to_owned(),
        );
    }
    // The advisory footer is never dropped: the body is truncated
    // (byte-aware, UTF-16-unit slicing like the reference) to fit the
    // budget together with the footer.
    let body = lines.join("\n");
    let mut truncated = false;
    let bounded = if body.len() + SUMMARY_FOOTER.len() <= max_bytes {
        format!("{body}{SUMMARY_FOOTER}")
    } else {
        let marker = SUMMARY_TRUNCATION_MARKER;
        let mut low = 0usize;
        let mut high = utf16_len(&body);
        while low < high {
            let mid = (low + high).div_ceil(2);
            if utf16_prefix_byte_len(&body, mid)
                + marker.len()
                + SUMMARY_FOOTER.len()
                <= max_bytes
            {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        truncated = true;
        format!(
            "{}{}{}",
            utf16_prefix_lossy(&body, low),
            marker,
            SUMMARY_FOOTER,
        )
    };
    let bytes = bounded.len();
    StructuralSummary {
        path: document.path.clone(),
        revision: revision.map(str::to_owned),
        text: bounded,
        truncated,
        bytes,
    }
}
