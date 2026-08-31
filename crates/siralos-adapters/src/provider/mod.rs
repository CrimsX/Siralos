//! Provider adapters (Stage 3R R7.1).
//!
//! This module owns the concrete provider side of the R7.1 contract:
//! the deterministic fake provider (identity 'deterministic-fake',
//! deterministic echo, 16-code-point chunking, and the generic
//! workspace list/read/search scenarios) and the strict bounded-turn
//! collector used by planner/reviewer-style call sites. Both build on
//! the provider-neutral contracts and the shared bounded accounting
//! core in 'siralos-core::provider'.

pub mod credential;
pub mod deterministic_fake;
pub mod registry;
pub mod strict_turn;

#[cfg(test)]
mod tests;

pub use credential::HostCredential;
pub use deterministic_fake::{
    DETERMINISTIC_FAKE_PROVIDER_ID, DeterministicFakeProvider,
};
pub use registry::{HostProvider, ProviderKind, UnknownProvider, provider_kind_from_str};
pub use strict_turn::{
    BoundedModelToolCall, BoundedModelTurnLimits, BoundedModelTurnOutcome,
    collect_bounded_model_turn,
};
