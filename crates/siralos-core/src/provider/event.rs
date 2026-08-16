//! Provider-neutral request/event contract and the external trust
//! boundary (Stage 3R R7.1).
//!
//! 'ModelEvent' is the validated, trusted typed event set — exactly
//! three variants, with no Unknown/Malformed/Raw escape hatches. Unknown
//! or malformed external provider data fails closed in
//! 'validate_external_event' before it can become a valid
//! 'ModelEvent': the runtime discriminator is authoritative, an unknown
//! discriminator is never reinterpreted from its field shape, and
//! malformed values of known variants are rejected deterministically.

use std::cell::Cell;

use serde_json::Value;

/// One validated provider-neutral model event (trusted typed Host data).
#[derive(Debug, Clone, PartialEq)]
pub enum ModelEvent {
    /// A text delta for the current turn.
    TextDelta {
        /// The delta text (UTF-8 byte-accounted by the collectors).
        text: String,
    },
    /// A tool-call proposal for the current turn.
    ToolCall {
        /// Correlation id of the proposed call.
        call_id: String,
        /// Name of the requested tool.
        tool_name: String,
        /// Detached JSON tool input.
        input: Value,
    },
    /// The provider's completion signal.
    Completed,
}

/// Frozen default per-turn provider stream bounds (the application
/// collector contract). Five dimensions are UTF-8 byte bounds and two
/// are count bounds; every bound is inclusive (exactly at the limit is
/// accepted, greater than the limit is rejected).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderTurnLimits {
    /// Total assistant text bytes across all deltas of one turn.
    pub max_assistant_text_bytes: usize,
    /// Number of text_delta events in one turn.
    pub max_text_events: usize,
    /// Number of tool_call events in one turn.
    pub max_tool_calls_per_turn: usize,
    /// UTF-8 bytes of one tool-call correlation id.
    pub max_call_id_bytes: usize,
    /// UTF-8 bytes of one tool name.
    pub max_tool_name_bytes: usize,
    /// UTF-8 bytes of one tool-call argument payload.
    pub max_tool_argument_bytes: usize,
    /// Aggregate UTF-8 bytes (text + ids + names + arguments) of one turn.
    pub max_turn_bytes: usize,
}

impl Default for ProviderTurnLimits {
    fn default() -> Self {
        Self {
            max_assistant_text_bytes: 64 * 1024,
            max_text_events: 4096,
            max_tool_calls_per_turn: 32,
            max_call_id_bytes: 256,
            max_tool_name_bytes: 256,
            max_tool_argument_bytes: 128 * 1024,
            max_turn_bytes: 256 * 1024,
        }
    }
}

/// The provider-visible tool definition (contract only; no registry and
/// no execution function in R7.1).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDefinition {
    /// Stable tool name.
    pub name: String,
    /// Provider-visible description.
    pub description: String,
    /// Provider-visible input schema (JSON object).
    pub input_schema: Value,
}

use super::conversation::ConversationItem;

/// A provider request. The Host builds every field; providers only
/// receive. Cancellation is Host control and is never part of this
/// value: it travels through the separate 'CancellationToken' seam.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRequest {
    /// Conversation items in original order.
    pub messages: Vec<ConversationItem>,
    /// Tool definitions in supplied order.
    pub tools: Vec<ToolDefinition>,
    /// Optional Host-selected system context.
    pub system: Option<String>,
}

/// Host-owned cooperative cancellation seam.
///
/// Synchronous single-threaded control only: 'cancel' marks the token
/// and every collector checks 'is_cancelled' between provider pulls
/// and before committing an outcome. The interior 'Cell' lets the
/// provider stream and the collector share one immutable handle while
/// the host (or a deterministic scripted cancellation point) marks it.
#[derive(Debug, Default)]
pub struct CancellationToken {
    cancelled: Cell<bool>,
}

impl CancellationToken {
    /// A fresh, not-cancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the token cancelled (idempotent).
    pub fn cancel(&self) {
        self.cancelled.set(true);
    }

    /// Whether the token is cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.get()
    }
}

/// Typed provider-event protocol violations. These are the validation
/// failures an untrusted external event can produce before typed
/// acceptance; a turn that encounters one fails closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolFailure {
    /// The discriminator is not a recognized event type.
    UnknownEventType,
    /// The event is not an object (null, array, or raw value).
    MalformedEvent,
    /// A text event without a string payload.
    MalformedTextEvent,
    /// A tool call with a non-string id or name.
    MalformedToolCall,
    /// A tool call without a JSON-serializable input.
    InvalidToolArgumentJson,
}

