//! Unit tests for the R7.1 provider contract, bounded turn state
//! machine, transcript validation, and tool-result detach boundary.

use serde_json::{Value, json};

use super::ProviderTurnLimits;
use super::conversation::{
    ConversationItem, TranscriptFailure, validate_conversation_items,
};
use super::event::{
    CancellationToken, ModelEvent, ModelProvider, ModelRequest, ProviderEvent,
    TurnFailure, validate_external_event,
};
use super::result::{
    DetachFailure, ToolExecutionResult, detach_bounded_tool_result,
};
use super::turn::{TurnOutcome, TurnToolCall, collect_provider_turn};

/// A deterministic scripted provider for the tests.
struct ScriptedProvider {
    events: Vec<ProviderEvent>,
}

impl ModelProvider for ScriptedProvider {
    type Stream<'a>
        = std::vec::IntoIter<ProviderEvent>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "scripted-test"
    }

    fn stream<'a>(
        &'a self,
        _request: &'a ModelRequest,
        _cancellation: &'a CancellationToken,
    ) -> Self::Stream<'a> {
        self.events.clone().into_iter()
    }
}

/// A provider that cancels the Host token after a fixed number of
/// emitted events (deterministic harness-style cancellation point).
struct CancellingProvider {
    events: Vec<ProviderEvent>,
    cancel_after: usize,
}

impl ModelProvider for CancellingProvider {
    type Stream<'a>
        = CancellingStream<'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "cancelling-test"
    }

    fn stream<'a>(
        &'a self,
        _request: &'a ModelRequest,
        cancellation: &'a CancellationToken,
    ) -> Self::Stream<'a> {
        CancellingStream {
            events: self.events.clone().into_iter(),
            cancellation,
            emitted: 0,
            cancel_after: self.cancel_after,
        }
    }
}

struct CancellingStream<'a> {
    events: std::vec::IntoIter<ProviderEvent>,
    cancellation: &'a CancellationToken,
    emitted: usize,
    cancel_after: usize,
}

impl Iterator for CancellingStream<'_> {
    type Item = ProviderEvent;

    fn next(&mut self) -> Option<ProviderEvent> {
        if self.emitted == self.cancel_after {
            self.cancellation.cancel();
            return None;
        }
        let event = self.events.next()?;
        self.emitted += 1;
        Some(event)
    }
}

fn text_event(text: &str) -> ProviderEvent {
    ProviderEvent::Event(ModelEvent::TextDelta { text: text.to_owned() })
}

fn call_event(call_id: &str, tool_name: &str, input: Value) -> ProviderEvent {
    ProviderEvent::Event(ModelEvent::ToolCall {
        call_id: call_id.to_owned(),
        tool_name: tool_name.to_owned(),
        input,
    })
}

fn completed_event() -> ProviderEvent {
    ProviderEvent::Event(ModelEvent::Completed)
}

fn user(content: &str) -> ConversationItem {
    ConversationItem::UserMessage { content: content.to_owned() }
}

fn collect(
    events: Vec<ProviderEvent>,
    history: Vec<ConversationItem>,
) -> TurnOutcome {
    let provider = ScriptedProvider { events };
    let token = CancellationToken::new();
    collect_provider_turn(&provider, &history, &[], None, &token)
}

fn collect_with_cancellation(
    events: Vec<ProviderEvent>,
    cancel_after: usize,
) -> TurnOutcome {
    let provider = CancellingProvider { events, cancel_after };
    let token = CancellationToken::new();
    collect_provider_turn(&provider, &[user("hello")], &[], None, &token)
}

fn failed_message(outcome: &TurnOutcome) -> String {
    outcome.failure_message().expect("expected a failed turn")
}

fn assert_failed(outcome: &TurnOutcome, failure: TurnFailure) {
    match outcome {
        TurnOutcome::Failed { failure: actual } => {
            assert_eq!(actual, &failure, "typed failure mismatch");
        }
        other => panic!("expected a failed turn, got {other:?}"),
    }
}

fn successful_turn(
    outcome: &TurnOutcome,
) -> (&str, &[String], &[TurnToolCall]) {
    match outcome {
        TurnOutcome::Turn { assistant_text, text_deltas, tool_calls } => {
            (assistant_text, text_deltas, tool_calls)
        }
        other => panic!("expected a successful turn, got {other:?}"),
    }
}

const LIMITS: ProviderTurnLimits = ProviderTurnLimits {
    max_assistant_text_bytes: 65_536,
    max_text_events: 4096,
    max_tool_calls_per_turn: 32,
    max_call_id_bytes: 256,
    max_tool_name_bytes: 256,
    max_tool_argument_bytes: 131_072,
    max_turn_bytes: 262_144,
};

