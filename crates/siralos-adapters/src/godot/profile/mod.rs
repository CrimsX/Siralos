//! Godot engine discovery and profiling (R8).

pub mod cache;
pub mod engine_profiler;

pub use cache::{
    ENGINE_PROFILE_CACHE_SCHEMA_VERSION, EngineProfileCacheUnavailable,
    GodotEngineProfileCache,
};
pub use engine_profiler::{
    GodotOverrideSource, GodotProfiledCandidate, GodotProfilerInputs,
    GodotSelectionError, deduplicate_candidates,
};