/// Typed turn-level limit classes (byte and count dimensions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitClass {
    /// Cumulative assistant text bytes.
    AssistantTextBytes,
    /// text_delta event count.
    TextEventCount,
    /// Tool-call count per turn.
    ToolCallCount,
    /// Per-call correlation id bytes.
    CallIdBytes,
    /// Per-call tool-name bytes.
    ToolNameBytes,
    /// Per-call serialized argument bytes.
    ToolArgumentBytes,
    /// Aggregate turn bytes.
    AggregateTurnBytes,
}

/// Typed provider-turn failure. The variant alone identifies the exact
/// failure class; the external message is derived by the deterministic
/// formatters below so machine branching never depends on prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnFailure {
    /// One of the seven bounded turn dimensions was exceeded.
    LimitExceeded(LimitClass),
    /// Any event arrived after 'Completed'.
    EventAfterCompletion,
    /// The provider stream ended without 'Completed'.
    EofWithoutCompletion,
    /// An external event failed the trust-boundary validation.
    Protocol(ProtocolFailure),
    /// The provider itself failed (non-cancellation); the message is the
    /// reference's 'describeError' text.
    ProviderFailed(String),
    /// The transcript is structurally invalid before provider use.
    InvalidTranscript(String),
}

impl TurnFailure {
    /// The fragment the application collector embeds in its limit
    /// messages.
    fn limit_fragment(class: LimitClass) -> &'static str {
        match class {
            LimitClass::AssistantTextBytes => "the assistant-text byte limit",
            LimitClass::TextEventCount => "the text-event count",
            LimitClass::ToolCallCount => "the tool-call count",
            LimitClass::CallIdBytes => "the tool-call id byte limit",
            LimitClass::ToolNameBytes => "the tool-name byte limit",
            LimitClass::ToolArgumentBytes => "the tool-argument byte limit",
            LimitClass::AggregateTurnBytes => "the aggregate turn byte limit",
        }
    }

    /// The exact externally observable application-collector message.
    pub fn application_message(&self) -> String {
        match self {
            TurnFailure::LimitExceeded(class) => format!(
                "The provider exceeded {} limit; the response was rejected.",
                Self::limit_fragment(*class)
            ),
            TurnFailure::EventAfterCompletion => {
                "The provider exceeded an event after completion limit; the response was rejected."
                    .to_owned()
            }
            TurnFailure::EofWithoutCompletion => {
                "The provider stream ended without a completion event; the response was rejected."
                    .to_owned()
            }
            TurnFailure::Protocol(ProtocolFailure::UnknownEventType) => {
                "The provider emitted an unknown event type; the response was rejected."
                    .to_owned()
            }
            TurnFailure::Protocol(ProtocolFailure::MalformedEvent) => {
                "The provider emitted a malformed event; the response was rejected."
                    .to_owned()
            }
            TurnFailure::Protocol(ProtocolFailure::MalformedTextEvent) => {
                "The provider emitted a text event without a string payload; the response was rejected."
                    .to_owned()
            }
            TurnFailure::Protocol(ProtocolFailure::MalformedToolCall) => {
                "The provider emitted a tool call with a non-string id or name; the response was rejected."
                    .to_owned()
            }
            TurnFailure::Protocol(ProtocolFailure::InvalidToolArgumentJson) => {
                "The provider exceeded the tool-argument JSON validity limit; the response was rejected."
                    .to_owned()
            }
            TurnFailure::ProviderFailed(message) => message.clone(),
            TurnFailure::InvalidTranscript(detail) => {
                format!(
                    "The conversation transcript is structurally invalid; the provider request was blocked: {detail}"
                )
            }
        }
    }

    /// The exact externally observable strict-adapter message.
    pub fn strict_message(&self, actor: &str) -> String {
        match self {
            TurnFailure::LimitExceeded(LimitClass::AssistantTextBytes) => {
                format!("{actor} output exceeded its byte limit.")
            }
            TurnFailure::LimitExceeded(LimitClass::TextEventCount) => {
                format!("{actor} exceeded the text-event count limit.")
            }
            TurnFailure::LimitExceeded(LimitClass::ToolCallCount) => {
                format!("{actor} exceeded the per-turn tool-call limit.")
            }
            TurnFailure::LimitExceeded(LimitClass::CallIdBytes) => {
                format!("{actor} exceeded the tool-call id byte limit.")
            }
            TurnFailure::LimitExceeded(LimitClass::ToolNameBytes) => {
                format!("{actor} exceeded the tool-name byte limit.")
            }
            TurnFailure::LimitExceeded(LimitClass::ToolArgumentBytes) => {
                format!("{actor} exceeded the tool-argument byte limit.")
            }
            TurnFailure::LimitExceeded(LimitClass::AggregateTurnBytes) => {
                format!("{actor} exceeded the aggregate turn byte limit.")
            }
            TurnFailure::EventAfterCompletion => {
                format!("{actor} stream emitted an event after completion.")
            }
            TurnFailure::EofWithoutCompletion => {
                format!("{actor} stream ended without a completion event.")
            }
            TurnFailure::Protocol(ProtocolFailure::UnknownEventType) => {
                format!("{actor} emitted an unknown event type.")
            }
            TurnFailure::Protocol(ProtocolFailure::MalformedEvent) => {
                format!("{actor} emitted a malformed event.")
            }
            TurnFailure::Protocol(ProtocolFailure::MalformedTextEvent) => {
                format!(
                    "{actor} emitted a text event without a string payload."
                )
            }
            TurnFailure::Protocol(ProtocolFailure::MalformedToolCall) => {
                format!(
                    "{actor} emitted a tool call with a non-string id or name."
                )
            }
            TurnFailure::Protocol(
                ProtocolFailure::InvalidToolArgumentJson,
            ) => {
                format!(
                    "{actor} emitted a tool argument that is not JSON-serializable."
                )
            }
            TurnFailure::ProviderFailed(message) => {
                format!("{actor} provider failed: {message}")
            }
            // The strict adapter never validates transcripts; the formatter
            // stays total with the application wording for completeness.
            TurnFailure::InvalidTranscript(detail) => {
                format!(
                    "The conversation transcript is structurally invalid; the provider request was blocked: {detail}"
                )
            }
        }
    }
}

