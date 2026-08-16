//! Deterministic fake provider (Stage 3R R7.1).
//!
//! Identity 'deterministic-fake'. Produces deterministic event streams:
//! the default echo 'Siralos received: <latest user prompt>' (or
//! 'Siralos received:' when no user message exists), split into chunks
//! of exactly 16 Unicode scalar values that never split a code point,
//! plus the generic workspace list/read/search tool scenarios. A Tool
//! must be present in the request definitions before the provider may
//! request it; otherwise it falls back to text behavior. Tool results
//! are read only from items after the latest user message, so a
//! previous turn's result is never reused. No randomness, no
//! timestamps, no environment-dependent output, and identical requests
//! produce identical event sequences.

use serde_json::{Value, json};
use siralos_core::provider::{
    CancellationSignal, ConversationItem, ModelEvent, ModelProvider,
    ModelRequest, ProviderEvent, ToolCallInput, ToolExecutionResult,
};

/// Stable provider identity.
pub const DETERMINISTIC_FAKE_PROVIDER_ID: &str = "deterministic-fake";

/// Chunk size in Unicode scalar values (code points), matching the
/// reference.
const CHUNK_SIZE: usize = 16;

/// The abort message matching the reference AbortError.
const ABORT_MESSAGE: &str = "The fake provider was aborted.";

/// The R7.1 deterministic fake provider (stateless).
#[derive(Debug, Clone, Copy, Default)]
pub struct DeterministicFakeProvider;

impl DeterministicFakeProvider {
    /// Construct the provider.
    pub fn new() -> Self {
        Self
    }
}

/// The generic tool scenarios the R7.1 fake provider can propose.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Scenario {
    /// 'list files' -> workspace.list {"path": "."}
    List,
    /// 'read README.md' -> workspace.read {"path": "README.md"}
    Read,
    /// 'search <text>' -> workspace.search {"query": <text>, "path": "."}
    Search {
        /// The trimmed query text after the 'search ' prefix.
        query: String,
    },
}

impl Scenario {
    fn tool_name(&self) -> &'static str {
        match self {
            Scenario::List => "workspace.list",
            Scenario::Read => "workspace.read",
            Scenario::Search { .. } => "workspace.search",
        }
    }

    fn input(&self) -> Value {
        match self {
            Scenario::List => json!({ "path": "." }),
            Scenario::Read => json!({ "path": "README.md" }),
            Scenario::Search { query } => json!({
                "query": query,
                "path": "."
            }),
        }
    }
}

/// The deterministic stream for one request.
///
/// Holds only the read-only cancellation observation view: the fake
/// provider can observe Host cancellation and stop cooperatively, but it
/// cannot mutate Host cancellation state.
pub struct FakeStream<'a> {
    cancellation: CancellationSignal<'a>,
    pending_call: Option<ModelEvent>,
    chunks: std::vec::IntoIter<String>,
    completed_emitted: bool,
}

impl Iterator for FakeStream<'_> {
    type Item = ProviderEvent;

    fn next(&mut self) -> Option<ProviderEvent> {
        if self.cancellation.is_cancelled() {
            return Some(ProviderEvent::Cancelled {
                message: ABORT_MESSAGE.to_owned(),
            });
        }
        if let Some(call) = self.pending_call.take() {
            return Some(ProviderEvent::Event(call));
        }
        if let Some(chunk) = self.chunks.next() {
            return Some(ProviderEvent::Event(ModelEvent::TextDelta {
                text: chunk,
            }));
        }
        if !self.completed_emitted {
            self.completed_emitted = true;
            return Some(ProviderEvent::Event(ModelEvent::Completed));
        }
        None
    }
}

