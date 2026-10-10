//! Shared provider constants.
//!
//! The bounded response-body limit and the fixed wire and turn-lifecycle
//! strings that several provider paths share have one home here.

/// Cancellation message for a signal tripped before the provider starts.
pub(crate) const CANCELLED_BEFORE_PROVIDER_START: &str =
    "Host cancelled the turn before provider start";

/// Cancellation message for a signal already tripped before the provider call.
pub(crate) const CANCELLED_BEFORE_HTTP_CALL: &str =
    "Host cancelled before HTTP call";

/// Cancellation message for a signal tripped before the request is sent.
pub(crate) const CANCELLED_BEFORE_HTTP_SEND: &str =
    "Host cancelled before HTTP send";

/// Cancellation message for a signal tripped once the response has arrived.
pub(crate) const CANCELLED_AFTER_HTTP_RESPONSE: &str =
    "Host cancelled after HTTP response";

/// The `anthropic-version` header value every Anthropic-shaped path sends.
pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The replay reason before any provider response has been observed.
///
/// It is the initial `last_replay` value and the one restored by
/// `take_last_replay_availability`; the probe asserts this exact text.
pub(crate) const NO_PROVIDER_RESPONSE_OBSERVED: &str =
    "no provider response observed yet";

/// Maximum provider response body bytes accepted before truncation.
pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
