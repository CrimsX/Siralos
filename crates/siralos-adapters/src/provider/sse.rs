//! Server-sent-event assembly for streamed chat completions (S2 chunk 3).
//!
//! The generic provider used to read the whole response body and convert it
//! once, so a turn produced every event at the end -- which is why the UI
//! could not repaint while the model was working. This module turns the SSE
//! stream back into the SAME events, incrementally, and assembles the
//! equivalent non-streamed body so RECORDINGS stay replayable through
//! [`crate::provider::replay::completion_events_from_body`].
//!
//! Pure by construction: `push_chunk` takes raw text and returns the events
//! that text completed. No I/O, no clock, no provider specifics beyond the
//! documented OpenAI-compatible delta shape.

use crate::provider::MAX_RESPONSE_BYTES;
use serde_json::{Value, json};
use siralos_core::provider::{ModelEvent, ProviderEvent, ToolCallInput};

/// The most tool calls one streamed turn may accumulate. Above this the
/// frame's call is ignored: the index comes from the provider, so it cannot
/// be allowed to size an allocation.
const MAX_STREAMED_TOOL_CALLS: usize = 64;
const MAX_STREAM_EVENTS: usize = 4096;
const MAX_MODE_PROBE_BYTES: usize = 4096;

fn valid_stream_text(text: &str) -> bool {
    text.chars().all(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
}

fn valid_metadata_text(text: &str, max_bytes: usize) -> bool {
    !text.is_empty()
        && text.len() <= max_bytes
        && text.chars().all(|ch| !ch.is_control())
}

fn bounded_metadata_value(value: &Value, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    match value {
        Value::String(text) => valid_metadata_text(text, 4096),
        Value::Array(values) => {
            values.len() <= 64
                && values
                    .iter()
                    .all(|value| bounded_metadata_value(value, depth + 1))
        }
        Value::Object(object) => {
            object.len() <= 64
                && object.iter().all(|(key, value)| {
                    valid_metadata_text(key, 128)
                        && bounded_metadata_value(value, depth + 1)
                })
        }
        _ => true,
    }
}

/// One accumulated tool call (OpenAI streams them by index, in pieces).
#[derive(Clone, Default, PartialEq, Eq)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

impl std::fmt::Debug for PartialCall {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("PartialCall")
            .field("id_len", &self.id.len())
            .field("name_len", &self.name.len())
            .field("arguments_len", &self.arguments.len())
            .finish()
    }
}

/// Incremental assembler for one streamed completion.
pub struct CompletionStream {
    /// Bytes accepted so far, against `limit`.
    bytes: usize,
    limit: usize,
    /// Partial trailing line (SSE frames arrive split across reads).
    pending: String,
    event_id: String,
    event_type: Option<String>,
    model: String,
    content: String,
    /// Reasoning text the route volunteered (S3 renders it; nothing is
    /// emitted for it yet).
    reasoning: String,
    calls: Vec<PartialCall>,
    finish_reason: Option<String>,
    usage: Option<Value>,
    completed: bool,
    /// Number of successful provider events emitted by this assembler.
    event_count: usize,
    /// A terminal decode failure; no later event may be successful.
    failed: bool,
    /// A raw (non-SSE) body that arrived instead of a stream.
    plain_body: Option<String>,
}

impl std::fmt::Debug for CompletionStream {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("CompletionStream")
            .field("bytes", &self.bytes)
            .field("limit", &self.limit)
            .field("pending_len", &self.pending.len())
            .field("event_id_len", &self.event_id.len())
            .field("model_configured", &!self.model.is_empty())
            .field("content_len", &self.content.len())
            .field("reasoning_len", &self.reasoning.len())
            .field("tool_call_count", &self.calls.len())
            .field("completed", &self.completed)
            .field("failed", &self.failed)
            .field(
                "plain_body_len",
                &self.plain_body.as_ref().map(String::len),
            )
            .finish()
    }
}

