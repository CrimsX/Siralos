//! Generic structural-document representation, normalization, and the
//! deterministic advisory summary formatter (Stage 3R R5, ADR 0016).
//!
//! A structural document carries only language-neutral facts: typed
//! declarations with optional names, opaque signature/detail text,
//! opaque attributes/modifiers, optional one-based lines, and nested
//! children; dependencies as opaque bounded facts; a complete/partial
//! status derived from typed issues; an explicit truncated flag; and
//! the R4 revision binding. The Host never interprets language
//! semantics: attribute strings such as `static` or `export` are data,
//! never meaning. A language implementation turns exact source at a
//! known revision into this bounded observation through its own
//! parser (language-domain extraction is a later milestone's
//! surface). The advisory summary formatter is the deterministic
//! language-neutral renderer (byte-bounded, revision-stating, always
//! advisory; the footer is never truncated away). A summary is
//! advisory and never authoritative source, and structural
//! information grants no read, write, capability, or completion
//! authority.

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

/// Genuinely cross-language declaration categories.
///
/// This small closed vocabulary is the stable generic Host contract;
/// language implementations map their own constructs onto it (for
/// example a signal or event maps to [`StructuralKind::Event`], a
/// property or field to [`StructuralKind::Field`]). The vocabulary
/// never includes language-domain kinds, and no behavior is attached
/// to any kind beyond deterministic rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StructuralKind {
    /// A type/class/interface/struct declaration.
    Type,
    /// A standalone function declaration.
    Function,
    /// A method (function nested inside a type).
    Method,
    /// A field or property declaration.
    Field,
    /// A variable declaration.
    Variable,
    /// A constant declaration.
    Constant,
    /// An enum declaration.
    Enum,
    /// An event/signal declaration.
    Event,
    /// A namespace or module declaration.
    Module,
    /// An opaque or unknown declaration category.
    Other,
}

impl StructuralKind {
    /// The canonical protocol string for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Function => "function",
            Self::Method => "method",
            Self::Field => "field",
            Self::Variable => "variable",
            Self::Constant => "constant",
            Self::Enum => "enum",
            Self::Event => "event",
            Self::Module => "module",
            Self::Other => "other",
        }
    }

    /// Parse a protocol kind string; unknown kinds are rejected so
    /// invalid data never silently becomes a valid declaration.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "type" => Some(Self::Type),
            "function" => Some(Self::Function),
            "method" => Some(Self::Method),
            "field" => Some(Self::Field),
            "variable" => Some(Self::Variable),
            "constant" => Some(Self::Constant),
            "enum" => Some(Self::Enum),
            "event" => Some(Self::Event),
            "module" => Some(Self::Module),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

/// One structural declaration (bounded, ordered, language-neutral).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralDeclaration {
    /// Cross-language declaration kind.
    pub kind: StructuralKind,
    /// Optional display name.
    pub name: Option<String>,
    /// Optional opaque signature/detail text.
    pub detail: Option<String>,
    /// Optional one-based source line.
    pub line: Option<u64>,
    /// Opaque bounded attributes/modifiers (never interpreted by the
    /// Host; e.g. `static`, `export`, `rpc` are data).
    pub attributes: Vec<String>,
    /// Nested declarations in document order (bounded).
    pub children: Vec<StructuralDeclaration>,
}

impl StructuralDeclaration {
    /// Construct a declaration with no children.
    pub fn leaf(
        kind: StructuralKind,
        name: Option<String>,
        detail: Option<String>,
        line: Option<u64>,
        attributes: Vec<String>,
    ) -> Self {
        Self { kind, name, detail, line, attributes, children: Vec::new() }
    }
}

/// One typed structural issue (parser/structure error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralIssue {
    /// Optional one-based source line.
    pub line: Option<u64>,
    /// Bounded issue message.
    pub message: String,
}

/// Bounds for structural normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructureOptions {
    /// Maximum total declarations retained across the whole tree.
    pub max_declarations: usize,
    /// Maximum nesting depth (document children are depth 1).
    pub max_depth: usize,
    /// Maximum dependencies retained.
    pub max_dependencies: usize,
    /// Maximum issues retained.
    pub max_issues: usize,
}

