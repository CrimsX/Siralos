//! Language/file adapters (Stage 3R R5).
//!
//! Generic language-service boundary infrastructure only: URI mapping
//! from service URIs to workspace-relative paths. No Godot process
//! execution and no LSP transport live here.

pub mod uri;
