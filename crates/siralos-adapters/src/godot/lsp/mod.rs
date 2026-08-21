//! Bounded LSP client adapters (R8).

pub mod file_uri;
pub mod frame_parser;
pub mod json_rpc;
pub mod normalizers;
pub mod port_allocator;
pub mod service;

pub use file_uri::{
    file_uri_to_path, mirror_uri_to_workspace_relative, path_to_file_uri,
    workspace_relative_to_mirror_uri,
};
pub use frame_parser::{LspFrameOutcome, LspFrameParser, frame_message};
pub use json_rpc::{JsonRpcCode, JsonRpcMessage, classify_json_rpc_payload};
pub use normalizers::{
    LspNormalizationContext, NormalizedPublishDiagnostics,
    normalize_completion, normalize_definition, normalize_hover,
    normalize_publish_diagnostics,
};
pub use port_allocator::{
    AllocatedLspPort, MAX_ALLOCATION_ATTEMPTS, allocate_loopback_port,
    release_loopback_port,
};
pub use service::{
    GODOT_LSP_EXECUTION_UNAVAILABLE_MESSAGE, GodotLspService,
    GodotLspServiceCancelled,
};