/// Serialized byte length of {"payload":"<count copies of z>"}.
fn payload_argument_bytes(count: usize) -> usize {
    14 + count
}

fn payload_input(count: usize) -> Value {
    json!({ "payload": "z".repeat(count) })
}

#[test]
fn valid_text_turn() {
    let outcome = collect(
        vec![
            text_event("Siralos received:"),
            text_event(" hello"),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (text, deltas, calls) = successful_turn(&outcome);
    assert_eq!(text, "Siralos received: hello");
    assert_eq!(deltas, &["Siralos received:", " hello"]);
    assert!(calls.is_empty());
}

#[test]
fn empty_text_turn() {
    let outcome = collect(vec![completed_event()], vec![user("hello")]);
    let (text, deltas, calls) = successful_turn(&outcome);
    assert_eq!(text, "");
    assert!(deltas.is_empty());
    assert!(calls.is_empty());
}

#[test]
fn valid_tool_calls_preserve_order() {
    let outcome = collect(
        vec![
            call_event("c1", "a.tool", json!({"n": 1})),
            call_event("c2", "b.tool", json!({"n": 2})),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (_, _, calls) = successful_turn(&outcome);
    assert_eq!(
        calls,
        &[
            TurnToolCall::Execute {
                call_id: "c1".to_owned(),
                tool_name: "a.tool".to_owned(),
                input: json!({"n": 1}),
            },
            TurnToolCall::Execute {
                call_id: "c2".to_owned(),
                tool_name: "b.tool".to_owned(),
                input: json!({"n": 2}),
            },
        ]
    );
}

#[test]
fn text_and_tool_calls_keep_event_order() {
    let outcome = collect(
        vec![
            text_event("prefix"),
            call_event("c1", "a.tool", json!({})),
            text_event("suffix"),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (text, deltas, calls) = successful_turn(&outcome);
    assert_eq!(text, "prefixsuffix");
    assert_eq!(deltas, &["prefix", "suffix"]);
    assert_eq!(calls.len(), 1);
}

#[test]
fn assistant_text_exact_limit_accepted() {
    let outcome = collect(
        vec![
            text_event(&"a".repeat(LIMITS.max_assistant_text_bytes)),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (text, _, _) = successful_turn(&outcome);
    assert_eq!(text.len(), 65_536);
}

#[test]
fn assistant_text_over_limit_rejected() {
    let outcome = collect(
        vec![text_event(&"a".repeat(LIMITS.max_assistant_text_bytes + 1))],
        vec![user("hello")],
    );
    assert_failed(
        &outcome,
        TurnFailure::LimitExceeded(
            super::event::LimitClass::AssistantTextBytes,
        ),
    );
    assert_eq!(
        failed_message(&outcome),
        "The provider exceeded the assistant-text byte limit limit; the response was rejected."
    );
}

#[test]
fn assistant_text_cumulative_across_deltas() {
    let outcome = collect(
        vec![
            text_event(&"a".repeat(32_768)),
            text_event(&"a".repeat(32_768)),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (text, _, _) = successful_turn(&outcome);
    assert_eq!(text.len(), 65_536);

    let over = collect(
        vec![text_event(&"a".repeat(32_768)), text_event(&"a".repeat(32_769))],
        vec![user("hello")],
    );
    assert_failed(
        &over,
        TurnFailure::LimitExceeded(
            super::event::LimitClass::AssistantTextBytes,
        ),
    );
}

#[test]
fn text_event_exact_count_accepted() {
    let events = (0..LIMITS.max_text_events)
        .map(|_| text_event("x"))
        .chain(std::iter::once(completed_event()))
        .collect();
    let outcome = collect(events, vec![user("hello")]);
    let (text, deltas, _) = successful_turn(&outcome);
    assert_eq!(text.len(), 4096);
    assert_eq!(deltas.len(), 4096);
}

#[test]
fn text_event_over_count_rejected() {
    let events =
        (0..LIMITS.max_text_events + 1).map(|_| text_event("x")).collect();
    let outcome = collect(events, vec![user("hello")]);
    assert_failed(
        &outcome,
        TurnFailure::LimitExceeded(super::event::LimitClass::TextEventCount),
    );
    assert_eq!(
        failed_message(&outcome),
        "The provider exceeded the text-event count limit; the response was rejected."
    );
}

#[test]
fn tool_call_exact_count_accepted() {
    let events = (0..LIMITS.max_tool_calls_per_turn)
        .map(|index| call_event(&format!("c{index}"), "a.tool", json!({})))
        .chain(std::iter::once(completed_event()))
        .collect();
    let outcome = collect(events, vec![user("hello")]);
    let (_, _, calls) = successful_turn(&outcome);
    assert_eq!(calls.len(), 32);
}

#[test]
fn tool_call_over_count_rejected() {
    let events = (0..LIMITS.max_tool_calls_per_turn + 1)
        .map(|index| call_event(&format!("c{index}"), "a.tool", json!({})))
        .collect();
    let outcome = collect(events, vec![user("hello")]);
    assert_failed(
        &outcome,
        TurnFailure::LimitExceeded(super::event::LimitClass::ToolCallCount),
    );
    assert_eq!(
        failed_message(&outcome),
        "The provider exceeded the tool-call count limit; the response was rejected."
    );
}

#[test]
fn call_id_exact_bytes_accepted() {
    let outcome = collect(
        vec![
            call_event(
                &"c".repeat(LIMITS.max_call_id_bytes),
                "a.tool",
                json!({}),
            ),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (_, _, calls) = successful_turn(&outcome);
    assert_eq!(calls.len(), 1);
}

#[test]
fn call_id_over_bytes_rejected() {
    let outcome = collect(
        vec![call_event(
            &"c".repeat(LIMITS.max_call_id_bytes + 1),
            "a.tool",
            json!({}),
        )],
        vec![user("hello")],
    );
    assert_failed(
        &outcome,
        TurnFailure::LimitExceeded(super::event::LimitClass::CallIdBytes),
    );
    assert_eq!(
        failed_message(&outcome),
        "The provider exceeded the tool-call id byte limit limit; the response was rejected."
    );
}

#[test]
fn tool_name_exact_bytes_accepted() {
    let outcome = collect(
        vec![
            call_event(
                "c1",
                &"t".repeat(LIMITS.max_tool_name_bytes),
                json!({}),
            ),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (_, _, calls) = successful_turn(&outcome);
    assert_eq!(calls.len(), 1);
}

#[test]
fn tool_name_over_bytes_rejected() {
    let outcome = collect(
        vec![call_event(
            "c1",
            &"t".repeat(LIMITS.max_tool_name_bytes + 1),
            json!({}),
        )],
        vec![user("hello")],
    );
    assert_failed(
        &outcome,
        TurnFailure::LimitExceeded(super::event::LimitClass::ToolNameBytes),
    );
    assert_eq!(
        failed_message(&outcome),
        "The provider exceeded the tool-name byte limit limit; the response was rejected."
    );
}

#[test]
fn tool_argument_exact_bytes_accepted() {
    let count = LIMITS.max_tool_argument_bytes - 14;
    assert_eq!(payload_argument_bytes(count), 131_072);
    let outcome = collect(
        vec![
            call_event("c1", "a.tool", payload_input(count)),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (_, _, calls) = successful_turn(&outcome);
    assert_eq!(calls.len(), 1);
}

#[test]
fn tool_argument_over_bytes_rejected() {
    let count = LIMITS.max_tool_argument_bytes - 13;
    assert_eq!(payload_argument_bytes(count), 131_073);
    let outcome = collect(
        vec![call_event("c1", "a.tool", payload_input(count))],
        vec![user("hello")],
    );
    assert_failed(
        &outcome,
        TurnFailure::LimitExceeded(
            super::event::LimitClass::ToolArgumentBytes,
        ),
    );
    assert_eq!(
        failed_message(&outcome),
        "The provider exceeded the tool-argument byte limit limit; the response was rejected."
    );
}

#[test]
fn aggregate_turn_exact_bytes_accepted() {
    // call-1: id 2 + name 6 + arg (14 + 131_042) = 131_064
    // call-2: id 2 + name 6 + arg (14 + 131_058) = 131_080
    // total: 262_144 exactly.
    let outcome = collect(
        vec![
            call_event("c1", "a.tool", payload_input(131_042)),
            call_event("c2", "a.tool", payload_input(131_058)),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (_, _, calls) = successful_turn(&outcome);
    assert_eq!(calls.len(), 2);
}

#[test]
fn aggregate_turn_over_bytes_rejected() {
    // Two maximum-argument calls: 131_080 + 131_080 = 262_160.
    let outcome = collect(
        vec![
            call_event("c1", "a.tool", payload_input(131_058)),
            call_event("c2", "a.tool", payload_input(131_058)),
        ],
        vec![user("hello")],
    );
    assert_failed(
        &outcome,
        TurnFailure::LimitExceeded(
            super::event::LimitClass::AggregateTurnBytes,
        ),
    );
    assert_eq!(
        failed_message(&outcome),
        "The provider exceeded the aggregate turn byte limit limit; the response was rejected."
    );
}

#[test]
fn multibyte_utf8_byte_accounting() {
    // 32_768 e-acute = 65_536 bytes: exactly at the limit.
    let outcome = collect(
        vec![text_event(&"é".repeat(32_768)), completed_event()],
        vec![user("hello")],
    );
    let (text, _, _) = successful_turn(&outcome);
    assert_eq!(text.len(), 65_536);
    assert_eq!(text.chars().count(), 32_768);

    // One more character pushes the byte count over while the character
    // count (32_769) stays below 65_536.
    let over =
        collect(vec![text_event(&"é".repeat(32_769))], vec![user("hello")]);
    assert_failed(
        &over,
        TurnFailure::LimitExceeded(
            super::event::LimitClass::AssistantTextBytes,
        ),
    );
}

#[test]
fn multibyte_cumulative_crossing_delta() {
    // 32 KiB of "a" plus multibyte deltas crossing the limit.
    let prefix = "a".repeat(32_768);
    let crossing = "é".repeat(16_384); // 32_768 bytes
    let outcome = collect(
        vec![text_event(&prefix), text_event(&crossing), text_event("é")],
        vec![user("hello")],
    );
    assert_failed(
        &outcome,
        TurnFailure::LimitExceeded(
            super::event::LimitClass::AssistantTextBytes,
        ),
    );
}

#[test]
fn duplicate_call_application_semantics() {
    let outcome = collect(
        vec![
            call_event("call-dup", "a.tool", json!({"first": true})),
            call_event("call-dup", "a.tool", json!({"again": true})),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (_, _, calls) = successful_turn(&outcome);
    assert_eq!(
        calls,
        &[
            TurnToolCall::Execute {
                call_id: "call-dup".to_owned(),
                tool_name: "a.tool".to_owned(),
                input: json!({"first": true}),
            },
            TurnToolCall::Invalid {
                call_id: "invalid-call-1".to_owned(),
                tool_name: "a.tool".to_owned(),
                message: "Duplicate tool call id: call-dup.".to_owned(),
            },
        ]
    );
}

#[test]
fn empty_id_and_name_application_semantics() {
    let outcome = collect(
        vec![
            call_event("", "a.tool", json!({})),
            call_event("c1", "", json!({})),
            call_event("c2", "a.tool", json!({})),
            completed_event(),
        ],
        vec![user("hello")],
    );
    let (_, _, calls) = successful_turn(&outcome);
    assert_eq!(
        calls,
        &[
            TurnToolCall::Invalid {
                call_id: "invalid-call-1".to_owned(),
                tool_name: "a.tool".to_owned(),
                message: "Provider emitted a tool call with an empty call id or tool name."
                    .to_owned(),
            },
            TurnToolCall::Invalid {
                call_id: "invalid-call-2".to_owned(),
                tool_name: "<empty>".to_owned(),
                message: "Provider emitted a tool call with an empty call id or tool name."
                    .to_owned(),
            },
            TurnToolCall::Execute {
                call_id: "c2".to_owned(),
                tool_name: "a.tool".to_owned(),
                input: json!({}),
            },
        ]
    );
}

#[test]
fn invalid_calls_consume_turn_bytes_like_valid_ones() {
    // Invalid calls are accounted like valid ones: a maximum-argument
    // valid call followed by a maximum-argument empty-name call exceeds
    // the aggregate before the empty-name handling would apply.
    let outcome = collect(
        vec![
            call_event("c1", "a.tool", payload_input(131_058)),
            call_event("c2", "", payload_input(131_058)),
        ],
        vec![user("hello")],
    );
    assert_failed(
        &outcome,
        TurnFailure::LimitExceeded(
            super::event::LimitClass::AggregateTurnBytes,
        ),
    );
}

#[test]
fn completion_is_required() {
    let outcome = collect(vec![text_event("partial")], vec![user("hello")]);
    assert_failed(&outcome, TurnFailure::EofWithoutCompletion);
    assert_eq!(
        failed_message(&outcome),
        "The provider stream ended without a completion event; the response was rejected."
    );
}

#[test]
fn tool_call_then_eof_is_failure() {
    let outcome = collect(
        vec![call_event("c1", "a.tool", json!({}))],
        vec![user("hello")],
    );
    assert_failed(&outcome, TurnFailure::EofWithoutCompletion);
}

#[test]
fn event_after_completion_fails() {
    let outcome = collect(
        vec![completed_event(), text_event("late")],
        vec![user("hello")],
    );
    assert_failed(&outcome, TurnFailure::EventAfterCompletion);
    assert_eq!(
        failed_message(&outcome),
        "The provider exceeded an event after completion limit; the response was rejected."
    );
}

#[test]
fn duplicate_completion_fails() {
    let outcome = collect(
        vec![completed_event(), completed_event()],
        vec![user("hello")],
    );
    assert_failed(&outcome, TurnFailure::EventAfterCompletion);
}

#[test]
fn unknown_event_after_completion_is_after_completion() {
    let outcome = collect(
        vec![
            completed_event(),
            ProviderEvent::Raw(json!({
                "type": "unexpected",
                "callId": "c1",
                "toolName": "a.tool",
                "input": {}
            })),
        ],
        vec![user("hello")],
    );
    assert_failed(&outcome, TurnFailure::EventAfterCompletion);
}

#[test]
fn cancellation_before_turn() {
    let provider = ScriptedProvider { events: vec![completed_event()] };
    let token = CancellationToken::new();
    token.cancel();
    let outcome =
        collect_provider_turn(&provider, &[user("hello")], &[], None, &token);
    assert_eq!(outcome, TurnOutcome::Cancelled);
}

#[test]
fn cancellation_before_first_event() {
    let outcome = collect_with_cancellation(vec![completed_event()], 0);
    assert_eq!(outcome, TurnOutcome::Cancelled);
}

#[test]
fn cancellation_during_turn() {
    let outcome = collect_with_cancellation(
        vec![text_event("one"), text_event("two"), completed_event()],
        1,
    );
    assert_eq!(outcome, TurnOutcome::Cancelled);
}

#[test]
fn cancellation_outranks_completion() {
    let outcome = collect_with_cancellation(
        vec![text_event("one"), completed_event()],
        1,
    );
    assert_eq!(outcome, TurnOutcome::Cancelled);
}

#[test]
fn cancellation_outranks_limit_failure() {
    // A token cancelled before collection wins even when the stream
    // would immediately exceed a bound.
    let provider =
        ScriptedProvider { events: vec![text_event(&"a".repeat(65_537))] };
    let token = CancellationToken::new();
    token.cancel();
    let outcome =
        collect_provider_turn(&provider, &[user("hello")], &[], None, &token);
    assert_eq!(outcome, TurnOutcome::Cancelled);
}

#[test]
fn provider_cancellation_event() {
    let outcome = collect(
        vec![ProviderEvent::Cancelled {
            message: "The fake provider was aborted.".to_owned(),
        }],
        vec![user("hello")],
    );
    assert_eq!(outcome, TurnOutcome::Cancelled);
}

#[test]
fn provider_failure_propagation() {
    let outcome = collect(
        vec![ProviderEvent::Failed("boom".to_owned())],
        vec![user("hello")],
    );
    assert_failed(&outcome, TurnFailure::ProviderFailed("boom".to_owned()));
    assert_eq!(failed_message(&outcome), "boom");
}

#[test]
fn provider_failure_after_partial_text() {
    let outcome = collect(
        vec![text_event("prefix-"), ProviderEvent::Failed("boom".to_owned())],
        vec![user("hello")],
    );
    assert_failed(&outcome, TurnFailure::ProviderFailed("boom".to_owned()));
}

#[test]
fn unknown_event_protocol_failure() {
    let outcome = collect(
        vec![ProviderEvent::Raw(json!({
            "type": "unexpected",
            "callId": "call-1",
            "toolName": "workspace.read",
            "input": {"path": "README.md"}
        }))],
        vec![user("hello")],
    );
    assert_failed(
        &outcome,
        TurnFailure::Protocol(super::event::ProtocolFailure::UnknownEventType),
    );
    assert_eq!(
        failed_message(&outcome),
        "The provider emitted an unknown event type; the response was rejected."
    );
    // No tool proposal may be retained.
    assert!(matches!(outcome, TurnOutcome::Failed { .. }));
}

#[test]
fn malformed_event_protocol_failures() {
    for raw in
        [Value::Null, json!([1, 2]), json!("text"), json!(42), json!(true)]
    {
        let outcome =
            collect(vec![ProviderEvent::Raw(raw)], vec![user("hello")]);
        assert_failed(
            &outcome,
            TurnFailure::Protocol(
                super::event::ProtocolFailure::MalformedEvent,
            ),
        );
        assert_eq!(
            failed_message(&outcome),
            "The provider emitted a malformed event; the response was rejected."
        );
    }
}

#[test]
fn malformed_text_event_protocol_failure() {
    let outcome = collect(
        vec![ProviderEvent::Raw(json!({"type": "text_delta", "text": 42}))],
        vec![user("hello")],
    );
    assert_failed(
        &outcome,
        TurnFailure::Protocol(
            super::event::ProtocolFailure::MalformedTextEvent,
        ),
    );
}

#[test]
fn malformed_tool_call_protocol_failures() {
    for raw in [
        json!({"type": "tool_call", "callId": 7, "toolName": "a.tool", "input": {}}),
        json!({"type": "tool_call", "callId": "c1", "toolName": 9, "input": {}}),
        json!({"type": "tool_call", "callId": null, "toolName": "a.tool"}),
    ] {
        let outcome =
            collect(vec![ProviderEvent::Raw(raw)], vec![user("hello")]);
        assert_failed(
            &outcome,
            TurnFailure::Protocol(
                super::event::ProtocolFailure::MalformedToolCall,
            ),
        );
    }
}

#[test]
fn missing_tool_input_is_json_validity_failure() {
    let outcome = collect(
        vec![ProviderEvent::Raw(json!({
            "type": "tool_call",
            "callId": "c1",
            "toolName": "a.tool"
        }))],
        vec![user("hello")],
    );
    assert_failed(
        &outcome,
        TurnFailure::Protocol(
            super::event::ProtocolFailure::InvalidToolArgumentJson,
        ),
    );
    assert_eq!(
        failed_message(&outcome),
        "The provider exceeded the tool-argument JSON validity limit; the response was rejected."
    );
}

#[test]
fn raw_event_validates_like_typed_event() {
    let outcome = collect(
        vec![
            ProviderEvent::Raw(json!({"type": "text_delta", "text": "hi"})),
            ProviderEvent::Raw(
                json!({"type": "tool_call", "callId": "c1", "toolName": "a.tool", "input": {"x": 1}}),
            ),
            ProviderEvent::Raw(
                json!({"type": "completed", "extra": "ignored"}),
            ),
        ],
        vec![user("hello")],
    );
    let (text, _, calls) = successful_turn(&outcome);
    assert_eq!(text, "hi");
    assert_eq!(calls.len(), 1);
}

#[test]
fn validator_extra_fields_on_completed_are_irrelevant() {
    assert_eq!(
        validate_external_event(&json!({"type": "completed", "anything": 1})),
        Ok(ModelEvent::Completed)
    );
}

#[test]
fn validator_unknown_discriminator_fails_closed() {
    assert_eq!(
        validate_external_event(&json!({
            "type": "unexpected",
            "callId": "call-1",
            "toolName": "workspace.read",
            "input": {"path": "README.md"}
        })),
        Err(super::event::ProtocolFailure::UnknownEventType)
    );
    assert_eq!(
        validate_external_event(&json!({"type": 7})),
        Err(super::event::ProtocolFailure::UnknownEventType)
    );
}

#[test]
fn invalid_transcript_blocks_provider_use() {
    let outcome = collect(
        vec![completed_event()],
        vec![
            user("hello"),
            ConversationItem::AssistantToolCall {
                call_id: "c1".to_owned(),
                tool_name: "a.tool".to_owned(),
                input: json!({}),
            },
            ConversationItem::ToolResult {
                call_id: "c1".to_owned(),
                tool_name: "a.tool".to_owned(),
                result: ToolExecutionResult::Success {
                    output: json!({"ok": true}),
                    summary: "ok".to_owned(),
                },
            },
            ConversationItem::AssistantToolCall {
                call_id: "c2".to_owned(),
                tool_name: "b.tool".to_owned(),
                input: json!({}),
            },
            user("next"),
        ],
    );
    assert_failed(
        &outcome,
        TurnFailure::InvalidTranscript(
            "Tool call c2 (b.tool) has no result before the next user message."
                .to_owned(),
        ),
    );
    assert_eq!(
        failed_message(&outcome),
        "The conversation transcript is structurally invalid; the provider request was blocked: Tool call c2 (b.tool) has no result before the next user message."
    );
}

#[test]
fn transcript_valid_pairing() {
    let items = vec![
        user("hello"),
        ConversationItem::AssistantToolCall {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned(),
            input: json!({}),
        },
        ConversationItem::ToolResult {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned(),
            result: ToolExecutionResult::Success {
                output: json!({"ok": true}),
                summary: "ok".to_owned(),
            },
        },
        ConversationItem::AssistantMessage { content: "done".to_owned() },
        user("next"),
    ];
    assert_eq!(validate_conversation_items(&items), Ok(()));
}

#[test]
fn transcript_orphan_result() {
    let items = vec![
        user("hello"),
        ConversationItem::ToolResult {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned(),
            result: ToolExecutionResult::Failed {
                message: "no call".to_owned(),
            },
        },
    ];
    assert_eq!(
        validate_conversation_items(&items),
        Err(TranscriptFailure::OrphanResult { call_id: "c1".to_owned() })
    );
    assert_eq!(
        validate_conversation_items(&items).unwrap_err().message(),
        "Tool result for c1 has no recorded call."
    );
}

#[test]
fn transcript_duplicate_call_id() {
    let items = vec![
        user("hello"),
        ConversationItem::AssistantToolCall {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned(),
            input: json!({}),
        },
        ConversationItem::AssistantToolCall {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned(),
            input: json!({}),
        },
    ];
    assert_eq!(
        validate_conversation_items(&items),
        Err(TranscriptFailure::DuplicateCallId { call_id: "c1".to_owned() })
    );
}

#[test]
fn transcript_duplicate_result_reported_as_orphan() {
    // The reference deletes a call from its pending set once resolved, so
    // a second result for the same id is an orphan, not a duplicate.
    let items = vec![
        user("hello"),
        ConversationItem::AssistantToolCall {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned(),
            input: json!({}),
        },
        ConversationItem::ToolResult {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned(),
            result: ToolExecutionResult::Success {
                output: json!({}),
                summary: "ok".to_owned(),
            },
        },
        ConversationItem::ToolResult {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned(),
            result: ToolExecutionResult::Success {
                output: json!({}),
                summary: "again".to_owned(),
            },
        },
    ];
    assert_eq!(
        validate_conversation_items(&items),
        Err(TranscriptFailure::OrphanResult { call_id: "c1".to_owned() })
    );
}

#[test]
fn transcript_empty_call_id() {
    let items = vec![
        user("hello"),
        ConversationItem::AssistantToolCall {
            call_id: "".to_owned(),
            tool_name: "a.tool".to_owned(),
            input: json!({}),
        },
    ];
    assert_eq!(
        validate_conversation_items(&items),
        Err(TranscriptFailure::EmptyCallId)
    );
}

#[test]
fn transcript_unresolved_before_user() {
    let items = vec![
        user("hello"),
        ConversationItem::AssistantToolCall {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned(),
            input: json!({}),
        },
        user("next"),
    ];
    assert_eq!(
        validate_conversation_items(&items),
        Err(TranscriptFailure::UnresolvedBeforeUser {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned()
        })
    );
}

#[test]
fn transcript_unresolved_at_end() {
    let items = vec![
        user("hello"),
        ConversationItem::AssistantToolCall {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned(),
            input: json!({}),
        },
    ];
    assert_eq!(
        validate_conversation_items(&items),
        Err(TranscriptFailure::UnresolvedAtEnd {
            call_id: "c1".to_owned(),
            tool_name: "a.tool".to_owned()
        })
    );
}

#[test]
fn transcript_first_unresolved_failure_is_deterministic() {
    // The reference reports the first-inserted pending call.
    let items = vec![
        user("hello"),
        ConversationItem::AssistantToolCall {
            call_id: "first".to_owned(),
            tool_name: "a.tool".to_owned(),
            input: json!({}),
        },
        ConversationItem::AssistantToolCall {
            call_id: "second".to_owned(),
            tool_name: "b.tool".to_owned(),
            input: json!({}),
        },
        user("next"),
    ];
    assert_eq!(
        validate_conversation_items(&items),
        Err(TranscriptFailure::UnresolvedBeforeUser {
            call_id: "first".to_owned(),
            tool_name: "a.tool".to_owned()
        })
    );
}

fn detached(
    value: Value,
    max_bytes: usize,
) -> Result<(ToolExecutionResult, usize), DetachFailure> {
    detach_bounded_tool_result(&value, max_bytes)
}

#[test]
fn detach_valid_success_drops_unknown_fields() {
    let (result, bytes) = detached(
        json!({"status": "success", "output": {"ok": true}, "summary": "ok", "unknown": "discarded"}),
        1024,
    )
    .expect("valid success");
    assert_eq!(
        result,
        ToolExecutionResult::Success {
            output: json!({"ok": true}),
            summary: "ok".to_owned(),
        }
    );
    assert_eq!(
        bytes,
        "{\"status\":\"success\",\"output\":{\"ok\":true},\"summary\":\"ok\",\"unknown\":\"discarded\"}"
            .len()
    );
}

#[test]
fn detach_every_allowed_failure_status() {
    let cases = [
        ("invalid_input", "bad input"),
        ("denied", "denied"),
        ("conflict", "conflict"),
        ("failed", "failed"),
        ("cancelled", "cancelled"),
        ("timed_out", "timed out"),
        ("output_limit", "output limit"),
        ("sandbox_denied", "sandbox denied"),
        ("sandbox_unavailable", "sandbox unavailable"),
        ("workspace_violation", "workspace violation"),
        ("unavailable", "unavailable"),
    ];
    for (status, message) in cases {
        let (result, _) =
            detached(json!({"status": status, "message": message}), 1024)
                .unwrap_or_else(|failure| panic!("{status}: {failure:?}"));
        assert_eq!(result.status_str(), status);
        assert_eq!(result.message(), message);
    }
}

#[test]
fn detach_non_object_shapes_rejected() {
    for value in [
        json!("plain text"),
        json!([1, 2]),
        Value::Null,
        json!(42),
        json!(true),
    ] {
        assert_eq!(detached(value, 1024), Err(DetachFailure::InvalidShape));
    }
}

#[test]
fn detach_invalid_success_rejected() {
    assert_eq!(
        detached(json!({"status": "success", "summary": "ok"}), 1024),
        Err(DetachFailure::InvalidSuccess)
    );
    assert_eq!(
        detached(json!({"status": "success", "output": 1}), 1024),
        Err(DetachFailure::InvalidSuccess)
    );
    assert_eq!(
        detached(
            json!({"status": "success", "output": 1, "summary": 42}),
            1024
        ),
        Err(DetachFailure::InvalidSuccess)
    );
}

#[test]
fn detach_invalid_failure_rejected() {
    assert_eq!(
        detached(json!({"status": "failed"}), 1024),
        Err(DetachFailure::InvalidFailure)
    );
    assert_eq!(
        detached(json!({"status": "failed", "message": 42}), 1024),
        Err(DetachFailure::InvalidFailure)
    );
}

#[test]
fn detach_unknown_status_rejected() {
    assert_eq!(
        detached(json!({"status": "invented", "message": "bad"}), 1024),
        Err(DetachFailure::UnknownStatus)
    );
    assert_eq!(
        detached(json!({"status": 7}), 1024),
        Err(DetachFailure::UnknownStatus)
    );
}

#[test]
fn detach_oversize_rejected() {
    let value = json!({"status": "success", "output": {"text": "x".repeat(100)}, "summary": "large"});
    assert_eq!(
        detached(value, 32),
        Err(DetachFailure::Oversize { max_bytes: 32 })
    );
    assert_eq!(
        DetachFailure::Oversize { max_bytes: 32 }.message("test.tool"),
        "test.tool returned a tool result exceeding the 32-byte limit."
    );
}

#[test]
fn detach_exact_size_accepted() {
    let value = json!({"status": "success", "output": 1, "summary": "s"});
    let serialized = serde_json::to_string(&value).unwrap();
    let (_, bytes) = detached(value, serialized.len()).expect("exact size");
    assert_eq!(bytes, serialized.len());
}

#[test]
fn detach_messages_match_the_reference() {
    assert_eq!(
        DetachFailure::NonJson.message("test.tool"),
        "test.tool returned a non-JSON tool result."
    );
    assert_eq!(
        DetachFailure::InvalidShape.message("test.tool"),
        "test.tool returned an invalid tool-result shape."
    );
    assert_eq!(
        DetachFailure::UnknownStatus.message("test.tool"),
        "test.tool returned an unknown tool-result status."
    );
    assert_eq!(
        DetachFailure::InvalidSuccess.message("test.tool"),
        "test.tool returned an invalid success tool result."
    );
    assert_eq!(
        DetachFailure::InvalidFailure.message("test.tool"),
        "test.tool returned an invalid failure tool result."
    );
}

#[test]
fn detach_retained_result_is_owned() {
    let mut input = json!({"status": "success", "output": {"value": "original"}, "summary": "ok"});
    let (result, _) = detached(input.clone(), 1024).expect("valid success");
    input["output"]["value"] = json!("mutated-after-detach");
    match result {
        ToolExecutionResult::Success { output, .. } => {
            assert_eq!(output, json!({"value": "original"}));
        }
        other => panic!("unexpected result {other:?}"),
    }
}

#[test]
fn failed_turn_never_commits_partial_text() {
    let provider = ScriptedProvider {
        events: vec![text_event("prefix-"), text_event(&"x".repeat(65_537))],
    };
    let token = CancellationToken::new();
    let outcome =
        collect_provider_turn(&provider, &[user("hello")], &[], None, &token);
    assert!(matches!(outcome, TurnOutcome::Failed { .. }));
}
