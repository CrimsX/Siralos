//! Generic workspace Tool adapters (Stage 3R R7.2, ADR 0038).
//!
//! Read tools wrap the R4 read-only primitives; `write` and `edit`
//! wrap the atomic staging and checkpoint lifecycle primitives. Each
//! Tool owns input validation, returns the canonical R7.1
//! `ToolExecutionResult`, and receives only the read-only
//! `CancellationSignal`. Mutation authorization (per-action approval)
//! is decided by the Host application executor, never here.

pub mod context;
pub mod context_events;
pub mod mutation_tools;
pub mod workspace_tools;

pub use context::{
    ContextExpandTool, ContextInspectTool, ContextSearchTool, ContextToolState,
};
pub use mutation_tools::{EditTool, WriteTool};
pub use workspace_tools::{
    ReadTool, WorkspaceListTool, WorkspaceReadTool, WorkspaceSearchTool,
};