impl CompletionStream {
    /// An assembler bounded to `limit` response bytes.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        let limit = limit.min(MAX_RESPONSE_BYTES);
        Self {
            bytes: 0,
            limit,
            pending: String::new(),
            event_id: String::new(),
            event_type: None,
            model: String::new(),
            content: String::new(),
            reasoning: String::new(),
            calls: Vec::new(),
            finish_reason: None,
            usage: None,
            event_count: 0,
            completed: false,
            failed: false,
            plain_body: None,
        }
    }

    /// True once the assembler saw `[DONE]` or a completion marker.
    #[must_use]
    pub fn is_completed(&self) -> bool {
        self.completed
    }

    /// Whether a malformed or incomplete stream has terminally failed.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        self.failed
    }

    /// The raw body when the endpoint answered with plain JSON instead of a
    /// stream (some gateways ignore `stream: true`).
    #[must_use]
    pub fn plain_body(&self) -> Option<&str> {
        self.plain_body.as_deref()
    }

    /// Bytes accepted so far.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The reasoning text the route volunteered, if any.
    #[must_use]
    pub fn reasoning(&self) -> &str {
        &self.reasoning
    }

    /// Feed one raw response chunk; returns the events it completed.
    pub fn push_chunk(&mut self, chunk: &str) -> Vec<ProviderEvent> {
        if self.failed || self.completed {
            return Vec::new();
        }
        if self.bytes.saturating_add(chunk.len()) > self.limit {
            self.bytes = self.limit.saturating_add(1);
            self.failed = true;
            return vec![ProviderEvent::Failed(format!(
                "{chunk_len} more bytes would exceed the {limit}-byte response bound",
                chunk_len = chunk.len(),
                limit = self.limit,
            ))];
        }
        self.bytes += chunk.len();
        // A plain JSON body (no `data:` framing) is handed back verbatim.
        // Delay the mode decision across whitespace-only chunks so a later
        // JSON chunk is not misclassified as SSE.
        if self.plain_body.is_none() {
            let mut probe = self.pending.clone();
            probe.push_str(chunk);
            let head =
                probe.strip_prefix('\u{feff}').unwrap_or(&probe).trim_start();
            if head.starts_with('{') || head.starts_with('[') {
                self.pending.clear();
                let body = probe.strip_prefix('\u{feff}').unwrap_or(&probe);
                self.plain_body = Some(body.to_owned());
                return Vec::new();
            }
            if head.is_empty() {
                if self.pending.len().saturating_add(chunk.len())
                    > MAX_MODE_PROBE_BYTES
                {
                    self.failed = true;
                    return vec![ProviderEvent::Failed(
                        "provider response framing is too ambiguous"
                            .to_owned(),
                    )];
                }
                self.pending = probe;
                return Vec::new();
            }
            if !self.pending.is_empty() && self.pending.trim().is_empty() {
                self.pending.clear();
            }
        }
        if self.plain_body.is_some() {
            let mut body = self.plain_body.take().unwrap_or_default();
            body.push_str(chunk);
            self.plain_body = Some(body);
            return Vec::new();
        }
        let mut events = Vec::new();
        self.pending.push_str(chunk);
        let buffer = std::mem::take(&mut self.pending);
        let mut consumed = 0usize;
        for (pos, _) in buffer.match_indices('\n') {
            let line = buffer[consumed..pos].trim_end_matches('\r');
            consumed = pos + 1;
            self.consume_line(line, &mut events);
            if self.failed {
                break;
            }
        }
        if !self.failed {
            self.pending.push_str(&buffer[consumed..]);
        }
        events
    }

    /// Flush the last line when the stream ends without a trailing newline.
    pub fn finish(&mut self) -> Vec<ProviderEvent> {
        let mut events = Vec::new();
        if self.failed {
            return events;
        }
        let tail = std::mem::take(&mut self.pending);
        let tail = tail.trim();
        if !tail.is_empty() {
            self.consume_line(tail, &mut events);
        }
        if self.plain_body.is_some() {
            return events;
        }
        if !self.failed && !self.completed {
            self.failed = true;
            events.push(ProviderEvent::Failed(
                "provider stream ended before an explicit [DONE]".to_owned(),
            ));
        }
        events
    }

    fn emit_event(
        &mut self,
        events: &mut Vec<ProviderEvent>,
        event: ProviderEvent,
    ) -> bool {
        if self.event_count >= MAX_STREAM_EVENTS {
            self.failed = true;
            events.push(ProviderEvent::Failed(
                "provider SSE event count exceeds its bound".to_owned(),
            ));
            false
        } else {
            self.event_count += 1;
            events.push(event);
            true
        }
    }

    fn consume_line(&mut self, line: &str, events: &mut Vec<ProviderEvent>) {
        if self.failed || self.completed {
            return;
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            return;
        }
        if let Some(event) = line.strip_prefix("event:") {
            let event = event.trim();
            if !valid_metadata_text(event, 128) {
                self.failed = true;
                events.push(ProviderEvent::Failed(
                    "provider SSE event type is invalid".to_owned(),
                ));
                return;
            }
            self.event_type = Some(event.to_owned());
            if event.eq_ignore_ascii_case("error") {
                self.failed = true;
                events.push(ProviderEvent::Failed(
                    "provider SSE event declares an error".to_owned(),
                ));
            }
            return;
        }
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if self.completed {
            self.failed = true;
            events.push(ProviderEvent::Failed(
                "provider SSE frame arrived after [DONE]".to_owned(),
            ));
            return;
        }
        if data == "[DONE]" {
            events.extend(self.commit());
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            self.failed = true;
            events.push(ProviderEvent::Failed(
                "provider SSE frame is not valid JSON".to_owned(),
            ));
            return;
        };
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            if !valid_metadata_text(id, 256) {
                self.failed = true;
                events.push(ProviderEvent::Failed(
                    "provider SSE event id is invalid".to_owned(),
                ));
                return;
            }
            if self.event_id.is_empty() {
                self.event_id = id.to_owned();
            }
        }
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            if !valid_metadata_text(model, 256) {
                self.failed = true;
                events.push(ProviderEvent::Failed(
                    "provider SSE model metadata is invalid".to_owned(),
                ));
                return;
            }
            if self.model.is_empty() {
                self.model = model.to_owned();
            }
        }
        if let Some(usage) = value.get("usage") {
            if !usage.is_null() {
                if !bounded_metadata_value(usage, 0) {
                    self.failed = true;
                    events.push(ProviderEvent::Failed(
                        "provider SSE usage metadata is invalid".to_owned(),
                    ));
                    return;
                }
                self.usage = Some(usage.clone());
            }
        }
        if value.get("error").is_some() {
            self.failed = true;
            events.push(ProviderEvent::Failed(
                "provider SSE frame contains an explicit error".to_owned(),
            ));
            return;
        }
        let status_failed = match value.get("status") {
            Some(Value::String(status))
                if matches!(
                    status.to_ascii_lowercase().as_str(),
                    "failed"
                        | "incomplete"
                        | "cancelled"
                        | "canceled"
                        | "error"
                ) =>
            {
                true
            }
            Some(Value::Number(status)) => !status
                .as_u64()
                .is_some_and(|status| (200..300).contains(&status)),
            Some(Value::Null) | None => false,
            Some(_) => true,
        };
        if status_failed {
            self.failed = true;
            events.push(ProviderEvent::Failed(
                "provider SSE frame reports a failed status".to_owned(),
            ));
            return;
        }
        let Some(choices) = value.get("choices").and_then(Value::as_array)
        else {
            self.failed = true;
            events.push(ProviderEvent::Failed(
                "provider SSE frame is missing choices".to_owned(),
            ));
            return;
        };
        if choices.is_empty() {
            if self.usage.is_some() {
                return;
            }
            self.failed = true;
            events.push(ProviderEvent::Failed(
                "provider SSE frame contains no choices".to_owned(),
            ));
            return;
        }
        if choices.len() > 1 {
            self.failed = true;
            events.push(ProviderEvent::Failed(
                "provider SSE frame contains multiple choices".to_owned(),
            ));
            return;
        }
        for choice in choices {
            if choice.get("error").is_some() {
                self.failed = true;
                events.push(ProviderEvent::Failed(
                    "provider SSE choice contains an explicit error"
                        .to_owned(),
                ));
                return;
            }
            if let Some(reason) =
                choice.get("finish_reason").and_then(Value::as_str)
            {
                if reason.eq_ignore_ascii_case("error") {
                    self.failed = true;
                    events.push(ProviderEvent::Failed(
                        "provider SSE choice contains an explicit error"
                            .to_owned(),
                    ));
                    return;
                }
                if !valid_metadata_text(reason, 64) {
                    self.failed = true;
                    events.push(ProviderEvent::Failed(
                        "provider SSE finish metadata is invalid".to_owned(),
                    ));
                    return;
                }
                self.finish_reason = Some(reason.to_owned());
            }
            let Some(delta) = choice.get("delta") else {
                self.failed = true;
                events.push(ProviderEvent::Failed(
                    "provider SSE choice is missing delta".to_owned(),
                ));
                return;
            };
            if !delta.is_object() {
                self.failed = true;
                events.push(ProviderEvent::Failed(
                    "provider SSE delta must be an object".to_owned(),
                ));
                return;
            }
            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                if !valid_stream_text(text) {
                    self.failed = true;
                    events.push(ProviderEvent::Failed(
                        "provider SSE text contains an unsupported control"
                            .to_owned(),
                    ));
                    return;
                }
                if !text.is_empty() {
                    self.content.push_str(text);
                    if !self.emit_event(
                        events,
                        ProviderEvent::Event(ModelEvent::TextDelta {
                            text: text.to_owned(),
                        }),
                    ) {
                        return;
                    }
                }
            }
            for field in ["reasoning", "reasoning_content"] {
                if let Some(text) = delta.get(field).and_then(Value::as_str) {
                    if !valid_stream_text(text) {
                        self.failed = true;
                        events.push(ProviderEvent::Failed(
                            "provider SSE reasoning contains an unsupported control"
                                .to_owned(),
                        ));
                        return;
                    }
                    if !text.is_empty() {
                        self.reasoning.push_str(text);
                        // S3: thinking is streamed on its own channel.
                        if !self.emit_event(
                            events,
                            ProviderEvent::Event(ModelEvent::ReasoningDelta {
                                text: text.to_owned(),
                            }),
                        ) {
                            return;
                        }
                    }
                }
            }
            if let Some(raw_calls) = delta.get("tool_calls") {
                let Some(calls) = raw_calls.as_array() else {
                    self.failed = true;
                    events.push(ProviderEvent::Failed(
                        "provider SSE tool_calls must be an array".to_owned(),
                    ));
                    return;
                };
                for call in calls {
                    let Some(index) = call
                        .get("index")
                        .and_then(Value::as_u64)
                        .and_then(|value| usize::try_from(value).ok())
                    else {
                        self.failed = true;
                        events.push(ProviderEvent::Failed(
                            "provider SSE tool call index is invalid"
                                .to_owned(),
                        ));
                        return;
                    };
                    // The index is UNTRUSTED. Growing to it would let a
                    // ~70-byte frame asking for index 2^32 allocate gigabytes
                    // -- the 1 MiB body bound does not bound memory.
                    if index >= MAX_STREAMED_TOOL_CALLS {
                        self.failed = true;
                        events.push(ProviderEvent::Failed(
                            "provider SSE tool call index exceeds its bound"
                                .to_owned(),
                        ));
                        return;
                    }
                    while self.calls.len() <= index {
                        self.calls.push(PartialCall::default());
                    }
                    let slot = &mut self.calls[index];
                    if let Some(id_value) = call.get("id") {
                        let Some(id) = id_value.as_str() else {
                            self.failed = true;
                            events.push(ProviderEvent::Failed(
                                "provider SSE tool call id must be text"
                                    .to_owned(),
                            ));
                            return;
                        };
                        if !valid_metadata_text(id, 256) {
                            self.failed = true;
                            events.push(ProviderEvent::Failed(
                                "provider SSE tool call id is invalid"
                                    .to_owned(),
                            ));
                            return;
                        }
                        if !id.is_empty() && slot.id.is_empty() {
                            slot.id = id.to_owned();
                        }
                    }
                    if let Some(function_value) = call.get("function") {
                        let Some(function) = function_value.as_object() else {
                            self.failed = true;
                            events.push(ProviderEvent::Failed(
                                "provider SSE tool call function must be an object"
                                    .to_owned(),
                            ));
                            return;
                        };
                        if let Some(name_value) = function.get("name") {
                            let Some(name) = name_value.as_str() else {
                                self.failed = true;
                                events.push(ProviderEvent::Failed(
                                    "provider SSE tool call name must be text"
                                        .to_owned(),
                                ));
                                return;
                            };
                            if !valid_metadata_text(
                                name,
                                crate::provider::tool_names::MAX_PROVIDER_TOOL_NAME_BYTES,
                            ) {
                                self.failed = true;
                                events.push(ProviderEvent::Failed(
                                    "provider SSE tool call name is invalid"
                                        .to_owned(),
                                ));
                                return;
                            }
                            if !name.is_empty() && slot.name.is_empty() {
                                slot.name = name.to_owned();
                            }
                        }
                        if let Some(args_value) = function.get("arguments") {
                            let Some(args) = args_value.as_str() else {
                                self.failed = true;
                                events.push(ProviderEvent::Failed(
                                    "provider SSE tool arguments must be text"
                                        .to_owned(),
                                ));
                                return;
                            };
                            if !valid_stream_text(args) {
                                self.failed = true;
                                events.push(ProviderEvent::Failed(
                                    "provider SSE tool arguments contain an unsupported control"
                                        .to_owned(),
                                ));
                                return;
                            }
                            slot.arguments.push_str(args);
                        }
                    }
                }
            }
        }
    }

    /// Emit the tool calls and the completion, exactly once.
    fn commit(&mut self) -> Vec<ProviderEvent> {
        if self.completed || self.failed {
            return Vec::new();
        }
        let mut events = Vec::new();
        let calls = self.calls.clone();
        for call in &calls {
            if call.name.is_empty() {
                if call.id.is_empty() && call.arguments.is_empty() {
                    continue;
                }
                self.failed = true;
                return vec![ProviderEvent::Failed(
                    "provider SSE tool call is missing its name".to_owned(),
                )];
            }
            if call.id.is_empty() {
                self.failed = true;
                return vec![ProviderEvent::Failed(
                    "provider SSE tool call is missing its id".to_owned(),
                )];
            }
            if call.id.len() > 256
                || call.name.len()
                    > crate::provider::tool_names::MAX_PROVIDER_TOOL_NAME_BYTES
                || call.id.chars().any(char::is_control)
                || call.name.chars().any(char::is_control)
            {
                self.failed = true;
                return vec![ProviderEvent::Failed(
                    "provider SSE tool call identifier is invalid".to_owned(),
                )];
            }
            let Ok(input_val) = serde_json::from_str::<Value>(&call.arguments)
            else {
                self.failed = true;
                return vec![ProviderEvent::Failed(
                    "provider SSE tool arguments are not valid JSON"
                        .to_owned(),
                )];
            };
            if !input_val.is_object() {
                self.failed = true;
                return vec![ProviderEvent::Failed(
                    "provider SSE tool arguments must be an object".to_owned(),
                )];
            }
            let event = ProviderEvent::Event(ModelEvent::ToolCall {
                call_id: call.id.clone(),
                tool_name: call.name.clone(),
                input: ToolCallInput::from_value(input_val),
            });
            if !self.emit_event(&mut events, event) {
                return Vec::new();
            }
        }
        if self.content.is_empty()
            && self.reasoning.is_empty()
            && !self.calls.iter().any(|call| {
                !call.name.is_empty()
                    || !call.id.is_empty()
                    || !call.arguments.is_empty()
            })
        {
            self.failed = true;
            return vec![ProviderEvent::Failed(
                "provider SSE completed without a usable event".to_owned(),
            )];
        }
        self.completed = true;
        if !self.emit_event(
            &mut events,
            ProviderEvent::Event(ModelEvent::Completed),
        ) {
            return Vec::new();
        }
        events
    }

    /// The non-streamed body equivalent to what was streamed, so a recording
    /// replays through the SAME converter the live path used to call.
    #[must_use]
    pub fn assembled_body(&self) -> Value {
        let tool_calls: Vec<Value> = self
            .calls
            .iter()
            .filter(|call| !call.name.is_empty() && !call.id.is_empty())
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {
                        "name": call.name,
                        "arguments": call.arguments,
                    },
                })
            })
            .collect();
        let mut message = json!({
            "role": "assistant",
        });
        if !self.reasoning.is_empty() {
            message["reasoning"] = Value::String(self.reasoning.clone());
        }
        if !self.content.is_empty() {
            message["content"] = Value::String(self.content.clone());
        }
        if !tool_calls.is_empty() {
            message["tool_calls"] = Value::Array(tool_calls);
        }
        let mut body = json!({
            "id": self.event_id,
            "model": self.model,
            "choices": [{
                "index": 0,
                "message": message,
                "finish_reason": self
                    .finish_reason
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            }],
        });
        if let Some(usage) = &self.usage {
            body["usage"] = usage.clone();
        }
        body
    }
}

