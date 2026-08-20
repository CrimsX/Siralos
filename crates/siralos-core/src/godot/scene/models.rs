//! Godot text resource semantic models (R8).
//!
//! Mirrors `packages/core/src/godot/scene/models.ts`. Every model is a
//! derived, read-only projection of `.tscn`/` .tres` source, bound to the
//! exact workspace revision it was parsed from. No model is authoritative
//! source; inspection never executes project code.

/// Parse outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotParseStatus {
    /// No errors.
    Complete,
    /// Usable structure plus errors.
    Partial,
    /// No usable structure.
    Invalid,
}

impl GodotParseStatus {
    /// Canonical string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Invalid => "invalid",
        }
    }
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotDiagnosticSeverity {
    /// Error.
    Error,
    /// Warning.
    Warning,
    /// Info.
    Info,
}

impl GodotDiagnosticSeverity {
    /// Canonical string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// Parser diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotDiagnosticCode {
    /// scene.missing_header
    SceneMissingHeader,
    /// scene.unexpected_header
    SceneUnexpectedHeader,
    /// scene.malformed_section
    SceneMalformedSection,
    /// scene.duplicate_resource_id
    SceneDuplicateResourceId,
    /// scene.missing_resource_id
    SceneMissingResourceId,
    /// scene.unknown_resource_reference
    SceneUnknownResourceReference,
    /// scene.unresolved_parent
    SceneUnresolvedParent,
    /// scene.missing_signal_source
    SceneMissingSignalSource,
    /// scene.missing_signal_target
    SceneMissingSignalTarget,
    /// scene.unbalanced_value
    SceneUnbalancedValue,
    /// scene.value_truncated
    SceneValueTruncated,
    /// scene.document_truncated
    SceneDocumentTruncated,
    /// scene.unknown_header_attribute
    SceneUnknownHeaderAttribute,
    /// scene.unknown_property
    SceneUnknownProperty,
    /// resource.missing_header
    ResourceMissingHeader,
    /// resource.unexpected_header
    ResourceUnexpectedHeader,
    /// resource.malformed_section
    ResourceMalformedSection,
    /// resource.duplicate_resource_id
    ResourceDuplicateResourceId,
    /// resource.missing_resource_id
    ResourceMissingResourceId,
    /// resource.unknown_resource_reference
    ResourceUnknownResourceReference,
    /// resource.unknown_property
    ResourceUnknownProperty,
    /// resource.unbalanced_value
    ResourceUnbalancedValue,
    /// resource.value_truncated
    ResourceValueTruncated,
    /// resource.document_truncated
    ResourceDocumentTruncated,
}

impl GodotDiagnosticCode {
    /// Canonical dotted code.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SceneMissingHeader => "scene.missing_header",
            Self::SceneUnexpectedHeader => "scene.unexpected_header",
            Self::SceneMalformedSection => "scene.malformed_section",
            Self::SceneDuplicateResourceId => "scene.duplicate_resource_id",
            Self::SceneMissingResourceId => "scene.missing_resource_id",
            Self::SceneUnknownResourceReference => {
                "scene.unknown_resource_reference"
            }
            Self::SceneUnresolvedParent => "scene.unresolved_parent",
            Self::SceneMissingSignalSource => "scene.missing_signal_source",
            Self::SceneMissingSignalTarget => "scene.missing_signal_target",
            Self::SceneUnbalancedValue => "scene.unbalanced_value",
            Self::SceneValueTruncated => "scene.value_truncated",
            Self::SceneDocumentTruncated => "scene.document_truncated",
            Self::SceneUnknownHeaderAttribute => {
                "scene.unknown_header_attribute"
            }
            Self::SceneUnknownProperty => "scene.unknown_property",
            Self::ResourceMissingHeader => "resource.missing_header",
            Self::ResourceUnexpectedHeader => "resource.unexpected_header",
            Self::ResourceMalformedSection => "resource.malformed_section",
            Self::ResourceDuplicateResourceId => {
                "resource.duplicate_resource_id"
            }
            Self::ResourceMissingResourceId => "resource.missing_resource_id",
            Self::ResourceUnknownResourceReference => {
                "resource.unknown_resource_reference"
            }
            Self::ResourceUnknownProperty => "resource.unknown_property",
            Self::ResourceUnbalancedValue => "resource.unbalanced_value",
            Self::ResourceValueTruncated => "resource.value_truncated",
            Self::ResourceDocumentTruncated => "resource.document_truncated",
        }
    }
}

