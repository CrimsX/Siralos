//! Generic Application Tool Loop (Stage 3R R7.2).
//!
//! This module owns the closed generic R7.2 slice: a domain-neutral
//! capability identifier, Host permission evaluation, the immutable
//! ordered Tool Registry, the one generic callable Tool seam, the Tool
//! Round with the one-call/one-result pairing invariant, the
//! approved-visible-surface execution guard, the round budget, the
//! closed Tool-loop event set, and the synchronous pull-based
//! Application loop around the R7.1 bounded provider turn.
//!
//! The TypeScript reference is
//! `packages/core/src/application/application.ts` (with `tool-round.ts`
//! and `application-events.ts`) plus the security/tool modules it
//! composes. Rust keeps the observable behavior but represents the
//! async generator as an explicit pull machine; no async runtime,
//! threads, locks, or shared synchronization are introduced.

pub mod budget;
pub mod capability;
pub mod display_input;
pub mod events;
pub mod permission;
pub mod registry;
pub mod round;
pub mod session;

#[cfg(test)]
mod projection_tests;
#[cfg(test)]
mod tests;

pub use budget::{DEFAULT_MAX_TOOL_ROUNDS, MAX_TOOL_ROUNDS, RoundBudget};
pub use capability::{
    CapabilityId, CapabilityIdError, MAX_CAPABILITY_ID_BYTES,
};
pub use display_input::{
    DisplayInput, MAX_DISPLAY_INPUT_LENGTH, to_display_input,
};
pub use events::ToolLoopEvent;
pub use permission::{
    PermissionDecision, PermissionPolicy, PermissionRule, PolicyRule,
    evaluate_permission,
};
pub use registry::{
    ApprovedToolSurface, RegisteredToolInfo, Tool, ToolRegistry,
    ToolRegistryError,
};
pub use round::{
    CANCELLED_BEFORE_EXECUTION_MESSAGE, ExecutableToolCall, ToolCallExecution,
    ToolCallExecutor, ToolRoundKind, ToolRoundOutcome, ToolRoundRunner,
    ToolRoundStep,
};
pub use session::{PromptStartError, SiralosApplication};
