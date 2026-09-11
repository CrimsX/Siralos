//! Session replay-run composition for decision 77 — in-process
//! record-then-replay; nothing persisted.
//!
//! Recorded-response playback for decision 68 section 3 — serves
//! determinism-port recordings as `ProviderEvent`s; recordings live in memory
//! only.

use serde_json::Value;
use siralos_core::determinism::ReplayRecording;
use siralos_core::provider::{
    CancellationSignal, ModelEvent, ModelProvider, ModelRequest, ProviderEvent,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Convert a sanitized bounded body text into `ProviderEvent`s.
///
/// Mirrors the tail of `GenericProvider::call_generic`: OpenAI `choices` shape
/// first, then the Anthropic `content`/`tool_use` fallback shape, then the
/// empty-events fallback that pushes an empty `TextDelta`, and finally a
/// `Completed` event.
pub(crate) fn completion_events_from_body(text: &str) -> Vec<ProviderEvent> {
    let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let mut events = Vec::new();
    let choices = value
        .get("choices")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for choice in choices {
        let message = choice.get("message").cloned().unwrap_or(Value::Null);
        if let Some(content) = message.get("content").and_then(|v| v.as_str())
        {
            if !content.is_empty() {
                events.push(ProviderEvent::Event(ModelEvent::TextDelta {
                    text: content.to_owned(),
                }));
            }
        }
        if let Some(tool_calls) =
            message.get("tool_calls").and_then(|v| v.as_array())
        {
            for call in tool_calls {
                let id = call
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let name = call
                    .get("function")
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let args_str = call
                    .get("function")
                    .and_then(|v| v.get("arguments"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let input_val = serde_json::from_str::<Value>(args_str)
                    .unwrap_or(Value::String(args_str.to_owned()));
                if id.is_empty() || name.is_empty() {
                    continue;
                }
                let input = siralos_core::provider::ToolCallInput::from_value(
                    input_val,
                );
                events.push(ProviderEvent::Event(ModelEvent::ToolCall {
                    call_id: id,
                    tool_name: name,
                    input,
                }));
            }
        }
    }
    if events.is_empty() {
        if let Some(content) = value
            .get("content")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
        {
            if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    events.push(ProviderEvent::Event(ModelEvent::TextDelta {
                        text: text.to_owned(),
                    }));
                }
            }
            if content.get("type").and_then(|v| v.as_str()) == Some("tool_use")
            {
                let id = content
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let name = content
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let input_val =
                    content.get("input").cloned().unwrap_or(Value::Null);
                if !id.is_empty() && !name.is_empty() {
                    let input =
                        siralos_core::provider::ToolCallInput::from_value(
                            input_val,
                        );
                    events.push(ProviderEvent::Event(ModelEvent::ToolCall {
                        call_id: id,
                        tool_name: name,
                        input,
                    }));
                }
            }
        }
        if let Some(content_arr) =
            value.get("content").and_then(|v| v.as_array())
        {
            for block in content_arr.iter().skip(1) {
                if block.get("type").and_then(|v| v.as_str())
                    == Some("tool_use")
                {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let input_val =
                        block.get("input").cloned().unwrap_or(Value::Null);
                    if !id.is_empty() && !name.is_empty() {
                        let input =
                            siralos_core::provider::ToolCallInput::from_value(
                                input_val,
                            );
                        events.push(ProviderEvent::Event(
                            ModelEvent::ToolCall {
                                call_id: id,
                                tool_name: name,
                                input,
                            },
                        ));
                    }
                } else if let Some(text) =
                    block.get("text").and_then(|v| v.as_str())
                {
                    if !text.is_empty() {
                        events.push(ProviderEvent::Event(
                            ModelEvent::TextDelta { text: text.to_owned() },
                        ));
                    }
                }
            }
        }
    }
    if events.is_empty() {
        events.push(ProviderEvent::Event(ModelEvent::TextDelta {
            text: String::new(),
        }));
    }
    events.push(ProviderEvent::Event(ModelEvent::Completed));
    events
}

/// Recorded-response replay provider for determinism-port recordings.
///
/// Serves in-memory [`ReplayRecording`]s as `ProviderEvent`s. Recording data
/// lives in memory only and is consumed sequentially.
///
/// The `model` is a live cell like the HTTP adapters (a `/model` switch
/// updates the label it reports; playback itself serves the fixed
/// recordings and never re-matches on the model).
#[derive(Debug)]
pub struct RecordedReplayProvider {
    /// Provider identifier.
    provider_id: String,
    /// Model identifier.
    model: Rc<RefCell<String>>,
    /// Recordings to serve in order.
    recordings: Vec<ReplayRecording>,
    /// Cursor into `recordings`.
    cursor: core::cell::Cell<usize>,
}

impl RecordedReplayProvider {
    /// Create a new replay provider over the given recordings.
    #[must_use]
    pub fn new(
        provider_id: String,
        model: String,
        recordings: Vec<ReplayRecording>,
    ) -> Self {
        Self {
            provider_id,
            model: Rc::new(RefCell::new(model)),
            recordings,
            cursor: core::cell::Cell::new(0),
        }
    }

    /// Number of recordings remaining to be served.
    #[must_use]
    pub fn recordings_remaining(&self) -> usize {
        self.recordings.len().saturating_sub(self.cursor.get())
    }

    /// Replace the live model label in place (playback still serves the
    /// fixed recordings; the label is what the session reports).
    pub fn set_model(&self, model: String) {
        *self.model.borrow_mut() = model;
    }

    /// The current live model label.
    #[must_use]
    pub fn live_model(&self) -> String {
        self.model.borrow().clone()
    }
}

impl ModelProvider for RecordedReplayProvider {
    type Stream<'a>
        = Box<dyn Iterator<Item = ProviderEvent> + 'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        &self.provider_id
    }

    fn stream<'a>(
        &'a self,
        _request: &'a ModelRequest,
        cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        if cancellation.is_cancelled() {
            return Box::new(std::iter::once(ProviderEvent::Cancelled {
                message: "Host cancelled the turn before provider start"
                    .to_owned(),
            }));
        }
        let index = self.cursor.get();
        if index >= self.recordings.len() {
            return Box::new(std::iter::once(ProviderEvent::Failed(
                "no recorded response for replay: recording exhausted"
                    .to_owned(),
            )));
        }
        self.cursor.set(index + 1);
        let recording = &self.recordings[index];
        let events = completion_events_from_body(&recording.body);
        Box::new(events.into_iter())
    }
}

