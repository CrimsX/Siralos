//! Bounded single-model-turn state machine and the application
//! collector (Stage 3R R7.1).
//!
//! Core owns one deterministic bounded-turn accounting state machine
//! ('BoundedTurnState') plus the application turn collector. The strict
//! adapter collector (siralos-adapters) shares the same accounting core
//! and represents its own call-site contract. Events are consumed
//! strictly in provider order; once a terminal outcome is chosen, later
//! provider data cannot replace it. A rejected or cancelled turn is
//! never committed as a successful turn.
//!
//! Tool calls are typed proposals only: R7.1 emits Execute/Invalid
//! 'TurnToolCall' records and never executes a tool or creates a
//! tool_result (that is R7.2).

use std::collections::BTreeSet;

use serde_json::Value;

use super::conversation::{ConversationItem, validate_conversation_items};
use super::event::{
    CancellationToken, LimitClass, ModelEvent, ModelProvider, ModelRequest,
    ProviderEvent, ProviderTurnLimits, ToolDefinition, TurnFailure,
    validate_external_event,
};

/// One collected tool-call proposal (R7.1 emits proposals only).
#[derive(Debug, Clone, PartialEq)]
pub enum TurnToolCall {
    /// A valid, executable proposal retained for the future Tool Round.
    Execute {
        /// Correlation id of the call.
        call_id: String,
        /// Name of the requested tool.
        tool_name: String,
        /// Detached JSON tool input.
        input: Value,
    },
    /// A call the application layer marks invalid (empty id/name or a
    /// duplicate call id) with a deterministic synthetic id.
    Invalid {
        /// Synthetic id ('invalid-call-N').
        call_id: String,
        /// The tool name ('<empty>' when the provider emitted an empty
        /// tool name).
        tool_name: String,
        /// The deterministic invalid-call message.
        message: String,
    },
}

/// Outcome of one bounded application provider turn.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnOutcome {
    /// The turn completed successfully.
    Turn {
        /// Committed assistant text (all deltas concatenated).
        assistant_text: String,
        /// Presentation text-delta sequence in provider order.
        text_deltas: Vec<String>,
        /// Tool-call proposals in emission order.
        tool_calls: Vec<TurnToolCall>,
    },
    /// The turn was cancelled by the Host (outranks provider completion
    /// and terminal failures).
    Cancelled,
    /// The turn failed; the typed failure derives the exact external
    /// message.
    Failed {
        /// The typed failure class.
        failure: TurnFailure,
    },
}

impl TurnOutcome {
    /// The exact externally observable failure message, when failed.
    pub fn failure_message(&self) -> Option<String> {
        match self {
            TurnOutcome::Failed { failure } => {
                Some(failure.application_message())
            }
            _ => None,
        }
    }
}

/// One bounded-turn accounting state machine, shared by the application
/// and strict-adapter collectors.
///
/// It tracks only what the contract requires: assistant text and its
/// bytes, the text-event count, the aggregate turn bytes, and the
/// completion state. Per-dimension checks happen in the reference's
/// order and every bound is inclusive.
#[derive(Debug, Clone)]
pub struct BoundedTurnState {
    limits: ProviderTurnLimits,
    assistant_text: String,
    assistant_text_bytes: usize,
    text_events: usize,
    turn_bytes: usize,
    completion_seen: bool,
}

impl BoundedTurnState {
    /// A fresh collector state with the given limits.
    pub fn new(limits: ProviderTurnLimits) -> Self {
        Self {
            limits,
            assistant_text: String::new(),
            assistant_text_bytes: 0,
            text_events: 0,
            turn_bytes: 0,
            completion_seen: false,
        }
    }

    /// The active limits.
    pub fn limits(&self) -> ProviderTurnLimits {
        self.limits
    }

    /// Whether the provider sent 'Completed'.
    pub fn completion_seen(&self) -> bool {
        self.completion_seen
    }

    /// The accumulated assistant text.
    pub fn assistant_text(&self) -> &str {
        &self.assistant_text
    }