/// 1-based source range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceRange {
    /// Start line (1-based).
    pub start_line: u32,
    /// Start column (1-based).
    pub start_column: u32,
    /// End line (1-based).
    pub end_line: u32,
    /// End column (1-based).
    pub end_column: u32,
}

/// Structured parse diagnostic; malformed project data is not infrastructure failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotTextDiagnostic {
    /// Parser code.
    pub code: GodotDiagnosticCode,
    /// Severity.
    pub severity: GodotDiagnosticSeverity,
    /// Human message.
    pub message: String,
    /// 1-based line, when known.
    pub line: Option<u32>,
    /// 1-based column, when known.
    pub column: Option<u32>,
    /// Source range, when known.
    pub range: Option<SourceRange>,
}

/// Bounded raw text preserved for unknown/opaque value syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotRawValue {
    /// Bounded raw text exactly as scanned (truncated past bound).
    pub text: String,
    /// True when truncated.
    pub truncated: bool,
}

/// One dictionary entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DictionaryEntry {
    /// Key.
    pub key: Box<GodotVariantValue>,
    /// Value.
    pub value: Box<GodotVariantValue>,
}

/// Conservatively parsed Godot Variant value (bounded).
#[derive(Debug, Clone, PartialEq)]
pub enum GodotVariantValue {
    /// `null`
    Null,
    /// Boolean.
    Boolean(bool),
    /// Integer (safe integer only; otherwise opaque).
    Integer(i64),
    /// Float.
    Float(f64),
    /// Quoted string.
    String(String),
    /// `&"..."` StringName.
    StringName(String),
    /// `NodePath("...")`
    NodePath(String),
    /// Array `[ ... ]`
    Array(Vec<GodotVariantValue>),
    /// Dictionary `{ ... }`
    Dictionary(Vec<DictionaryEntry>),
    /// Vector type e.g. `Vector2(1, 2)`
    Vector {
        /// Type name e.g. `Vector2`.
        type_name: String,
        /// Components.
        components: Vec<f64>,
    },
    /// `Color( r, g, b, a )`
    Color(Vec<f64>),
    /// `PackedStringArray( ... )` etc.
    PackedArray {
        /// Type name.
        type_name: String,
        /// Items.
        items: Vec<GodotVariantValue>,
    },
    /// `ExtResource("id")`
    ExtResource(String),
    /// `SubResource("id")`
    SubResource(String),
    /// `Resource("uid://..."[, "type"])` or `Resource("res://..."[, "type"])`
    Resource {
        /// uid when `uid://...`
        uid: Option<String>,
        /// path when `res://...`
        path: Option<String>,
        /// Optional type.
        type_name: Option<String>,
    },
    /// Unknown syntax preserved as bounded raw.
    Opaque {
        /// Recognizable type name or `unknown`.
        type_name: String,
        /// Bounded raw.
        raw: GodotRawValue,
    },
}

/// One property assignment `name = value`.
#[derive(Debug, Clone, PartialEq)]
pub struct GodotProperty {
    /// Property name.
    pub name: String,
    /// Parsed value.
    pub value: GodotVariantValue,
    /// Bounded raw value text exactly as scanned.
    pub raw_value: String,
    /// 1-based line of the assignment.
    pub line: Option<u32>,
}

/// `ext_resource` declaration (document-local id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalResourceRef {
    /// Document-local id e.g. `1_abcde`.
    pub id: String,
    /// Optional type.
    pub type_name: Option<String>,
    /// `res://` path when present.
    pub path: Option<String>,
    /// `uid://...` when present.
    pub uid: Option<String>,
    /// 1-based declaration line.
    pub line: Option<u32>,
}

/// `sub_resource` declaration (document-local id).
#[derive(Debug, Clone, PartialEq)]
pub struct SubResourceRef {
    /// Document-local id.
    pub id: String,
    /// Type.
    pub type_name: String,
    /// Properties.
    pub properties: Vec<GodotProperty>,
    /// 1-based declaration line.
    pub line: Option<u32>,
}

/// Reference to an external resource with resolved workspace path when safely known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceReference {
    /// External resource.
    pub resource: ExternalResourceRef,
    /// Workspace-relative path resolved from `res://`, when contained.
    pub resolved_path: Option<String>,
}

/// Reference to another PackedScene (inheritance base or node instance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneReference {
    /// External resource.
    pub resource: ExternalResourceRef,
    /// Workspace-relative path resolved from `res://`, when contained.
    pub resolved_path: Option<String>,
}

