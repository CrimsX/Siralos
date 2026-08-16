//! Generic read-only workspace Tool adapters (Stage 3R R7.2).
//!
//! These Tools wrap the existing R4 workspace primitives without
//! rewriting filesystem logic. Each Tool owns input validation, returns
//! the canonical R7.1 `ToolExecutionResult`, and receives only the
//! read-only `CancellationSignal`. No mutation, process, Git, network,
//! or optional-domain Tool exists here, and adapters never decide Host
//! authorization.

pub mod workspace_tools;

pub use workspace_tools::{
    WorkspaceListTool, WorkspaceReadTool, WorkspaceSearchTool,
};
