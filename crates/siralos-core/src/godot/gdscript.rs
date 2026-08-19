//! Provider-neutral GDScript diagnostic model (R8).
//!
//! Mirrors `packages/core/src/godot/gdscript.ts`.
//!
//! Bounded, sanitized; unknown line/column values are never fabricated.

/// Source of a GDScript diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GdScriptDiagnosticSource {
    /// From `godot --check-only`.
    CheckOnly,
    /// From LSP.
    Lsp,
}

/// Severity of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GdScriptSeverity {
    /// Error.
    Error,
    /// Warning.
    Warning,
    /// Info.
    Info,
    /// Unknown (engine output carried none).
    Unknown,
}

/// One bounded, sanitized GDScript diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotGdScriptDiagnostic {
    /// Source.
    pub source: GdScriptDiagnosticSource,
    /// Severity.
    pub severity: GdScriptSeverity,
    /// Workspace-relative path; `None` when absent.
    pub path: Option<String>,
    /// 1-based line, if present.
    pub line: Option<u32>,
    /// 1-based column, if present.
    pub column: Option<u32>,
    /// Stable code, if present.
    pub code: Option<String>,
    /// Bounded, sanitized message.
    pub message: String,
    /// Raw category token, if present.
    pub raw_category: Option<String>,
}

/// One script target of a prepared diagnostic check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotScriptCheckTarget {
    /// Workspace-relative `/`-separated path.
    pub path: String,
    /// SHA-256 of the script bytes (64 hex).
    pub sha256: String,
    /// Size in bytes.
    pub bytes: usize,
}

/// Preview shown before the one-time approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiagnosticPreview {
    /// Project name, if known.
    pub project_name: Option<String>,
    /// Engine version string.
    pub engine_version: String,
    /// Installation id.
    pub installation_id: String,
    /// Engine edition string.
    pub engine_edition: String,
    /// Support string.
    pub support: String,
    /// Compatibility string.
    pub compatibility: String,
    /// Script counts + total bytes.
    pub scripts: GodotDiagnosticScripts,
    /// Risk-manifest digest the approval binds to.
    pub manifest_digest: String,
}

/// Script summary inside a diagnostic preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiagnosticScripts {
    /// Script count.
    pub count: usize,
    /// Exact relative paths for single-script checks; `None` for project-wide.
    pub paths: Option<Vec<String>>,
    /// Total bytes.
    pub total_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::{GdScriptDiagnosticSource, GdScriptSeverity};

    #[test]
    fn diagnostic_source_variants() {
        assert_ne!(
            GdScriptDiagnosticSource::CheckOnly,
            GdScriptDiagnosticSource::Lsp
        );
    }

    #[test]
    fn severity_variants() {
        assert_ne!(GdScriptSeverity::Error, GdScriptSeverity::Unknown);
    }
}