impl SceneReference {
    /// Kind constant `scene`.
    pub const KIND: &'static str = "scene";
}

/// Serialized scene signal connection.
#[derive(Debug, Clone, PartialEq)]
pub struct GodotSignalConnection {
    /// Signal name.
    pub signal: String,
    /// Node path of the emitting node.
    pub from: String,
    /// Node path of the receiving node.
    pub to: String,
    /// Method name.
    pub method: String,
    /// Optional flags.
    pub flags: Option<u32>,
    /// Optional binds.
    pub binds: Option<Vec<GodotVariantValue>>,
    /// 1-based declaration line.
    pub line: Option<u32>,
}

/// One node in a `.tscn` document.
#[derive(Debug, Clone, PartialEq)]
pub struct GodotSceneNode {
    /// Node name.
    pub name: String,
    /// Engine node type; absent for instanced nodes.
    pub type_name: Option<String>,
    /// Serialized parent path; `"."` for root.
    pub parent_path: Option<String>,
    /// Serialized `owner` attribute when present.
    pub owner_path: Option<String>,
    /// PackedScene instance (`instance=ExtResource(...)`).
    pub instance: Option<SceneReference>,
    /// Script attachment.
    pub script: Option<ResourceReference>,
    /// Group memberships.
    pub groups: Vec<String>,
    /// Ordinary property assignments.
    pub properties: Vec<GodotProperty>,
    /// Header attributes preserved but not interpreted.
    pub raw_attributes: Vec<(String, String)>,
    /// Source range when known.
    pub source_range: Option<SourceRange>,
}

/// Read-only semantic model of one `.tscn` document, bound to workspace revision.
#[derive(Debug, Clone, PartialEq)]
pub struct GodotSceneModel {
    /// Workspace-relative path.
    pub path: String,
    /// Exact workspace revision handle, if known.
    pub revision: Option<String>,
    /// Scene `uid://` when declared in header.
    pub uid: Option<String>,
    /// Serialized `format` version when declared.
    pub format: Option<u32>,
    /// Serialized `load_steps` when declared.
    pub load_steps: Option<u32>,
    /// Inherited base scene (root node `instance`).
    pub base_scene: Option<SceneReference>,
    /// External resources.
    pub external_resources: Vec<ExternalResourceRef>,
    /// Sub-resources.
    pub sub_resources: Vec<SubResourceRef>,
    /// Nodes.
    pub nodes: Vec<GodotSceneNode>,
    /// Signal connections.
    pub connections: Vec<GodotSignalConnection>,
    /// `[editable path="..."]` declarations.
    pub editable_instances: Vec<String>,
}

/// Read-only semantic model of one `.tres` document, bound to workspace revision.
#[derive(Debug, Clone, PartialEq)]
pub struct GodotResourceModel {
    /// Workspace-relative path.
    pub path: String,
    /// Exact workspace revision handle, if known.
    pub revision: Option<String>,
    /// Resource type.
    pub type_name: String,
    /// Resource `uid://` when declared.
    pub uid: Option<String>,
    /// Serialized `format` version when declared.
    pub format: Option<u32>,
    /// Serialized `load_steps` when declared.
    pub load_steps: Option<u32>,
    /// `script` reference when declared in `[resource]` section.
    pub script: Option<ResourceReference>,
    /// External resources.
    pub external_resources: Vec<ExternalResourceRef>,
    /// Sub-resources.
    pub sub_resources: Vec<SubResourceRef>,
    /// Properties.
    pub properties: Vec<GodotProperty>,
}

/// Parse result carrying the bounded derived model plus diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct GodotTextDocument<T> {
    /// Workspace-relative path.
    pub path: String,
    /// Exact workspace revision handle, if known.
    pub revision: Option<String>,
    /// `scene` or `resource`.
    pub kind: GodotTextDocumentKind,
    /// Outcome.
    pub status: GodotParseStatus,
    /// Document when status is not `invalid`.
    pub document: Option<T>,
    /// Diagnostics (bounded by `GODOT_SCENE_LIMITS::max_diagnostics`).
    pub diagnostics: Vec<GodotTextDiagnostic>,
    /// True when a bounded parse limit stopped reading.
    pub truncated: bool,
}

/// Document kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotTextDocumentKind {
    /// `.tscn`
    Scene,
    /// `.tres`
    Resource,
}

impl GodotTextDocumentKind {
    /// Canonical string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::Resource => "resource",
        }
    }
}
