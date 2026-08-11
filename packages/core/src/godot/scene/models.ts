import type { WorkspaceRevisionHandle } from "../../workspace/workspace-revision.js";

/**
 * Godot text resource semantic models (Stage 3 milestone 8).
 *
 * All models in this module are DERIVED, READ-ONLY projections of Godot
 * text source (`.tscn`/`.tres`). They are never authoritative source:
 * source files, workspace revisions, and Godot itself remain the truth.
 * Every parsed document binds to the exact workspace revision it was
 * parsed from; a document whose source changed is stale evidence, not
 * current project truth.
 *
 * Inspection never executes project code: no `@tool` scripts, no plugins,
 * no imports, no Godot process. Malformed text produces diagnostics and
 * partial results, never fabricated structure.
 */

/** Parse outcome: complete (no errors), partial (structure plus errors), invalid (no usable structure). */
export type GodotParseStatus = "complete" | "partial" | "invalid";

export type GodotDiagnosticSeverity = "error" | "warning" | "info";

export type GodotDiagnosticCode =
  | "scene.missing_header"
  | "scene.unexpected_header"
  | "scene.malformed_section"
  | "scene.duplicate_resource_id"
  | "scene.missing_resource_id"
  | "scene.unknown_resource_reference"
  | "scene.unresolved_parent"
  | "scene.missing_signal_source"
  | "scene.missing_signal_target"
  | "scene.unbalanced_value"
  | "scene.value_truncated"
  | "scene.document_truncated"
  | "scene.unknown_header_attribute"
  | "scene.unknown_property"
  | "resource.missing_header"
  | "resource.unexpected_header"
  | "resource.malformed_section"
  | "resource.duplicate_resource_id"
  | "resource.missing_resource_id"
  | "resource.unknown_resource_reference"
  | "resource.unknown_property"
  | "resource.unbalanced_value"
  | "resource.value_truncated"
  | "resource.document_truncated";

export interface SourceRange {
  readonly startLine: number;
  readonly startColumn: number;
  readonly endLine: number;
  readonly endColumn: number;
}

/** Structured parse diagnostic; ordinary malformed project data is not an infrastructure failure. */
export interface GodotTextDiagnostic {
  /** Parser category/code, e.g. `scene.unknown_resource_reference`. */
  readonly code: GodotDiagnosticCode;
  readonly severity: GodotDiagnosticSeverity;
  readonly message: string;
  /** One-based line in the source document, when known. */
  readonly line?: number;
  /** One-based column in the source document, when known. */
  readonly column?: number;
  readonly range?: SourceRange;
}

/** Bounded raw text preserved for unknown/opaque value syntax. */
export interface GodotRawValue {
  /** Bounded raw text exactly as scanned (truncated past the raw bound). */
  readonly text: string;
  readonly truncated: boolean;
}

/** Conservatively parsed Godot Variant value (bounded; see variant.ts). */
export type GodotVariantValue =
  | { readonly kind: "null" }
  | { readonly kind: "boolean"; readonly value: boolean }
  | { readonly kind: "integer"; readonly value: number }
  | { readonly kind: "float"; readonly value: number }
  | { readonly kind: "string"; readonly value: string }
  | { readonly kind: "string_name"; readonly value: string }
  | { readonly kind: "node_path"; readonly value: string }
  | { readonly kind: "array"; readonly items: readonly GodotVariantValue[] }
  | {
      readonly kind: "dictionary";
      readonly entries: readonly {
        readonly key: GodotVariantValue;
        readonly value: GodotVariantValue;
      }[];
    }
  | {
      readonly kind: "vector";
      /** Variant type name, e.g. `Vector2`, `Transform2D`. */
      readonly typeName: string;
      readonly components: readonly number[];
    }
  | { readonly kind: "color"; readonly components: readonly number[] }
  | {
      readonly kind: "packed_array";
      /** Variant type name, e.g. `PackedStringArray`. */
      readonly typeName: string;
      readonly items: readonly GodotVariantValue[];
    }
  | { readonly kind: "ext_resource"; readonly id: string }
  | { readonly kind: "sub_resource"; readonly id: string }
  | {
      readonly kind: "resource";
      readonly uid?: string;
      readonly path?: string;
      readonly type?: string;
    }
  | {
      readonly kind: "opaque";
      /** Variant type name when recognizable (`TypeName(...)`), else `unknown`. */
      readonly typeName: string;
      readonly raw: GodotRawValue;
    };

/** One serialized property assignment (`name = value`). */
export interface GodotProperty {
  readonly name: string;
  readonly value: GodotVariantValue;
  /** Bounded raw value text exactly as scanned. */
  readonly rawValue: string;
  /** One-based line of the property assignment. */
  readonly line?: number;
}

/**
 * `ext_resource` declaration. Godot assigns document-local string ids
 * (`1_abcde`); identity must never be inferred from numeric/local ids
 * alone — path and `uid://` identity are preserved when Godot provides
 * them, and neither is invented when absent.
 */