impl std::fmt::Display for RecordedReplayProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RecordedReplayProvider({}:[REDACTED])", self.provider_id)
    }
}

/// composes a replay provider from the recorder's detached snapshot
/// (recordings_snapshot()); the recorder keeps its recordings — the provider
/// serves a copy.
#[must_use]
pub fn replay_provider_from_recorder(
    provider_id: String,
    model: String,
    recorder: &siralos_core::determinism::RetainingReplayRecorder,
) -> RecordedReplayProvider {
    RecordedReplayProvider::new(
        provider_id,
        model,
        recorder.records_snapshot(),
    )
}

/// Session replay-run composition: in-process record-then-replay.
///
/// The composer owns a retaining recorder for the record phase and composes a
/// replay provider from its detached snapshot. Nothing is persisted.
#[derive(Debug)]
pub struct SessionReplayComposer {
    provider_id: String,
    model: String,
    recorder: std::rc::Rc<siralos_core::determinism::RetainingReplayRecorder>,
}

impl SessionReplayComposer {
    /// Create a composer that owns its retaining recorder.
    #[must_use]
    pub fn new(provider_id: String, model: String) -> Self {
        Self {
            provider_id,
            model,
            recorder: std::rc::Rc::new(
                siralos_core::determinism::RetainingReplayRecorder::new(),
            ),
        }
    }

    /// The recorder to attach to the live provider for the record phase.
    #[must_use]
    pub fn recorder(
        &self,
    ) -> std::rc::Rc<siralos_core::determinism::RetainingReplayRecorder> {
        std::rc::Rc::clone(&self.recorder)
    }

    /// Compose a replay provider from the recorder's detached snapshot.
    #[must_use]
    pub fn compose(&self) -> RecordedReplayProvider {
        replay_provider_from_recorder(
            self.provider_id.clone(),
            self.model.clone(),
            &self.recorder,
        )
    }

    /// Evidence for the current recorder state.
    #[must_use]
    pub fn evidence(
        &self,
    ) -> siralos_core::determinism::SessionReplayEvidence {
        let len = self.recorder.records_snapshot().len();
        siralos_core::determinism::SessionReplayEvidence {
            provider_id: self.provider_id.clone(),
            model: self.model.clone(),
            recorded_count: len,
            recorder_snapshot_count: len,
        }
    }
}