impl ModelProvider for DeterministicFakeProvider {
    type Stream<'a>
        = FakeStream<'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        DETERMINISTIC_FAKE_PROVIDER_ID
    }

    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
        cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        let prompt = latest_user_prompt(&request.messages);
        let scenario = find_scenario(&prompt);
        let mut pending_call = None;
        let mut text = None;
        if let Some(scenario) = scenario {
            if let Some(result) =
                find_result_for_call(&request.messages, "call-1")
            {
                text = Some(format_scenario_response(&scenario, &result));
            } else if is_tool_available(&request.tools, scenario.tool_name()) {
                pending_call = Some(ModelEvent::ToolCall {
                    call_id: "call-1".to_owned(),
                    tool_name: scenario.tool_name().to_owned(),
                    input: ToolCallInput::from_value(scenario.input()),
                });
            }
        }
        let chunks = match (pending_call.is_some(), text) {
            (true, _) => Vec::new(),
            (false, Some(text)) => chunk_text(&text, CHUNK_SIZE),
            (false, None) => {
                chunk_text(&format!("Siralos received: {prompt}"), CHUNK_SIZE)
            }
        };
        FakeStream {
            cancellation,
            pending_call,
            chunks: chunks.into_iter(),
            completed_emitted: false,
        }
    }
}

/// The content of the latest user message, or an empty string.
fn latest_user_prompt(messages: &[ConversationItem]) -> String {
    for item in messages.iter().rev() {
        if let ConversationItem::UserMessage { content } = item {
            return content.clone();
        }
    }
    String::new()
}

/// Resolve the generic tool scenario from the latest user prompt.
fn find_scenario(prompt: &str) -> Option<Scenario> {
    if prompt == "list files" {
        return Some(Scenario::List);
    }
    if prompt == "read README.md" {
        return Some(Scenario::Read);
    }
    if let Some(rest) = prompt.strip_prefix("search ") {
        let query = rest.trim();
        if !query.is_empty() {
            return Some(Scenario::Search { query: query.to_owned() });
        }
    }
    None
}

/// The result for a call id in the current prompt/turn segment (items
/// after the latest user message); previous-turn results are never
/// reused.
fn find_result_for_call(
    messages: &[ConversationItem],
    call_id: &str,
) -> Option<ToolExecutionResult> {
    let first_item_of_current_turn = latest_user_message_index(messages) + 1;
    for item in messages[first_item_of_current_turn..].iter().rev() {
        if let ConversationItem::ToolResult {
            call_id: item_call_id,
            result,
            ..
        } = item
        {
            if item_call_id == call_id {
                return Some(result.clone());
            }
        }
    }
    None
}

fn latest_user_message_index(messages: &[ConversationItem]) -> usize {
    for (index, item) in messages.iter().enumerate().rev() {
        if matches!(item, ConversationItem::UserMessage { .. }) {
            return index;
        }
    }
    0
}

fn is_tool_available(
    tools: &[siralos_core::provider::ToolDefinition],
    tool_name: &str,
) -> bool {
    tools.iter().any(|tool| tool.name == tool_name)
}

/// The formatted follow-up text for a completed generic scenario.
fn format_scenario_response(
    scenario: &Scenario,
    result: &ToolExecutionResult,
) -> String {
    let ToolExecutionResult::Success { output, .. } = result else {
        return format!(
            "Siralos could not complete the workspace operation: {}",
            result.message()
        );
    };
    match scenario {
        Scenario::List => match count_array_field(output, "entries") {
            Some(count) => {
                format!("Siralos inspected {count} workspace entries.")
            }
            None => "Siralos inspected the workspace entries.".to_owned(),
        },
        Scenario::Read => "Siralos read README.md.".to_owned(),
        Scenario::Search { .. } => {
            match count_array_field(output, "matches") {
                Some(count) => {
                    format!("Siralos found {count} matching lines.")
                }
                None => "Siralos searched the workspace.".to_owned(),
            }
        }
    }
}

/// The length of an array field of an output object, or None.
fn count_array_field(output: &Value, key: &str) -> Option<usize> {
    let record = output.as_object()?;
    let value = record.get(key)?;
    value.as_array().map(Vec::len)
}

/// Split text into chunks of at most 'size' Unicode scalar values; a
/// chunk never splits a code point and recombining reproduces the
/// original text exactly.
fn chunk_text(text: &str, size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut count = 0;
    for character in text.chars() {
        current.push(character);
        count += 1;
        if count >= size {
            chunks.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}