export interface ExternalResourceRef {
  /** Document-local resource id, e.g. `1_abcde`. */
  readonly id: string;
  readonly type?: string;
  /** `res://` path when present in the declaration. */
  readonly path?: string;
  /** `uid://...` identity when present in the declaration. */
  readonly uid?: string;
  /** One-based declaration line. */
  readonly line?: number;
}

/**
 * `sub_resource` declaration. Subresource identity is document-local:
 * `SubResource("1")` in one `.tscn` never refers to `SubResource("1")` in
 * another document.
 */
export interface SubResourceRef {
  /** Document-local resource id, e.g. `RectangleShape2D_1`. */
  readonly id: string;
  readonly type: string;
  readonly properties: readonly GodotProperty[];
  /** One-based declaration line. */
  readonly line?: number;
}

/** Reference to an external resource with a resolved workspace path when safely known. */
export interface ResourceReference {
  readonly resource: ExternalResourceRef;
  /** Workspace-relative path resolved from the `res://` path, when contained. */
  readonly resolvedPath?: string;
}

/** Reference to another PackedScene (inheritance base or node instance). */
export interface SceneReference extends ResourceReference {
  readonly kind: "scene";
}

/** Serialized scene signal connection (existence is serialized fact, never verified runtime behavior). */
export interface GodotSignalConnection {
  readonly signal: string;
  /** Node path of the emitting node. */
  readonly from: string;
  /** Node path of the receiving node. */
  readonly to: string;
  readonly method: string;
  readonly flags?: number;
  readonly binds?: readonly GodotVariantValue[];
  /** One-based declaration line. */
  readonly line?: number;
}

export interface GodotSceneNode {
  readonly name: string;
  /** Engine node type; absent for instanced/inherited nodes whose type is external. */
  readonly type?: string;
  /** Serialized parent path; `"."` for the root node. */
  readonly parentPath?: string;
  /**
   * Serialized `owner` attribute when present. Parent and owner are
   * distinct relationships and are never conflated.
   */
  readonly ownerPath?: string;
  /** PackedScene instance (`instance=ExtResource(...)`); the root instance is the base scene. */
  readonly instance?: SceneReference;
  /** Script attachment (`script=ExtResource(...)`/`SubResource(...)`). */
  readonly script?: ResourceReference;
  /** Serialized group memberships. */
  readonly groups: readonly string[];
  /** Ordinary property assignments. */
  readonly properties: readonly GodotProperty[];
  /** Header attributes that were preserved but not interpreted. */
  readonly rawAttributes: readonly { readonly name: string; readonly rawValue: string }[];
  readonly sourceRange?: SourceRange;
}

/**
 * Read-only semantic model of one `.tscn` document, bound to the exact
 * workspace revision it was parsed from.
 */
export interface GodotSceneModel {
  readonly path: string;
  readonly revision: WorkspaceRevisionHandle | null;
  /** Scene `uid://` identity when declared in the header. */
  readonly uid?: string;
  /** Serialized `format` version when declared. */
  readonly format?: number;
  /** Serialized `load_steps` when declared. */
  readonly loadSteps?: number;
  /** Inherited base scene (root node `instance` reference), distinct from child instances. */
  readonly baseScene?: SceneReference;
  readonly externalResources: readonly ExternalResourceRef[];
  readonly subResources: readonly SubResourceRef[];
  readonly nodes: readonly GodotSceneNode[];
  readonly connections: readonly GodotSignalConnection[];
  /** `[editable path="..."]` declarations (editable-instance metadata). */
  readonly editableInstances: readonly string[];
}

/**
 * Read-only semantic model of one `.tres` (or other Godot text resource)
 * document, bound to the exact workspace revision it was parsed from.
 */
export interface GodotResourceModel {
  readonly path: string;
  readonly revision: WorkspaceRevisionHandle | null;
  readonly type: string;
  /** Resource `uid://` identity when declared in the header. */
  readonly uid?: string;
  /** Serialized `format` version when declared. */
  readonly format?: number;
  /** Serialized `load_steps` when declared. */
  readonly loadSteps?: number;
  /** `script` reference when declared in the `[resource]` section. */
  readonly script?: ResourceReference;
  readonly externalResources: readonly ExternalResourceRef[];
  readonly subResources: readonly SubResourceRef[];
  readonly properties: readonly GodotProperty[];
}

/**
 * Parse result carrying the bounded derived model plus diagnostics. The
 * status is `complete` when no errors were produced, `partial` when a
 * usable structure exists despite errors, and `invalid` when the document
 * cannot be interpreted as the requested kind.
 */
export interface GodotTextDocument<T> {
  readonly path: string;
  /** Exact workspace revision this document was parsed from (host-bound). */
  readonly revision: WorkspaceRevisionHandle | null;
  readonly kind: "scene" | "resource";
  readonly status: GodotParseStatus;
  readonly document: T | null;
  readonly diagnostics: readonly GodotTextDiagnostic[];
  /** True when a bounded parse limit stopped reading further content. */
  readonly truncated: boolean;
}
