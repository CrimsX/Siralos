//! Provider-neutral contract and bounded single-model-turn semantics
//! (Stage 3R R7.1).
//!
//! This module owns the observable provider contract without any provider
//! SDK: the provider-visible conversation model, the validated typed
//! model-event set, the frozen per-turn bounds, the bounded turn-collection
//! state machine, the tool-call proposal boundary (R7.1 emits proposals
//! only — no tool executes here), the external event trust boundary, and
//! the bounded tool-result validation/detachment boundary.
//!
//! Provider output is untrusted data. The typed `ModelEvent` is trusted
//! Host data; anything arriving from outside passes through
//! `validate_external_event` first, so an unknown discriminator or a
//! malformed known variant can never become a successful typed event.
//!
//! The TypeScript reference collectors are
//! `collectProviderTurn` (packages/core/src/application/provider-turn.ts)
//! and `detachBoundedToolResult`/the strict bounded collector
//! (packages/adapters/src/providers/bounded-model-turn.ts). Rust keeps one
//! bounded accounting core (`BoundedTurnState`) and represents the two
//! call-site contracts (application vs strict adapter) with the two
//! collectors; the strict collector itself lives in
//! `siralos-adapters` because it is the adapter call-site contract.
//!
//! Determinism: message order, tool-definition order, event order,
//! tool-call order, and invalid-call numbering are preserved exactly as
//! the reference produces them. No maps with observable iteration order
//! are used where order matters.

pub mod conversation;
pub mod event;
pub mod result;
pub mod turn;

#[cfg(test)]
mod tests;

pub use conversation::{
    AssistantToolCallInput, ConversationItem, TranscriptFailure,
    validate_conversation_items,
};
pub use event::{
    CancellationSignal, CancellationToken, LimitClass, ModelEvent,
    ModelProvider, ModelRequest, ProtocolFailure, ProviderEvent,
    ProviderTurnLimits, ToolCallInput, ToolDefinition, TurnFailure,
    validate_external_event,
};
pub use result::{
    DetachFailure, ToolExecutionResult, detach_bounded_tool_result,
};
pub use turn::{
    BoundedTurnState, TurnOutcome, TurnToolCall, collect_provider_turn,
};
