//! Godot adapters (R8).

pub mod diagnostics;
pub mod discovery;
pub mod knowledge;
pub mod lsp;
pub mod process;
pub mod profile;
pub mod project;
pub mod scene;
pub mod scene_mutation;

pub use scene::GodotSceneIntelligenceService;
pub use scene_mutation::{
    GODOT_MUTATION_APPLY_UNAVAILABLE_MESSAGE, GodotMutationApplyOutcome,
    GodotSceneMutationService,
};
