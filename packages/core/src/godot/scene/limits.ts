/**
 * Immutable parse bounds for Godot text resources (Stage 3 milestone 8).
 *
 * Provider input cannot raise these limits. Exceeding a bound never
 * crashes or recurses indefinitely: parsing stops at the bound, records an
 * explicit truncation/partial state, and the caller treats the model as
 * bounded. Large scenes are truncated honestly, never silently complete.
 */
export const GODOT_SCENE_LIMITS = {
  /** Maximum document bytes accepted by the intelligence service read. */
  maxDocumentBytes: 8 * 1024 * 1024,
  /** Maximum source lines scanned (the read limit is the primary bound). */
  maxLines: 200_000,
  /** Maximum section declarations per document. */
  maxSections: 4096,
  /** Maximum node declarations per scene. */
  maxNodes: 2048,
  /** Maximum ext_resource + sub_resource declarations per document. */
  maxResources: 2048,
  /** Maximum signal connections per scene. */
  maxConnections: 2048,
  /** Maximum group memberships per node. */
  maxGroupsPerNode: 64,
  /** Maximum property assignments per document. */
  maxProperties: 8192,
  /** Maximum header attributes interpreted per section. */
  maxHeaderAttributes: 128,
  /** Maximum `[editable]` entries per scene. */
  maxEditableInstances: 256,
  /** Maximum Variant nesting depth (arrays/dictionaries). */
  maxVariantDepth: 16,
  /** Maximum array items retained per array/packed-array value. */
  maxArrayItems: 512,
  /** Maximum dictionary entries retained per dictionary value. */
  maxDictionaryEntries: 512,
  /** Maximum numeric components retained per vector/color value. */
  maxVectorComponents: 16,
  /** Maximum raw text preserved for one value (UTF-16 length bound). */
  maxRawValueLength: 4096,
  /** Maximum continuation lines accumulated for one multiline value. */
  maxValueContinuationLines: 64,
  /** Maximum diagnostics retained per document. */
  maxDiagnostics: 100,
  /** Maximum bounded dependency traversal depth (scene inheritance/instancing chains). */
  maxDependencyDepth: 8,
  /** Maximum files visited in one bounded dependency traversal. */
  maxDependencyFiles: 64,
  /** Maximum relationship-index entries (bounded session-scoped memory). */
  maxIndexEntries: 2048,
} as const;