impl Default for StructureOptions {
    fn default() -> Self {
        Self {
            max_declarations: LANGUAGE_LIMITS.max_structural_declarations,
            max_depth: LANGUAGE_LIMITS.max_structural_depth,
            max_dependencies: LANGUAGE_LIMITS.max_structural_dependencies,
            max_issues: LANGUAGE_LIMITS.max_structural_issues,
        }
    }
}

/// A normalized, bounded structural document for one exact source state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralDocument {
    /// Workspace-relative path with `/` separators.
    pub path: String,
    /// R4 revision handle of the exact source state observed.
    pub revision: Option<String>,
    /// Declarations in document order (depth-first, bounded).
    pub declarations: Vec<StructuralDeclaration>,
    /// Bounded opaque dependencies.
    pub dependencies: Vec<String>,
    /// Parse status derived from the bounded issues.
    pub status: StructureStatus,
    /// Typed issues (source order, bounded).
    pub issues: Vec<StructuralIssue>,
    /// True when any output bound was applied (explicit truncation).
    pub truncated: bool,
}

impl StructuralDocument {
    /// Total declaration count across the whole bounded tree.
    pub fn declaration_count(&self) -> usize {
        fn walk(declarations: &[StructuralDeclaration], count: &mut usize) {
            for declaration in declarations {
                *count += 1;
                walk(&declaration.children, count);
            }
        }
        let mut count = 0;
        walk(&self.declarations, &mut count);
        count
    }

    /// Top-level (depth-1) declarations.
    pub fn top_level(&self) -> &[StructuralDeclaration] {
        &self.declarations
    }
}

/// Normalize one structural observation: preserve document order,
/// bound the declaration tree (count and depth), dependencies, and
/// issues with explicit truncation, and derive the status from the
/// bounded issues. Malformed inputs (deep nesting, huge counts) never
/// panic; they are bounded conservatively.
pub fn normalize_structural_document(
    path: &str,
    declarations: Vec<StructuralDeclaration>,
    dependencies: Vec<String>,
    issues: Vec<StructuralIssue>,
    options: &StructureOptions,
) -> StructuralDocument {
    let mut truncated = false;
    let mut budget = options.max_declarations;
    let declarations = bound_declarations(
        declarations,
        options.max_depth,
        &mut budget,
        &mut truncated,
    );
    let mut dependencies = dependencies;
    if dependencies.len() > options.max_dependencies {
        dependencies.truncate(options.max_dependencies);
        truncated = true;
    }
    let mut issues = issues;
    if issues.len() > options.max_issues {
        issues.truncate(options.max_issues);
        truncated = true;
    }
    let status = if issues.is_empty() {
        StructureStatus::Complete
    } else {
        StructureStatus::Partial
    };
    StructuralDocument {
        path: path.to_owned(),
        revision: None,
        declarations,
        dependencies,
        status,
        issues,
        truncated,
    }
}

/// Depth-first bounded walk of the declaration tree in document order.
///
/// Recursion is bounded by `max_depth` (never by the input shape):
/// subtrees deeper than the bound are excluded without recursing into
/// them, and excluded or unprocessed subtrees are dropped through the
/// iterative `drop_subtree` helper so that cleanup never recurses
/// through drop glue.
fn bound_declarations(
    declarations: Vec<StructuralDeclaration>,
    max_depth: usize,
    budget: &mut usize,
    truncated: &mut bool,
) -> Vec<StructuralDeclaration> {
    bound_at_depth(declarations, max_depth, 1, budget, truncated)
}

fn bound_at_depth(
    declarations: Vec<StructuralDeclaration>,
    max_depth: usize,
    depth: usize,
    budget: &mut usize,
    truncated: &mut bool,
) -> Vec<StructuralDeclaration> {
    let mut out = Vec::new();
    let mut pending = declarations.into_iter();
    while let Some(mut declaration) = pending.next() {
        if *budget == 0 {
            // Output bound reached: the unprocessed remainder is
            // dropped iteratively (never through deep drop glue).
            *truncated = true;
            let remaining = pending.collect::<Vec<_>>();
            drop_subtree(remaining);
            break;
        }
        if depth > max_depth {
            // Deeper than the contract bound: excluded explicitly.
            *truncated = true;
            drop_subtree(std::mem::take(&mut declaration.children));
            continue;
        }
        *budget -= 1;
        declaration.children = bound_at_depth(
            std::mem::take(&mut declaration.children),
            max_depth,
            depth + 1,
            budget,
            truncated,
        );
        out.push(declaration);
    }
    out
}

