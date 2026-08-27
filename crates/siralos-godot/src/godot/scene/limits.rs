//! Immutable parse bounds for Godot text resources (R8).
//!
//! Mirrors `packages/core/src/godot/scene/limits.ts`.
//! Provider input cannot raise these limits. Exceeding a bound never
//! crashes: parsing stops at the bound and records truncation.

/// Godot scene text-resource parse limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotSceneLimits {
    /// Maximum document bytes.
    pub max_document_bytes: usize,
    /// Maximum source lines scanned.
    pub max_lines: usize,
    /// Maximum section declarations per document.
    pub max_sections: usize,
    /// Maximum node declarations per scene.
    pub max_nodes: usize,
    /// Maximum ext_resource + sub_resource declarations per document.
    pub max_resources: usize,
    /// Maximum signal connections per scene.
    pub max_connections: usize,
    /// Maximum group memberships per node.
    pub max_groups_per_node: usize,
    /// Maximum property assignments per document.
    pub max_properties: usize,
    /// Maximum header attributes interpreted per section.
    pub max_header_attributes: usize,
    /// Maximum `[editable]` entries per scene.
    pub max_editable_instances: usize,
    /// Maximum Variant nesting depth (arrays/dictionaries).
    pub max_variant_depth: usize,
    /// Maximum array items retained per array/packed-array value.
    pub max_array_items: usize,
    /// Maximum dictionary entries retained per dictionary value.
    pub max_dictionary_entries: usize,
    /// Maximum numeric components retained per vector/color value.
    pub max_vector_components: usize,
    /// Maximum raw text preserved for one value (UTF-16 length bound).
    pub max_raw_value_length: usize,
    /// Maximum continuation lines accumulated for one multiline value.
    pub max_value_continuation_lines: usize,
    /// Maximum diagnostics retained per document.
    pub max_diagnostics: usize,
    /// Maximum bounded dependency traversal depth.
    pub max_dependency_depth: usize,
    /// Maximum files visited in one bounded dependency traversal.
    pub max_dependency_files: usize,
    /// Maximum relationship-index entries (bounded session-scoped memory).
    pub max_index_entries: usize,
}

/// Canonical scene limits matching the TypeScript oracle.
pub const GODOT_SCENE_LIMITS: GodotSceneLimits = GodotSceneLimits {
    max_document_bytes: 8 * 1024 * 1024,
    max_lines: 200_000,
    max_sections: 4096,
    max_nodes: 2048,
    max_resources: 2048,
    max_connections: 2048,
    max_groups_per_node: 64,
    max_properties: 8192,
    max_header_attributes: 128,
    max_editable_instances: 256,
    max_variant_depth: 16,
    max_array_items: 512,
    max_dictionary_entries: 512,
    max_vector_components: 16,
    max_raw_value_length: 4096,
    max_value_continuation_lines: 64,
    max_diagnostics: 100,
    max_dependency_depth: 8,
    max_dependency_files: 64,
    max_index_entries: 2048,
};

#[cfg(test)]
mod tests {
    use super::GODOT_SCENE_LIMITS;

    #[test]
    fn scene_limits_are_sane() {
        assert_eq!(GODOT_SCENE_LIMITS.max_document_bytes, 8 * 1024 * 1024);
        assert_eq!(GODOT_SCENE_LIMITS.max_variant_depth, 16);
        assert_eq!(GODOT_SCENE_LIMITS.max_diagnostics, 100);
    }
}
