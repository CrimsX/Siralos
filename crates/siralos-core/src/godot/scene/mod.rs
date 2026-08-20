//! Read-only Godot text-resource intelligence (R8).
//!
//! Mirrors `packages/core/src/godot/scene/`.
//! Pure, bounded, static parsing. No filesystem or process.

pub mod intelligence;
pub mod limits;
pub mod models;
pub mod relationship_index;
pub mod resolution;
pub mod text;
pub mod tree;
pub mod variant;

pub use intelligence::{
    GodotAutoload, GodotDependencyEdge, GodotDependencyResult,
    GodotInputAction, GodotInspectionOutcome, GodotIntelligenceStatus,
    GodotMainSceneReference, GodotProjectRelationshipResult,
    GodotResourceInspectionResult, GodotSceneEvidenceView,
    GodotSceneInspectionResult, GodotSceneIntelligenceSupport,
};
pub use limits::{GODOT_SCENE_LIMITS, GodotSceneLimits};
pub use models::{
    DictionaryEntry, ExternalResourceRef, GodotParseStatus, GodotProperty,
    GodotRawValue, GodotResourceModel, GodotSceneModel, GodotSceneNode,
    GodotSignalConnection, GodotTextDiagnostic, GodotTextDocument,
    GodotTextDocumentKind, GodotVariantValue, ResourceReference,
    SceneReference, SourceRange, SubResourceRef,
};
pub use relationship_index::{
    GodotRelationshipEntry, GodotRelationshipIndex, GodotRelationshipKind,
};
pub use resolution::{ResPathResolution, is_godot_uid, resolve_res_path};
pub use text::{
    BalancedScan, HeaderAttribute, is_balanced_text, is_comment_line,
    parse_header_attributes, scan_balanced, split_key_value,
};
pub use tree::{
    GodotSceneNodeTree, GodotSceneTreeNode, build_scene_node_tree,
    nodes_in_group,
};
pub use variant::{
    VariantParseResult, parse_godot_variant, parse_quoted_string,
    split_top_level_arguments,
};