/// Drop a declaration subtree iteratively so cleanup of malformed
/// deeply nested input never recurses through drop glue.
fn drop_subtree(mut nodes: Vec<StructuralDeclaration>) {
    loop {
        let mut next = Vec::new();
        for mut node in nodes.drain(..) {
            next.append(&mut node.children);
        }
        if next.is_empty() {
            break;
        }
        nodes = next;
    }
}

/// Summary formatting options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryOptions {
    /// Hard cap on the summary text (UTF-8 bytes).
    pub max_bytes: Option<usize>,
    /// Number of notable top-level declaration names to list.
    pub notable_declarations: Option<usize>,
}

impl Default for SummaryOptions {
    fn default() -> Self {
        Self {
            max_bytes: Some(LANGUAGE_LIMITS.max_summary_bytes),
            notable_declarations: Some(
                LANGUAGE_LIMITS.default_notable_declarations,
            ),
        }
    }
}

/// Default advisory summary byte budget.
pub const DEFAULT_SUMMARY_MAX_BYTES: usize = 4096;

/// Default number of notable top-level declaration names in a summary.
pub const DEFAULT_SUMMARY_NOTABLE_DECLARATIONS: usize = 12;

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

/// Per-kind declaration counts in the fixed generic kind order.
fn kind_counts(
    declarations: &[StructuralDeclaration],
) -> Vec<(StructuralKind, usize)> {
    const KINDS: [StructuralKind; 10] = [
        StructuralKind::Type,
        StructuralKind::Function,
        StructuralKind::Method,
        StructuralKind::Field,
        StructuralKind::Variable,
        StructuralKind::Constant,
        StructuralKind::Enum,
        StructuralKind::Event,
        StructuralKind::Module,
        StructuralKind::Other,
    ];
    let mut counts = Vec::new();
    for kind in KINDS {
        let count = count_kind(declarations, kind);
        if count > 0 {
            counts.push((kind, count));
        }
    }
    counts
}

fn count_kind(
    declarations: &[StructuralDeclaration],
    kind: StructuralKind,
) -> usize {
    let mut count = 0;
    for declaration in declarations {
        if declaration.kind == kind {
            count += 1;
        }
        count += count_kind(&declaration.children, kind);
    }
    count
}

/// Build the deterministic advisory structural summary. The formatter
/// renders only generic structure: header with revision, declaration
/// totals by kind, notable top-level names, dependencies, partial
/// status with the issue count, and explicit truncation. Attribute
/// strings are never interpreted (no language-domain meaning).
pub fn build_structural_summary(
    document: &StructuralDocument,
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
    let notable = options
        .notable_declarations
        .unwrap_or(DEFAULT_SUMMARY_NOTABLE_DECLARATIONS);
    let mut lines = Vec::new();
    let name =
        document.path.rsplit('/').next().unwrap_or(document.path.as_str());
    let header = match document.revision.as_deref() {
        None => format!("{name} (summary no revision)"),
        Some(revision) => format!("{name} (summary @ {revision})"),
    };
    lines.push(header);
    let total = document.declaration_count();
    if total > 0 {
        let counts = kind_counts(&document.declarations)
            .into_iter()
            .map(|(kind, count)| format!("{}: {count}", kind.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("- declarations: {total} ({counts})"));
    }
    let named_top_level = document
        .top_level()
        .iter()
        .filter_map(|declaration| declaration.name.as_deref())
        .collect::<Vec<_>>();
    if !named_top_level.is_empty() {
        let listed = named_top_level
            .iter()
            .take(notable)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        if named_top_level.len() > notable {
            lines.push(format!(
                "- top-level: {listed}, ... ({} total)",
                named_top_level.len(),
            ));
        } else {
            lines.push(format!("- top-level: {listed}"));
        }
    }
    if !document.dependencies.is_empty() {
        lines.push(format!(
            "- dependencies: {}",
            document.dependencies.join(", "),
        ));
    }
    if document.status == StructureStatus::Partial {
        lines.push(format!(
            "- structural status: partial ({} issue(s))",
            document.issues.len(),
        ));
    }
    if document.truncated {
        lines.push(
            "- structural output truncated (output bound reached)".to_owned(),
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
        revision: document.revision.clone(),
        text: bounded,
        truncated,
        bytes,
    }
}