    /// The accumulated aggregate turn bytes.
    pub fn turn_bytes(&self) -> usize {
        self.turn_bytes
    }

    /// Record the provider's completion signal.
    pub fn complete(&mut self) {
        self.completion_seen = true;
    }

    /// Account one text delta (event count, then assistant-text bytes,
    /// then aggregate turn bytes — the reference's order) and append it.
    pub fn push_text(&mut self, text: &str) -> Result<(), TurnFailure> {
        self.text_events += 1;
        if self.text_events > self.limits.max_text_events {
            return Err(TurnFailure::LimitExceeded(
                LimitClass::TextEventCount,
            ));
        }
        let bytes = text.len();
        self.assistant_text_bytes += bytes;
        if self.assistant_text_bytes > self.limits.max_assistant_text_bytes {
            return Err(TurnFailure::LimitExceeded(
                LimitClass::AssistantTextBytes,
            ));
        }
        self.turn_bytes += bytes;
        if self.turn_bytes > self.limits.max_turn_bytes {
            return Err(TurnFailure::LimitExceeded(
                LimitClass::AggregateTurnBytes,
            ));
        }
        self.assistant_text.push_str(text);
        Ok(())
    }

    /// Account one tool call (call-id bytes, then tool-name bytes, then
    /// serialized-argument bytes, then aggregate turn bytes — the
    /// reference's order).
    pub fn push_tool_call(
        &mut self,
        call_id_bytes: usize,
        tool_name_bytes: usize,
        argument_bytes: usize,
    ) -> Result<(), TurnFailure> {
        if call_id_bytes > self.limits.max_call_id_bytes {
            return Err(TurnFailure::LimitExceeded(LimitClass::CallIdBytes));
        }
        if tool_name_bytes > self.limits.max_tool_name_bytes {
            return Err(TurnFailure::LimitExceeded(LimitClass::ToolNameBytes));
        }
        if argument_bytes > self.limits.max_tool_argument_bytes {
            return Err(TurnFailure::LimitExceeded(
                LimitClass::ToolArgumentBytes,
            ));
        }
        self.turn_bytes += call_id_bytes + tool_name_bytes + argument_bytes;
        if self.turn_bytes > self.limits.max_turn_bytes {
            return Err(TurnFailure::LimitExceeded(
                LimitClass::AggregateTurnBytes,
            ));
        }
        Ok(())
    }
}

fn apply_event(
    event: ModelEvent,
    state: &mut BoundedTurnState,
    tool_calls: &mut Vec<TurnToolCall>,
    seen_call_ids: &mut BTreeSet<String>,
    invalid_call_index: &mut usize,
    text_deltas: &mut Vec<String>,
) -> Result<(), TurnFailure> {
    match event {
        ModelEvent::Completed => {
            state.complete();
            Ok(())
        }
        ModelEvent::TextDelta { text } => {
            state.push_text(&text)?;
            text_deltas.push(text);
            Ok(())
        }
        ModelEvent::ToolCall { call_id, tool_name, input } => {
            let argument_bytes = serde_json::to_string(&input)
                .expect("serde_json::Value is always serializable")
                .len();
            state.push_tool_call(
                call_id.len(),
                tool_name.len(),
                argument_bytes,
            )?;
            if tool_calls.len() >= state.limits().max_tool_calls_per_turn {
                return Err(TurnFailure::LimitExceeded(
                    LimitClass::ToolCallCount,
                ));
            }
            if call_id.is_empty() || tool_name.is_empty() {
                *invalid_call_index += 1;
                tool_calls.push(TurnToolCall::Invalid {
                    call_id: format!("invalid-call-{invalid_call_index}"),
                    tool_name: if tool_name.is_empty() {
                        "<empty>".to_owned()
                    } else {
                        tool_name
                    },
                    message: "Provider emitted a tool call with an empty call id or tool name."
                        .to_owned(),
                });
            } else if seen_call_ids.contains(&call_id) {
                *invalid_call_index += 1;
                tool_calls.push(TurnToolCall::Invalid {
                    call_id: format!("invalid-call-{invalid_call_index}"),
                    tool_name,
                    message: format!("Duplicate tool call id: {call_id}."),
                });
            } else {
                seen_call_ids.insert(call_id.clone());
                tool_calls.push(TurnToolCall::Execute {
                    call_id,
                    tool_name,
                    input,
                });
            }
            Ok(())
        }
    }
}

