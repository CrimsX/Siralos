//! Provider-neutral GDScript language-session model (R8).
//!
//! Mirrors `packages/core/src/godot/lsp.ts`.
//! Core must not know TCP sockets, port numbers, mirror paths, or transport framing.
//! Positions are 1-based; the LSP adapter converts to 0-based at its boundary.

/// 1-based position (line and column from 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GdScriptPosition {
    /// Line (1-based).
    pub line: u32,
    /// Column (1-based).
    pub column: u32,
}

/// Source range (two 1-based positions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GdScriptSourceRange {
    /// Start.
    pub start: GdScriptPosition,
    /// End.
    pub end: GdScriptPosition,
}

/// Server capabilities Siralos actually intends to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GdScriptLspCapabilities {
    /// Diagnostics.
    pub diagnostics: bool,
    /// Hover.
    pub hover: bool,
    /// Completion.
    pub completion: bool,
    /// Definition.
    pub definition: bool,
}

/// All capabilities cleared.
pub const EMPTY_GDSCRIPT_LSP_CAPABILITIES: GdScriptLspCapabilities =
    GdScriptLspCapabilities {
        diagnostics: false,
        hover: false,
        completion: false,
        definition: false,
    };

/// Session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GdScriptSessionState {
    /// Starting.
    Starting,
    /// Ready.
    Ready,
    /// Stale.
    Stale,
    /// Closed.
    Closed,
    /// Unavailable.
    Unavailable,
}

/// Bounded session status for CLI/provider rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdScriptSessionStatus {
    /// State.
    pub state: GdScriptSessionState,
    /// Session id, if any.
    pub session_id: Option<String>,
    /// Engine version, if known.
    pub engine_version: Option<String>,
    /// Project name, if known.
    pub project_name: Option<String>,
    /// Started at (epoch ms), if known.
    pub started_at_ms: Option<u64>,
    /// Idle time in ms, if known.
    pub idle_ms: Option<u64>,
    /// Capabilities.
    pub capabilities: GdScriptLspCapabilities,
    /// Open document count.
    pub open_document_count: usize,
    /// Diagnostic count.
    pub diagnostic_count: usize,
    /// Network isolation scope.
    pub network_isolation: GdScriptNetworkIsolation,
}

/// Network isolation scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GdScriptNetworkIsolation {
    /// Loopback only.
    LoopbackOnly,
    /// Unverified.
    Unverified,
    /// Unavailable.
    Unavailable,
}

/// Hover section — markup is data, never rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdScriptHoverSection {
    /// Kind: `plaintext` or `markdown`.
    pub kind: String,
    /// Text.
    pub text: String,
}

/// Hover result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdScriptHoverResult {
    /// Workspace-relative `.gd` path.
    pub path: String,
    /// Range, if known.
    pub range: Option<GdScriptSourceRange>,
    /// Contents.
    pub contents: Vec<GdScriptHoverSection>,
}

/// Completion item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdScriptCompletionItem {
    /// Label.
    pub label: String,
    /// Kind, if known.
    pub kind: Option<String>,
    /// Detail, if known.
    pub detail: Option<String>,
    /// Documentation, if known.
    pub documentation: Option<String>,
    /// Insert text (never applied).
    pub insert_text: Option<String>,
}

/// Completion result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdScriptCompletionResult {
    /// Workspace-relative path.
    pub path: String,
    /// Items.
    pub items: Vec<GdScriptCompletionItem>,
    /// Whether items were truncated.
    pub truncated: bool,
}

/// Definition location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdScriptDefinitionLocation {
    /// Workspace-relative path.
    pub path: String,
    /// Range.
    pub range: GdScriptSourceRange,
    /// True for engine/internal locations.
    pub external: bool,
}

/// Definition result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdScriptDefinitionResult {
    /// Workspace-relative path.
    pub path: String,
    /// Locations.
    pub locations: Vec<GdScriptDefinitionLocation>,
    /// Whether truncated.
    pub truncated: bool,
}

/// Diagnostic result for one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdScriptDiagnosticResult {
    /// Workspace-relative path.
    pub path: String,
    /// Diagnostics.
    pub diagnostics: Vec<super::gdscript::GodotGdScriptDiagnostic>,
    /// Whether truncated.
    pub truncated: bool,
}

/// Document request (one `.gd` file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdScriptDocumentRequest {
    /// Workspace-relative `.gd` path.
    pub path: String,
}

/// Position request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdScriptPositionRequest {
    /// Workspace-relative `.gd` path.
    pub path: String,
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
}

/// Outcome of a single query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GdScriptQueryOutcome<T> {
    /// Ready — the query succeeded.
    Ready {
        /// Result.
        result: T,
    },
    /// Not ready or error.
    NotReady {
        /// Status code (e.g. `session_required`, `unavailable`).
        status: String,
        /// Human-readable message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{EMPTY_GDSCRIPT_LSP_CAPABILITIES, GdScriptNetworkIsolation};

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn empty_capabilities_are_all_false() {
        assert!(!EMPTY_GDSCRIPT_LSP_CAPABILITIES.diagnostics);
        assert!(!EMPTY_GDSCRIPT_LSP_CAPABILITIES.hover);
    }

    #[test]
    fn network_isolation_variants() {
        assert_ne!(
            GdScriptNetworkIsolation::LoopbackOnly,
            GdScriptNetworkIsolation::Unverified
        );
    }
}