/// One pulled item from a provider stream.
///
/// The seam carries validated typed events, untrusted raw external data
/// (validated by the collector through 'validate_external_event' so
/// the post-completion precedence holds), and the two provider failure
/// forms the reference distinguishes (AbortError-equivalent vs a plain
/// provider error).
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    /// A trusted typed event (host-constructed provider path).
    Event(ModelEvent),
    /// Untrusted external data awaiting the trust-boundary validation.
    Raw(Value),
    /// An AbortError-equivalent provider failure.
    Cancelled {
        /// The provider's abort message.
        message: String,
    },
    /// A non-cancellation provider failure.
    Failed(String),
}

/// The minimal provider seam: identity, tool-calling capability, and one
/// pull-based event iterator per request. No provider SDK types, no
/// factories, no registries, no managers.
pub trait ModelProvider {
    /// The concrete iterator type for one request stream.
    type Stream<'a>: Iterator<Item = ProviderEvent> + 'a
    where
        Self: 'a;

    /// Stable provider identity.
    fn id(&self) -> &str;

    /// Whether this route supports tool calling (absent means supported).
    fn tool_calling(&self) -> bool {
        true
    }

    /// Begin one bounded turn stream for the Host-selected request.
    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
        cancellation: &'a CancellationToken,
    ) -> Self::Stream<'a>;
}

/// Validate untrusted external provider event data before it becomes a
/// typed 'ModelEvent'.
///
/// The discriminator is authoritative: an unknown discriminator fails
/// closed even when it carries valid-looking callId/toolName/input
/// fields, and malformed values of known variants are rejected without
/// coercion. Extra fields on 'completed' are irrelevant.
pub fn validate_external_event(
    raw: &Value,
) -> Result<ModelEvent, ProtocolFailure> {
    let Some(object) = raw.as_object() else {
        return Err(ProtocolFailure::MalformedEvent);
    };
    match object.get("type") {
        Some(Value::String(kind)) => match kind.as_str() {
            "completed" => Ok(ModelEvent::Completed),
            "text_delta" => match object.get("text") {
                Some(Value::String(text)) => {
                    Ok(ModelEvent::TextDelta { text: text.clone() })
                }
                _ => Err(ProtocolFailure::MalformedTextEvent),
            },
            "tool_call" => {
                let call_id = match object.get("callId") {
                    Some(Value::String(value)) => value,
                    _ => return Err(ProtocolFailure::MalformedToolCall),
                };
                let tool_name = match object.get("toolName") {
                    Some(Value::String(value)) => value,
                    _ => return Err(ProtocolFailure::MalformedToolCall),
                };
                let input = match object.get("input") {
                    Some(value) => value.clone(),
                    None => {
                        return Err(ProtocolFailure::InvalidToolArgumentJson);
                    }
                };
                Ok(ModelEvent::ToolCall {
                    call_id: call_id.clone(),
                    tool_name: tool_name.clone(),
                    input,
                })
            }
            _ => Err(ProtocolFailure::UnknownEventType),
        },
        _ => Err(ProtocolFailure::UnknownEventType),
    }
}