/// Collect and validate exactly one application provider turn.
///
/// The transcript is validated before any provider use. Events are then
/// consumed strictly in provider order through the trust boundary
/// ('validate_external_event'), the seven turn dimensions are enforced,
/// and the outcome is committed only when the stream ends after a
/// 'Completed' event without cancellation. Cancellation outranks
/// completion and terminal failures.
pub fn collect_provider_turn<P: ModelProvider>(
    provider: &P,
    history: &[ConversationItem],
    tools: &[ToolDefinition],
    system: Option<String>,
    cancellation: &CancellationToken,
) -> TurnOutcome {
    if let Err(failure) = validate_conversation_items(history) {
        return TurnOutcome::Failed {
            failure: TurnFailure::InvalidTranscript(failure.message()),
        };
    }
    let request = ModelRequest {
        messages: history.to_vec(),
        tools: tools.to_vec(),
        system,
    };
    if cancellation.is_cancelled() {
        return TurnOutcome::Cancelled;
    }
    let mut stream = provider.stream(&request, cancellation);
    let mut state = BoundedTurnState::new(ProviderTurnLimits::default());
    let mut tool_calls: Vec<TurnToolCall> = Vec::new();
    let mut seen_call_ids: BTreeSet<String> = BTreeSet::new();
    let mut invalid_call_index: usize = 0;
    let mut text_deltas: Vec<String> = Vec::new();
    let mut failure: Option<TurnFailure> = None;
    loop {
        if cancellation.is_cancelled() {
            return TurnOutcome::Cancelled;
        }
        match stream.next() {
            None => break,
            Some(ProviderEvent::Cancelled { .. }) => {
                return TurnOutcome::Cancelled;
            }
            Some(ProviderEvent::Failed(message)) => {
                return TurnOutcome::Failed {
                    failure: TurnFailure::ProviderFailed(message),
                };
            }
            Some(ProviderEvent::Event(event)) => {
                if state.completion_seen() {
                    failure = Some(TurnFailure::EventAfterCompletion);
                    break;
                }
                if let Err(limit) = apply_event(
                    event,
                    &mut state,
                    &mut tool_calls,
                    &mut seen_call_ids,
                    &mut invalid_call_index,
                    &mut text_deltas,
                ) {
                    failure = Some(limit);
                    break;
                }
            }
            Some(ProviderEvent::Raw(raw)) => {
                if state.completion_seen() {
                    failure = Some(TurnFailure::EventAfterCompletion);
                    break;
                }
                match validate_external_event(&raw) {
                    Ok(event) => {
                        if let Err(limit) = apply_event(
                            event,
                            &mut state,
                            &mut tool_calls,
                            &mut seen_call_ids,
                            &mut invalid_call_index,
                            &mut text_deltas,
                        ) {
                            failure = Some(limit);
                            break;
                        }
                    }
                    Err(protocol) => {
                        failure = Some(TurnFailure::Protocol(protocol));
                        break;
                    }
                }
            }
        }
    }
    if cancellation.is_cancelled() {
        return TurnOutcome::Cancelled;
    }
    if let Some(failure) = failure {
        return TurnOutcome::Failed { failure };
    }
    if !state.completion_seen() {
        return TurnOutcome::Failed {
            failure: TurnFailure::EofWithoutCompletion,
        };
    }
    TurnOutcome::Turn {
        assistant_text: state.assistant_text().to_owned(),
        text_deltas,
        tool_calls,
    }
}
