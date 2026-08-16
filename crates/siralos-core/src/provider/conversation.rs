//! Provider-visible generic conversation model (Stage 3R R7.1, extended
//! for the R7.2 Tool Round).
//!
//! Only the provider-visible items are ported: user messages, assistant
//! messages, assistant tool calls, and tool results. Ordering is
//! preserved exactly as supplied; no application Tool Loop is implemented
//! here (R7.2). The transcript model exists because one model turn
//! validates existing history before sending a request and because the
//! Tool Round records the authoritative paired transcript.
//!
//! Invalid retained tool calls are recorded with the input field omitted,
//! matching the TypeScript reference (`input: undefined` is absent from
//! serialized records). `AssistantToolCallInput` makes that presence
//! explicit instead of using a sentinel value.

use serde_json::Value;

use super::result::ToolExecutionResult;

/// Input presence for a recorded assistant tool call.
///
/// R7.1 always carried a detached JSON value. The R7.2 Tool Round also
/// records invalid retained calls exactly like the reference does: the
/// call is present in the transcript but its `input` field is omitted.
#[derive(Debug, Clone, PartialEq)]
pub enum AssistantToolCallInput {
    /// A detached JSON tool input is present.
    Present(Value),
    /// The reference serialized this call without an input field.
    Omitted,
}

impl AssistantToolCallInput {
    /// The retained input value, when present.
    pub fn value(&self) -> Option<&Value> {
        match self {
            Self::Present(value) => Some(value),
            Self::Omitted => None,
        }
    }
}

/// One provider-visible conversation item.
#[derive(Debug, Clone, PartialEq)]
pub enum ConversationItem {
    /// A user prompt.
    UserMessage {
        /// The message content.
        content: String,
    },
    /// A committed assistant message.
    AssistantMessage {
        /// The message content.
        content: String,
    },
    /// An assistant tool-call proposal recorded in history.
    AssistantToolCall {
        /// Correlation id of the call.
        call_id: String,
        /// Name of the requested tool.
        tool_name: String,
        /// Detached JSON tool input, or an explicit omitted-input marker.
        input: AssistantToolCallInput,
    },
    /// The result paired with one recorded assistant tool call.
    ToolResult {
        /// Correlation id of the call this result belongs to.
        call_id: String,
        /// Name of the executed tool.
        tool_name: String,
        /// The typed execution result value.
        result: ToolExecutionResult,
    },
}

/// A typed transcript-pairing violation (deterministic first failure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptFailure {
    /// An unresolved tool call crosses a later user message.
    UnresolvedBeforeUser {
        /// Call id of the first unresolved call.
        call_id: String,
        /// Tool name of the first unresolved call.
        tool_name: String,
    },
    /// A recorded tool call has an empty call id.
    EmptyCallId,
    /// A call id appears on more than one recorded tool call.
    DuplicateCallId {
        /// The duplicated call id.
        call_id: String,
    },
    /// A tool result has no recorded call (including a second result for
    /// an already-resolved call, which the reference reports this way
    /// because resolved calls are removed from its pending set).
    OrphanResult {
        /// The result's call id.
        call_id: String,
    },
    /// An unresolved tool call remains at the end of the transcript.
    UnresolvedAtEnd {
        /// Call id of the first unresolved call.
        call_id: String,
        /// Tool name of the first unresolved call.
        tool_name: String,
    },
}

impl TranscriptFailure {
    /// The exact externally observable description (reference wording).
    pub fn message(&self) -> String {
        match self {
            TranscriptFailure::UnresolvedBeforeUser { call_id, tool_name } => {
                format!(
                    "Tool call {call_id} ({tool_name}) has no result before the next user message."
                )
            }
            TranscriptFailure::EmptyCallId => {
                "A tool call has an empty call id.".to_owned()
            }
            TranscriptFailure::DuplicateCallId { call_id } => {
                format!("Tool call id {call_id} appears more than once.")
            }
            TranscriptFailure::OrphanResult { call_id } => {
                format!("Tool result for {call_id} has no recorded call.")
            }
            TranscriptFailure::UnresolvedAtEnd { call_id, tool_name } => {
                format!("Tool call {call_id} ({tool_name}) has no result.")
            }
        }
    }
}

/// Validate transcript pairing invariants and return the deterministic
/// first failure.
///
/// Invariants: every `AssistantToolCall` has exactly one
/// `ToolResult` with the same call id before the next user message; no
/// result exists without a recorded call; no call id appears twice; no
/// empty call id exists; no unresolved call crosses a later user message
/// or remains at the end of the transcript. The reference deletes a call
/// from its pending set as soon as it is resolved, so a second result for
/// the same call id is reported as an orphan result.
pub fn validate_conversation_items(
    items: &[ConversationItem],
) -> Result<(), TranscriptFailure> {
    // Pending calls in insertion order; resolved entries are skipped,
    // which matches the reference's insertion-ordered map with
    // delete-on-resolve semantics for the deterministic first failure.
    let mut pending: Vec<(String, String, bool)> = Vec::new();
    for item in items {
        match item {
            ConversationItem::UserMessage { .. } => {
                if let Some((call_id, tool_name, _)) =
                    pending.iter().find(|(_, _, resolved)| !resolved)
                {
                    return Err(TranscriptFailure::UnresolvedBeforeUser {
                        call_id: call_id.clone(),
                        tool_name: tool_name.clone(),
                    });
                }
            }
            ConversationItem::AssistantToolCall {
                call_id, tool_name, ..
            } => {
                if call_id.is_empty() {
                    return Err(TranscriptFailure::EmptyCallId);
                }
                if pending.iter().any(|(id, _, _)| id == call_id) {
                    return Err(TranscriptFailure::DuplicateCallId {
                        call_id: call_id.clone(),
                    });
                }
                pending.push((call_id.clone(), tool_name.clone(), false));
            }
            ConversationItem::ToolResult { call_id, .. } => {
                let mut found = false;
                let mut resolved = false;
                for (id, _, resolved_flag) in &mut pending {
                    if id == call_id {
                        found = true;
                        if *resolved_flag {
                            resolved = true;
                        } else {
                            *resolved_flag = true;
                        }
                        break;
                    }
                }
                // The reference removes a call from its pending set as
                // soon as it is resolved, so a second result for the
                // same id is reported as an orphan result, exactly like
                // a result without any recorded call.
                if !found || resolved {
                    return Err(TranscriptFailure::OrphanResult {
                        call_id: call_id.clone(),
                    });
                }
            }
            ConversationItem::AssistantMessage { .. } => {}
        }
    }
    if let Some((call_id, tool_name, _)) =
        pending.iter().find(|(_, _, resolved)| !resolved)
    {
        return Err(TranscriptFailure::UnresolvedAtEnd {
            call_id: call_id.clone(),
            tool_name: tool_name.clone(),
        });
    }
    Ok(())
}
