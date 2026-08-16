//! Strict bounded-turn collector (Stage 3R R7.1).
//!
//! Mirrors the reference 'collectBoundedModelTurn' used by
//! planner/reviewer-style call sites. Unlike the application collector,
//! a duplicate or empty call id fails the whole turn, limits are
//! caller-supplied ('BoundedModelTurnLimits'), every external message is
//! actor-qualified, and cancellation surfaces as 'Aborted'. The
//! accounting and completion/EOF/cancellation precedence semantics are
//! the shared core 'BoundedTurnState'.

use std::collections::BTreeSet;

use serde_json::Value;
use siralos_core::provider::{
    BoundedTurnState, CancellationToken, ConversationItem, ModelEvent,
    ModelProvider, ModelRequest, ProviderEvent, ProviderTurnLimits,
    ToolDefinition, TurnFailure, validate_external_event,
};

/// Caller-supplied immutable limits for one strict provider turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedModelTurnLimits {
    /// Total assistant text bytes.
    pub max_text_bytes: usize,
    /// Number of text_delta events.
    pub max_text_events: usize,
    /// Number of tool calls.
    pub max_tool_calls: usize,
    /// UTF-8 bytes of one tool name.
    pub max_tool_name_bytes: usize,
    /// UTF-8 bytes of one tool-call correlation id.
    pub max_call_id_bytes: usize,
    /// UTF-8 bytes of one tool-call argument payload.
    pub max_tool_argument_bytes: usize,
    /// Aggregate turn bytes.
    pub max_turn_bytes: usize,
}

impl From<&BoundedModelTurnLimits> for ProviderTurnLimits {
    fn from(limits: &BoundedModelTurnLimits) -> Self {
        Self {
            max_assistant_text_bytes: limits.max_text_bytes,
            max_text_events: limits.max_text_events,
            max_tool_calls_per_turn: limits.max_tool_calls,
            max_call_id_bytes: limits.max_call_id_bytes,
            max_tool_name_bytes: limits.max_tool_name_bytes,
            max_tool_argument_bytes: limits.max_tool_argument_bytes,
            max_turn_bytes: limits.max_turn_bytes,
        }
    }
}

/// One retained strict tool call (always valid by construction).
#[derive(Debug, Clone, PartialEq)]
pub struct BoundedModelToolCall {
    /// Correlation id of the call.
    pub call_id: String,
    /// Name of the requested tool.
    pub tool_name: String,
    /// Detached JSON tool input.
    pub input: Value,
}

/// Outcome of one strict bounded turn.
#[derive(Debug, Clone, PartialEq)]
pub enum BoundedModelTurnOutcome {
    /// The turn completed successfully.
    Turn {
        /// Accumulated assistant text.
        text: String,
        /// Retained tool calls in emission order.
        tool_calls: Vec<BoundedModelToolCall>,
    },
    /// The turn was aborted by cancellation.
    Aborted,
    /// The turn failed with an actor-qualified message.
    Failed {
        /// The exact externally observable message.
        message: String,
    },
}

