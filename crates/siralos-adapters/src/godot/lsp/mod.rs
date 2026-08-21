//! Bounded LSP client adapters (R8).

pub mod file_uri;
pub mod frame_parser;
pub mod json_rpc;

pub use file_uri::{
    file_uri_to_path, mirror_uri_to_workspace_relative, path_to_file_uri,
    workspace_relative_to_mirror_uri,
};
pub use frame_parser::{LspFrameOutcome, LspFrameParser, frame_message};
pub use json_rpc::{JsonRpcCode, JsonRpcMessage, classify_json_rpc_payload};
