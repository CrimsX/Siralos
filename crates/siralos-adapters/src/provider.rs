//! Provider adapters (Stage 3R R7.1).
//!
//! This module owns the concrete provider side of the R7.1 contract:
//! the deterministic fake provider (identity 'deterministic-fake',
//! deterministic echo, 16-code-point chunking, and the generic
//! workspace list/read/search scenarios) and the strict bounded-turn
//! collector used by planner/reviewer-style call sites. Both build on
//! the provider-neutral contracts and the shared bounded accounting
//! core in 'siralos-core::provider'.

pub mod anthropic;
pub mod credential;
pub mod deterministic_fake;
pub mod generic;
pub mod openai;
pub mod registry;
pub mod replay;
pub mod sse;
pub mod strict_turn;
pub mod tool_names;

mod constants;
mod pipeline;
mod record;
mod redaction;

#[cfg(test)]
mod probe;
#[cfg(test)]
mod streaming_redactor_tests;
#[cfg(test)]
mod tests;

pub use credential::HostCredential;
pub use deterministic_fake::{
    DETERMINISTIC_FAKE_PROVIDER_ID, DeterministicFakeProvider,
};
pub use redaction::{
    redact_host_display, safe_endpoint_for_credential,
    safe_endpoint_for_output,
};
pub use registry::{
    HostProvider, ProviderKind, UnknownProvider, provider_kind_from_str,
};
pub use replay::RecordedReplayProvider;
pub use strict_turn::{
    BoundedModelToolCall, BoundedModelTurnLimits, BoundedModelTurnOutcome,
    collect_bounded_model_turn,
};

pub(crate) use anthropic::apply_prompt_cache_breakpoints;
pub(crate) use constants::{
    ANTHROPIC_VERSION, CANCELLED_AFTER_HTTP_RESPONSE,
    CANCELLED_BEFORE_HTTP_CALL, CANCELLED_BEFORE_HTTP_SEND,
    CANCELLED_BEFORE_PROVIDER_START, MAX_RESPONSE_BYTES,
    NO_PROVIDER_RESPONSE_OBSERVED,
};
pub(crate) use pipeline::{
    ChatPipelineOutcome, bounded_body_text_raw, build_http_client,
    run_chat_pipeline,
};
pub(crate) use record::{
    ReplayHooks, record_evidence_outcome_with_secret, record_outcome,
    record_outcome_with_secret, record_outcome_with_secret_and_request_digest,
    request_sha256, request_sha256_for_route, response_body_sha256,
};
pub(crate) use redaction::{StreamingSecretRedactor, redact_sensitive};

// Crate-internal seams kept at their historical `crate::provider::` paths
// even though no caller outside their home module reaches them through
// this module root today.
#[allow(unused_imports)]
pub(crate) use record::record_evidence_outcome;
#[allow(unused_imports)]
pub(crate) use redaction::{redact_body_with_secret, redact_provider_event};

// Redaction-suite helpers reach the private redaction and pipeline seams
// through this module root, so the suite's `super::` paths stay stable.
#[cfg(test)]
use pipeline::bounded_body_text_from_bytes;
#[cfg(test)]
use redaction::replace_variant_matches;