/// Collect one strict provider turn and race it against cancellation.
///
/// Iterator EOF is not completion, any event after 'Completed' rejects
/// the whole turn, and a provider failure surfaces as an
/// actor-qualified failure unless the token is cancelled. 'seen_call_ids'
/// is an optional attempt-wide correlation set: supplying one rejects a
/// call id reused in a later turn.
pub fn collect_bounded_model_turn<P: ModelProvider>(
    actor: &str,
    provider: &P,
    messages: &[ConversationItem],
    tools: &[ToolDefinition],
    cancellation: &CancellationToken,
    limits: &BoundedModelTurnLimits,
    seen_call_ids: Option<&mut BTreeSet<String>>,
) -> BoundedModelTurnOutcome {
    if cancellation.is_cancelled() {
        return BoundedModelTurnOutcome::Aborted;
    }
    let request = ModelRequest {
        messages: messages.to_vec(),
        tools: tools.to_vec(),
        system: None,
    };
    let mut stream = provider.stream(&request, cancellation);
    let mut state = BoundedTurnState::new(ProviderTurnLimits::from(limits));
    let mut tool_calls: Vec<BoundedModelToolCall> = Vec::new();
    let mut local_seen: BTreeSet<String> = BTreeSet::new();
    let seen_call_ids = seen_call_ids.unwrap_or(&mut local_seen);
    loop {
        if cancellation.is_cancelled() {
            return BoundedModelTurnOutcome::Aborted;
        }
        match stream.next() {
            None => break,
            Some(ProviderEvent::Cancelled { message }) => {
                if cancellation.is_cancelled() {
                    return BoundedModelTurnOutcome::Aborted;
                }
                return BoundedModelTurnOutcome::Failed {
                    message: format!("{actor} provider failed: {message}"),
                };
            }
            Some(ProviderEvent::Failed(message)) => {
                return BoundedModelTurnOutcome::Failed {
                    message: format!("{actor} provider failed: {message}"),
                };
            }
            Some(ProviderEvent::Event(event)) => {
                if state.completion_seen() {
                    return BoundedModelTurnOutcome::Failed {
                        message: format!(
                            "{actor} stream emitted an event after completion."
                        ),
                    };
                }
                if let Err(outcome) = apply_strict_event(
                    event,
                    actor,
                    limits,
                    &mut state,
                    &mut tool_calls,
                    seen_call_ids,
                ) {
                    return outcome;
                }
            }
            Some(ProviderEvent::Raw(raw)) => {
                if state.completion_seen() {
                    return BoundedModelTurnOutcome::Failed {
                        message: format!(
                            "{actor} stream emitted an event after completion."
                        ),
                    };
                }
                match validate_external_event(&raw) {
                    Ok(event) => {
                        if let Err(outcome) = apply_strict_event(
                            event,
                            actor,
                            limits,
                            &mut state,
                            &mut tool_calls,
                            seen_call_ids,
                        ) {
                            return outcome;
                        }
                    }
                    Err(protocol) => {
                        return BoundedModelTurnOutcome::Failed {
                            message: TurnFailure::Protocol(protocol)
                                .strict_message(actor),
                        };
                    }
                }
            }
        }
    }
    if cancellation.is_cancelled() {
        return BoundedModelTurnOutcome::Aborted;
    }
    if !state.completion_seen() {
        return BoundedModelTurnOutcome::Failed {
            message: format!(
                "{actor} stream ended without a completion event."
            ),
        };
    }
    BoundedModelTurnOutcome::Turn {
        text: state.assistant_text().to_owned(),
        tool_calls,
    }
}

/// Apply one validated event with strict-adapter semantics (empty,
/// duplicate, and count checks precede the byte accounting, matching
/// the reference order).
fn apply_strict_event(
    event: ModelEvent,
    actor: &str,
    limits: &BoundedModelTurnLimits,
    state: &mut BoundedTurnState,
    tool_calls: &mut Vec<BoundedModelToolCall>,
    seen_call_ids: &mut BTreeSet<String>,
) -> Result<(), BoundedModelTurnOutcome> {
    match event {
        ModelEvent::Completed => {
            state.complete();
            Ok(())
        }
        ModelEvent::TextDelta { text } => match state.push_text(&text) {
            Ok(()) => Ok(()),
            Err(failure) => Err(BoundedModelTurnOutcome::Failed {
                message: failure.strict_message(actor),
            }),
        },
        ModelEvent::ToolCall { call_id, tool_name, input } => {
            if call_id.is_empty() || tool_name.is_empty() {
                return Err(BoundedModelTurnOutcome::Failed {
                    message: format!(
                        "{actor} emitted a tool call with an empty id or name."
                    ),
                });
            }
            if seen_call_ids.contains(&call_id) {
                return Err(BoundedModelTurnOutcome::Failed {
                    message: format!(
                        "{actor} emitted duplicate tool call id {call_id}."
                    ),
                });
            }
            if tool_calls.len() >= limits.max_tool_calls {
                return Err(BoundedModelTurnOutcome::Failed {
                    message: format!(
                        "{actor} exceeded the per-turn tool-call limit."
                    ),
                });
            }
            let argument_bytes = serde_json::to_string(&input)
                .expect("serde_json::Value is always serializable")
                .len();
            if let Err(failure) = state.push_tool_call(
                call_id.len(),
                tool_name.len(),
                argument_bytes,
            ) {
                return Err(BoundedModelTurnOutcome::Failed {
                    message: failure.strict_message(actor),
                });
            }
            seen_call_ids.insert(call_id.clone());
            tool_calls.push(BoundedModelToolCall {
                call_id,
                tool_name,
                input,
            });
            Ok(())
        }
    }
}
