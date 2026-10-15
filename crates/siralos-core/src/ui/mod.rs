//! Declarative terminal UI component model (ADR 0039).
//!
//! Components are data, never code: specs declare what to show,
//! the Host owns all rendering. This module is purely semantic —
//! spec types, bounded validation, and the registry-plus-projection
//! seam. Feed resolution lives in `siralos-adapters`, drawing in
//! the cli TUI host. Core works with zero components registered.

pub mod projection;
pub mod spec;

pub use projection::{
    ProjectedSlot, UiProjection, UiRegistry, UiRegistryError, content_for,
    project_components,
};
pub use spec::{
    GRID_COLS, GRID_ROWS, MAX_SPEC_ID_BYTES, MAX_SPEC_PARAM_KEY_BYTES,
    MAX_SPEC_PARAM_VALUE_BYTES, MAX_SPEC_PARAMS, MAX_SPEC_SIZE_HINT,
    MAX_SPEC_TITLE_CHARS, MIN_SPEC_SIZE_HINT, SourcedSpec, SpecSource,
    UiComponentSpec, UiFeed, UiPlacement, UiSpecError, UnvalidatedSpec,
    merge_specs,
};
