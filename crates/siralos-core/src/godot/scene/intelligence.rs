//! Godot scene/resource intelligence port (R8).
//! Mirrors `packages/core/src/godot/scene/intelligence.ts`.

use super::models::{
    GodotParseStatus, GodotResourceModel, GodotSceneModel,
    GodotTextDiagnostic, GodotTextDocument,
};
use super::relationship_index::{
    GodotRelationshipEntry, GodotRelationshipKind,
};
use super::tree::GodotSceneNodeTree;

/// Status of an intelligence inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotIntelligenceStatus {
    /// Ok.
    Ok,
    /// Not found.
    NotFound,
    /// Unreadable.
    Unreadable,
    /// Unsupported.
    Unsupported,
    /// Denied.
    Denied,
    /// Failed.
    Failed,
}
impl GodotIntelligenceStatus {
    /// Canonical string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NotFound => "not_found",
            Self::Unreadable => "unreadable",
            Self::Unsupported => "unsupported",
            Self::Denied => "denied",
            Self::Failed => "failed",
        }
    }
}

/// Outcome of inspecting one document.
#[derive(Debug, Clone)]
pub struct GodotInspectionOutcome<T> {
    /// Status.
    pub status: GodotIntelligenceStatus,
    /// Human message when not Ok.
    pub message: Option<String>,
    /// Workspace-relative path.
    pub path: String,
    /// Exact workspace revision handle.
    pub revision: Option<String>,
    /// Parsed document when Ok.
    pub document: Option<GodotTextDocument<T>>,
}

/// Result of inspecting one `.tscn`.
#[derive(Debug, Clone)]
pub struct GodotSceneInspectionResult {
    /// Base outcome.
    pub outcome: GodotInspectionOutcome<GodotSceneModel>,
    /// Derived tree when document has usable structure.
    pub tree: Option<GodotSceneNodeTree>,
}

/// Result of inspecting one `.tres`.
#[derive(Debug, Clone)]
pub struct GodotResourceInspectionResult(
    pub GodotInspectionOutcome<GodotResourceModel>,
);

/// One bounded dependency edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDependencyEdge {
    /// Kind.
    pub kind: GodotRelationshipKind,
    /// Workspace-relative source path.
    pub source_path: String,
    /// Workspace-relative target path.
    pub target_path: String,
    /// Target `uid://` when known.
    pub target_uid: Option<String>,
    /// Depth from query root (0 = immediate).
    pub depth: u32,
}

/// Bounded dependency traversal result.
#[derive(Debug, Clone)]
pub struct GodotDependencyResult {
    /// Status.
    pub status: GodotIntelligenceStatus,
    /// Message when not Ok.
    pub message: Option<String>,
    /// Root path.
    pub root_path: String,
    /// Revision of the root.
    pub revision: Option<String>,
    /// Bounded edges.
    pub edges: Vec<GodotDependencyEdge>,
    /// Referrers from the relationship index.
    pub referrers: Vec<GodotRelationshipEntry>,
    /// Files visited.
    pub files_visited: usize,
    /// True when depth bound truncated.
    pub truncated_depth: bool,
    /// True when file-count bound truncated.
    pub truncated_files: bool,
    /// True when a cycle was detected.
    pub cycle_detected: bool,
    /// Cycle path when detected.
    pub cycle_path: Option<Vec<String>>,
}

/// Structured project relationships.
#[derive(Debug, Clone)]
pub struct GodotProjectRelationshipResult {
    /// Status `ok` or `no_project`.
    pub status: String,
    /// Message when not Ok.
    pub message: Option<String>,
    /// Main scene reference.
    pub main_scene: Option<GodotMainSceneReference>,
    /// Autoloads.
    pub autoloads: Vec<GodotAutoload>,
    /// Input actions.
    pub input_actions: Vec<GodotInputAction>,
    /// Diagnostics from the project scan.
    pub diagnostics: Vec<GodotTextDiagnostic>,
}

/// Main scene reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotMainSceneReference {
    /// Workspace-relative main scene path.
    pub path: String,
    /// Revision handle of the main scene at resolution time.
    pub revision: Option<String>,
    /// Whether the main scene file exists.
    pub exists: bool,
}

/// Structured autoload entry (never executed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotAutoload {
    /// Name.
    pub name: String,
    /// Workspace-relative target path.
    pub path: String,
    /// Serialized singleton state.
    pub enabled: bool,
    /// Target kind.
    pub target_kind: String,
    /// Original bounded target text.
    pub target: String,
}

/// Bounded input-action structural information.
#[derive(Debug, Clone, PartialEq)]
pub struct GodotInputAction {
    /// Name.
    pub name: String,
    /// Deadzone, if present.
    pub deadzone: Option<f64>,
    /// Event count.
    pub event_count: usize,
    /// Event types.
    pub event_types: Vec<String>,
}

/// Bounded scene/resource inspection observation for context projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotSceneEvidenceView {
    /// Workspace-relative path.
    pub path: String,
    /// Exact workspace revision handle.
    pub revision: Option<String>,
    /// Kind.
    pub kind: String,
    /// Status.
    pub status: GodotParseStatus,
    /// Bounded single-line structural summary.
    pub summary: String,
    /// Evidence id, if any.
    pub evidence_id: Option<String>,
}

/// Support marker — static parsing is always available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotSceneIntelligenceSupport {
    /// Always `ready` — offline capability.
    pub state: String,
}
impl Default for GodotSceneIntelligenceSupport {
    fn default() -> Self {
        Self { state: "ready".to_owned() }
    }
}
