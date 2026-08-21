//! Godot API knowledge adapters (R8).

pub mod api_dump;
pub mod api_index;
pub mod service;

pub use api_dump::parse_godot_api_dump_with_docs;
pub use api_index::{
    build_godot_api_index, lookup_godot_api_symbol, search_godot_api_index,
};
pub use service::GodotKnowledgeService;
