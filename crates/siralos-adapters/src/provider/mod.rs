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
pub mod strict_turn;

#[cfg(test)]
mod tests;

pub use credential::HostCredential;
pub use deterministic_fake::{
    DETERMINISTIC_FAKE_PROVIDER_ID, DeterministicFakeProvider,
};
pub use registry::{
    HostProvider, ProviderKind, UnknownProvider, provider_kind_from_str,
};
pub use strict_turn::{
    BoundedModelToolCall, BoundedModelTurnLimits, BoundedModelTurnOutcome,
    collect_bounded_model_turn,
};

/// Maximum provider response body bytes accepted before truncation.
pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Read an HTTP response body bounded at READ time: at most
/// `MAX_RESPONSE_BYTES + 1` bytes are buffered (via `io::Read::take`), so a
/// hostile endpoint cannot exhaust memory through an unbounded body. The
/// returned text is lossily UTF-8, stripped of control characters (newlines
/// and tabs kept), and marked `...[truncated]` when the bound was hit. The
/// `Err` payload is the raw I/O error for the caller to prefix.
pub(crate) fn bounded_body_text(
    response: reqwest::blocking::Response,
) -> Result<String, String> {
    use std::io::Read;
    let mut limited = response.take((MAX_RESPONSE_BYTES + 1) as u64);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).map_err(|err| err.to_string())?;
    let truncated = bytes.len() > MAX_RESPONSE_BYTES;
    bytes.truncate(MAX_RESPONSE_BYTES);
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    text.retain(|c| !c.is_control() || c == '\n' || c == '\t');
    if truncated {
        text.push_str("...[truncated]");
    }
    Ok(text)
}