#[cfg(test)]
mod tests {
    use super::CompletionStream;
    use crate::provider::MAX_RESPONSE_BYTES;
    use siralos_core::provider::{ModelEvent, ProviderEvent};

    fn texts(events: &[ProviderEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| match event {
                ProviderEvent::Event(ModelEvent::TextDelta { text }) => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn deltas_split_across_reads_are_reassembled() {
        let mut stream = CompletionStream::new(4096);
        // The SAME frame arrives in three reads, mid-JSON.
        assert!(
            stream
                .push_chunk("data: {\"choices\":[{\"delta\":{\"cont")
                .is_empty(),
            "a half frame yields nothing yet"
        );
        // The frame completes: its text is emitted NOW, not at the end.
        let first = stream.push_chunk("ent\":\"Hel\"}}]}\n");
        assert_eq!(texts(&first), vec!["Hel".to_owned()]);
        let events = stream.push_chunk(
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n",
        );
        assert_eq!(texts(&events), vec!["lo".to_owned()]);
        let done = stream.push_chunk("data: [DONE]\n");
        assert!(stream.is_completed());
        assert!(matches!(
            done.last(),
            Some(ProviderEvent::Event(ModelEvent::Completed))
        ));
    }

    #[test]
    fn rejects_explicit_error_frames_before_emitting_content() {
        let mut stream = CompletionStream::new(4096);
        let events = stream.push_chunk(
            "data: {\"error\":{\"message\":\"failed\"},\"choices\":[{\"delta\":{\"content\":\"secret\"}}]}\n",
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ProviderEvent::Failed(_)))
        );
        assert!(texts(&events).is_empty());
    }

    #[test]
    fn rejects_failed_status_before_emitting_content() {
        let mut stream = CompletionStream::new(4096);
        let events = stream.push_chunk(
            "data: {\"status\":\"failed\",\"choices\":[{\"delta\":{\"content\":\"secret\"}}]}\n",
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ProviderEvent::Failed(_)))
        );
        assert!(texts(&events).is_empty());
    }

    #[test]
    fn rejects_error_finish_reasons() {
        let mut stream = CompletionStream::new(4096);
        let events = stream.push_chunk(
            "data: {\"choices\":[{\"delta\":{\"content\":\"bad\"},\"finish_reason\":\"error\"}]}\n",
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ProviderEvent::Failed(_)))
        );
        assert!(texts(&events).is_empty());
    }
    #[test]
    fn tool_call_arguments_accumulate_across_frames() {
        let mut stream = CompletionStream::new(4096);
        stream.push_chunk("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-a\",\"function\":{\"name\":\"workspace_read\",\"arguments\":\"{\\\"pa\"}}]}}]}\n");
        stream.push_chunk("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"th\\\":\\\"a\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n");
        let events = stream.push_chunk("data: [DONE]\n");
        let call = events
            .iter()
            .find_map(|event| match event {
                ProviderEvent::Event(ModelEvent::ToolCall {
                    call_id,
                    tool_name,
                    input,
                }) => {
                    Some((call_id.clone(), tool_name.clone(), input.clone()))
                }
                _ => None,
            })
            .expect("one tool call");
        assert_eq!(call.0, "call-a");
        assert_eq!(call.1, "workspace_read");
        assert_eq!(call.2.value(), &serde_json::json!({"path": "a"}));
    }

    #[test]
    fn assembled_body_replays_through_the_shared_body_converter() {
        // The recording contract: a streamed turn records a body, and that
        // body must convert to the same events the live stream produced.
        let mut stream = CompletionStream::new(4096);
        let mut live = Vec::new();
        live.extend(stream.push_chunk("data: {\"id\":\"gen-1\",\"model\":\"example/model-a\",\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n"));
        live.extend(stream.push_chunk("data: {\"choices\":[{\"delta\":{\"content\":\" there\"},\"finish_reason\":\"stop\"}]}\n"));
        live.extend(stream.push_chunk("data: [DONE]\n"));
        let body = stream.assembled_body().to_string();
        let replayed =
            crate::provider::replay::completion_events_from_body(&body);
        // The live stream emits per delta; a recorded body merges them into
        // one message. The turn's assistant text is the same either way.
        assert_eq!(texts(&live), vec!["Hi".to_owned(), " there".to_owned()]);
        assert_eq!(texts(&live).join(""), texts(&replayed).join(""));
        assert!(matches!(
            replayed.last(),
            Some(ProviderEvent::Event(ModelEvent::Completed))
        ));
    }

    #[test]
    fn plain_json_body_is_handed_back_instead_of_parsed_as_sse() {
        let mut stream = CompletionStream::new(4096);
        assert!(
            stream
                .push_chunk(
                    "{\"choices\":[{\"message\":{\"content\":\"hi\"}}]}"
                )
                .is_empty()
        );
        assert_eq!(
            stream.plain_body().map(str::trim),
            Some("{\"choices\":[{\"message\":{\"content\":\"hi\"}}]}")
        );
        assert!(!stream.is_completed());
    }

    #[test]
    fn reasoning_streams_on_its_own_channel_and_replays_from_the_body() {
        // S3: the thinking is emitted as ReasoningDelta (never as answer
        // text) and the recorded body carries it, so a replay reproduces
        // the same channel.
        let mut stream = CompletionStream::new(4096);
        let events = stream.push_chunk(
            "data: {\"choices\":[{\"delta\":{\"reasoning\":\"weighing\"}}]}\n",
        );
        assert!(
            matches!(
                events.first(),
                Some(ProviderEvent::Event(ModelEvent::ReasoningDelta { text })) if text == "weighing"
            ),
            "reasoning streams as its own event, got: {events:?}"
        );
        assert!(texts(&events).is_empty(), "reasoning is not answer text");
        stream.push_chunk(
            "data: {\"choices\":[{\"delta\":{\"content\":\"the answer\"}}]}\n",
        );
        stream.push_chunk("data: [DONE]\n");
        let body = stream.assembled_body().to_string();
        let replayed =
            crate::provider::replay::completion_events_from_body(&body);
        assert!(
            replayed.iter().any(|event| matches!(
                event,
                ProviderEvent::Event(ModelEvent::ReasoningDelta { text }) if text == "weighing"
            )),
            "the recorded body must replay the reasoning, got: {replayed:?}"
        );
        assert_eq!(texts(&replayed).join(""), "the answer");
    }

    #[test]
    fn the_response_bound_is_enforced_incrementally() {
        let mut stream = CompletionStream::new(32);
        // A frame with no delta is malformed, so the stream is already failed
        // before the oversize chunk arrives; a failed assembler emits nothing
        // more and never re-opens.
        let first = stream.push_chunk("data: {\"choices\":[{}]}\n");
        assert!(matches!(first.first(), Some(ProviderEvent::Failed(_))));
        let events = stream.push_chunk(&"x".repeat(64));
        assert!(events.is_empty());
        assert!(stream.is_failed());
    }

    #[test]
    fn a_valid_frame_then_an_oversize_chunk_fails_at_the_bound() {
        let mut stream = CompletionStream::new(64);
        let first = stream.push_chunk(
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n",
        );
        assert_eq!(texts(&first), vec!["ok".to_owned()]);
        let events = stream.push_chunk(&"x".repeat(128));
        assert!(matches!(events.first(), Some(ProviderEvent::Failed(_))));
        assert!(stream.is_failed());
    }

    #[test]
    fn whitespace_only_framing_is_bounded_before_a_mode_decision() {
        let mut stream = CompletionStream::new(MAX_RESPONSE_BYTES);
        let chunk = " ".repeat(64);
        let mut failed = false;
        for _ in 0..200 {
            if stream
                .push_chunk(&chunk)
                .iter()
                .any(|event| matches!(event, ProviderEvent::Failed(_)))
            {
                failed = true;
                break;
            }
        }
        assert!(failed, "unbounded whitespace must not probe forever");
    }

    #[test]
    fn control_bearing_sse_metadata_is_rejected_before_replay_projection() {
        let mut stream = CompletionStream::new(4096);
        let events = stream.push_chunk(
            "data: {\"model\":\"bad\\u0007model\",\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n",
        );
        assert!(matches!(events.first(), Some(ProviderEvent::Failed(_))));
        assert!(!stream.assembled_body().to_string().contains("bad"));
    }

    #[test]
    fn malformed_tool_call_shapes_fail_instead_of_being_dropped() {
        for frame in [
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":{}}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":5}]}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":7}}]}}]}\n",
        ] {
            let mut stream = CompletionStream::new(4096);
            let events = stream.push_chunk(frame);
            assert!(
                matches!(events.first(), Some(ProviderEvent::Failed(_))),
                "{frame}"
            );
            assert!(texts(&events).is_empty(), "{frame}");
        }
    }

    #[test]
    fn multiple_choices_in_one_frame_fail_closed() {
        let mut stream = CompletionStream::new(4096);
        let events = stream.push_chunk(
            "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}},{\"delta\":{\"content\":\"b\"}}]}\n",
        );
        assert!(matches!(events.first(), Some(ProviderEvent::Failed(_))));
        assert!(texts(&events).is_empty());
    }

    #[test]
    fn the_advertised_limit_cannot_be_widened_past_the_response_bound() {
        let mut stream = CompletionStream::new(usize::MAX);
        let events = stream.push_chunk(&"x".repeat(MAX_RESPONSE_BYTES + 1));
        assert!(matches!(events.first(), Some(ProviderEvent::Failed(_))));
    }
}
