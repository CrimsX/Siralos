//! Adapter tests for the R7.1 deterministic fake provider and the
//! strict bounded-turn collector.

use serde_json::{Value, json};
use siralos_core::provider::{
    CancellationSignal, CancellationToken, ConversationItem, ModelEvent,
    ModelProvider, ModelRequest, ProviderEvent, ToolDefinition,
    ToolExecutionResult, validate_external_event,
};

use super::deterministic_fake::{
    DETERMINISTIC_FAKE_PROVIDER_ID, DeterministicFakeProvider,
};
use super::strict_turn::{
    BoundedModelTurnLimits, BoundedModelTurnOutcome,
    collect_bounded_model_turn,
};

fn list_tool() -> ToolDefinition {
    ToolDefinition {
        name: "workspace.list".to_owned(),
        description: "List one directory within the approved workspace."
            .to_owned(),
        input_schema: json!({}),
    }
}

fn read_tool() -> ToolDefinition {
    ToolDefinition {
        name: "workspace.read".to_owned(),
        description:
            "Read a bounded range from one text file inside the workspace."
                .to_owned(),
        input_schema: json!({}),
    }
}

fn search_tool() -> ToolDefinition {
    ToolDefinition {
        name: "workspace.search".to_owned(),
        description: "Search text files recursively within a bounded workspace directory."
            .to_owned(),
        input_schema: json!({}),
    }
}

fn user(content: &str) -> ConversationItem {
    ConversationItem::UserMessage { content: content.to_owned() }
}

fn assistant_call(
    call_id: &str,
    tool_name: &str,
    input: Value,
) -> ConversationItem {
    ConversationItem::AssistantToolCall {
        call_id: call_id.to_owned(),
        tool_name: tool_name.to_owned(),
        input,
    }
}

fn tool_result(
    call_id: &str,
    tool_name: &str,
    result: ToolExecutionResult,
) -> ConversationItem {
    ConversationItem::ToolResult {
        call_id: call_id.to_owned(),
        tool_name: tool_name.to_owned(),
        result,
    }
}

/// Pull a full fake-provider stream into typed events, stopping at the
/// first cancellation/provider failure.
fn collect_fake(
    request: &ModelRequest,
    token: &CancellationToken,
) -> Vec<ModelEvent> {
    let provider = DeterministicFakeProvider::new();
    let mut stream = provider.stream(request, token.signal());
    let mut events = Vec::new();
    loop {
        match stream.next() {
            None => break,
            Some(ProviderEvent::Event(event)) => events.push(event),
            Some(ProviderEvent::Cancelled { .. })
            | Some(ProviderEvent::Failed(_)) => break,
            Some(ProviderEvent::Raw(_)) => {
                unreachable!("fake emits typed events")
            }
        }
    }
    events
}

fn text_of(events: &[ModelEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn first_tool_call(events: &[ModelEvent]) -> Option<&ModelEvent> {
    events.iter().find(|event| matches!(event, ModelEvent::ToolCall { .. }))
}

#[test]
fn provider_id_is_deterministic_fake() {
    let provider = DeterministicFakeProvider::new();
    assert_eq!(provider.id(), DETERMINISTIC_FAKE_PROVIDER_ID);
    assert_eq!(provider.id(), "deterministic-fake");
}

#[test]
fn default_echo_with_chunks_and_completion() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user("hello")],
        tools: vec![],
        system: None,
    };
    let events = collect_fake(&request, &token);
    assert_eq!(text_of(&events), "Siralos received: hello");
    assert_eq!(events.last(), Some(&ModelEvent::Completed));
    let chunks = events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(chunks, vec!["Siralos received", ": hello"]);
}

#[test]
fn no_latest_user_prompt_echoes_empty() {
    let token = CancellationToken::new();
    let request =
        ModelRequest { messages: vec![], tools: vec![], system: None };
    let events = collect_fake(&request, &token);
    assert_eq!(text_of(&events), "Siralos received: ");
}

#[test]
fn latest_user_prompt_is_selected() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user("first"), user("second")],
        tools: vec![],
        system: None,
    };
    let events = collect_fake(&request, &token);
    assert_eq!(text_of(&events), "Siralos received: second");
}

#[test]
fn sixteen_code_point_chunking() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user(
            "a b c d e f g h i j k l m n o p q r s t u v w x y z",
        )],
        tools: vec![],
        system: None,
    };
    let events = collect_fake(&request, &token);
    let chunks = events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for chunk in &chunks {
        assert!(chunk.chars().count() <= 16);
    }
    assert!(chunks.len() > 1);
    assert_eq!(
        chunks.concat(),
        "Siralos received: a b c d e f g h i j k l m n o p q r s t u v w x y z"
    );
}

#[test]
fn unicode_scalar_values_never_split() {
    let text = "step 1 \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} step 2 \u{1f680} done";
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user(text)],
        tools: vec![],
        system: None,
    };
    let events = collect_fake(&request, &token);
    let chunks = events
        .iter()
        .filter_map(|event| match event {
            ModelEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    // Every chunk must be valid UTF-8 by construction; recombining
    // reproduces the exact response.
    assert_eq!(chunks.concat(), format!("Siralos received: {text}"));
}

#[test]
fn identical_request_produces_identical_sequence() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user("caf\u{e9} \u{e9}tude \u{1f600}")],
        tools: vec![],
        system: None,
    };
    let first = collect_fake(&request, &token);
    let second = collect_fake(&request, &token);
    assert_eq!(first, second);
}

#[test]
fn workspace_list_scenario() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user("list files")],
        tools: vec![list_tool()],
        system: None,
    };
    let events = collect_fake(&request, &token);
    assert_eq!(
        first_tool_call(&events),
        Some(&ModelEvent::ToolCall {
            call_id: "call-1".to_owned(),
            tool_name: "workspace.list".to_owned(),
            input: json!({"path": "."}),
        })
    );
    assert_eq!(events.last(), Some(&ModelEvent::Completed));
}

#[test]
fn workspace_list_final_response_after_result() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![
            user("list files"),
            assistant_call("call-1", "workspace.list", json!({"path": "."})),
            tool_result(
                "call-1",
                "workspace.list",
                ToolExecutionResult::Success {
                    output: json!({
                        "path": ".",
                        "entries": [{"name": "a.txt", "path": "a.txt", "type": "file", "size": 1}],
                        "truncated": false
                    }),
                    summary: "1 entries".to_owned(),
                },
            ),
        ],
        tools: vec![list_tool()],
        system: None,
    };
    let events = collect_fake(&request, &token);
    assert_eq!(text_of(&events), "Siralos inspected 1 workspace entries.");
}

#[test]
fn workspace_read_scenario() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user("read README.md")],
        tools: vec![read_tool()],
        system: None,
    };
    let events = collect_fake(&request, &token);
    assert_eq!(
        first_tool_call(&events),
        Some(&ModelEvent::ToolCall {
            call_id: "call-1".to_owned(),
            tool_name: "workspace.read".to_owned(),
            input: json!({"path": "README.md"}),
        })
    );
}

#[test]
fn workspace_search_scenario() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user("search modular monolith")],
        tools: vec![search_tool()],
        system: None,
    };
    let events = collect_fake(&request, &token);
    assert_eq!(
        first_tool_call(&events),
        Some(&ModelEvent::ToolCall {
            call_id: "call-1".to_owned(),
            tool_name: "workspace.search".to_owned(),
            input: json!({"query": "modular monolith", "path": "."}),
        })
    );
}

#[test]
fn search_scenario_trims_query() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user("search   spaced query  ")],
        tools: vec![search_tool()],
        system: None,
    };
    let events = collect_fake(&request, &token);
    match first_tool_call(&events) {
        Some(ModelEvent::ToolCall { input, .. }) => {
            assert_eq!(input, &json!({"query": "spaced query", "path": "."}));
        }
        other => panic!("expected a search tool call, got {other:?}"),
    }
}

#[test]
fn tool_absent_falls_back_to_text() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user("list files")],
        tools: vec![read_tool()],
        system: None,
    };
    let events = collect_fake(&request, &token);
    assert!(first_tool_call(&events).is_none());
    assert_eq!(text_of(&events), "Siralos received: list files");
}

#[test]
fn failed_tool_result_reported_truthfully() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![
            user("list files"),
            assistant_call("call-1", "workspace.list", json!({"path": "."})),
            tool_result(
                "call-1",
                "workspace.list",
                ToolExecutionResult::Denied {
                    message: "Path is outside the Siralos workspace."
                        .to_owned(),
                },
            ),
        ],
        tools: vec![list_tool()],
        system: None,
    };
    let events = collect_fake(&request, &token);
    assert_eq!(
        text_of(&events),
        "Siralos could not complete the workspace operation: Path is outside the Siralos workspace."
    );
}

#[test]
fn previous_turn_result_is_not_reused() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![
            user("list files"),
            assistant_call("call-1", "workspace.list", json!({"path": "."})),
            tool_result(
                "call-1",
                "workspace.list",
                ToolExecutionResult::Success {
                    output: json!({"path": ".", "entries": [], "truncated": false}),
                    summary: "0 entries".to_owned(),
                },
            ),
            user("read README.md"),
        ],
        tools: vec![list_tool(), read_tool()],
        system: None,
    };
    let events = collect_fake(&request, &token);
    assert_eq!(
        first_tool_call(&events),
        Some(&ModelEvent::ToolCall {
            call_id: "call-1".to_owned(),
            tool_name: "workspace.read".to_owned(),
            input: json!({"path": "README.md"}),
        })
    );
}

#[test]
fn cancellation_before_first_event() {
    let token = CancellationToken::new();
    token.cancel();
    let request = ModelRequest {
        messages: vec![user("hello")],
        tools: vec![],
        system: None,
    };
    let events = collect_fake(&request, &token);
    assert!(events.is_empty());
}

#[test]
fn cancellation_between_events_prevents_completion() {
    let token = CancellationToken::new();
    let request = ModelRequest {
        messages: vec![user("hello")],
        tools: vec![],
        system: None,
    };
    let provider = DeterministicFakeProvider::new();
    let mut stream = provider.stream(&request, token.signal());
    let mut events = Vec::new();
    let mut cancelled = false;
    loop {
        match stream.next() {
            None => break,
            Some(ProviderEvent::Event(event)) => {
                events.push(event);
                token.cancel();
            }
            Some(ProviderEvent::Cancelled { .. }) => {
                cancelled = true;
                break;
            }
            Some(ProviderEvent::Failed(_)) | Some(ProviderEvent::Raw(_)) => {
                break;
            }
        }
    }
    assert!(cancelled);
    assert!(!events.contains(&ModelEvent::Completed));
    assert!(!events.is_empty());
}

// ---------------------------------------------------------------------------
// Strict bounded-turn collector.
// ---------------------------------------------------------------------------

const STRICT_LIMITS: BoundedModelTurnLimits = BoundedModelTurnLimits {
    max_text_bytes: 32,
    max_text_events: 2,
    max_tool_calls: 2,
    max_tool_name_bytes: 16,
    max_call_id_bytes: 16,
    max_tool_argument_bytes: 32,
    max_turn_bytes: 64,
};

fn strict_events(events: Vec<ProviderEvent>) -> BoundedModelTurnOutcome {
    strict_events_with_limits(events, &STRICT_LIMITS)
}

fn strict_events_with_limits(
    events: Vec<ProviderEvent>,
    limits: &BoundedModelTurnLimits,
) -> BoundedModelTurnOutcome {
    let provider = StrictProvider { events };
    let token = CancellationToken::new();
    collect_bounded_model_turn(
        "The test actor",
        &provider,
        &[],
        &[],
        &token,
        limits,
        None,
    )
}

/// A scripted provider emitting typed events for the strict collector.
struct StrictProvider {
    events: Vec<ProviderEvent>,
}

impl ModelProvider for StrictProvider {
    type Stream<'a>
        = std::vec::IntoIter<ProviderEvent>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "bounded-turn-test"
    }

    fn stream<'a>(
        &'a self,
        _request: &'a ModelRequest,
        _cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        self.events.clone().into_iter()
    }
}

fn strict_text(text: &str) -> ProviderEvent {
    ProviderEvent::Event(ModelEvent::TextDelta { text: text.to_owned() })
}

fn strict_call(call_id: &str, tool_name: &str, input: Value) -> ProviderEvent {
    ProviderEvent::Event(ModelEvent::ToolCall {
        call_id: call_id.to_owned(),
        tool_name: tool_name.to_owned(),
        input,
    })
}

fn strict_completed() -> ProviderEvent {
    ProviderEvent::Event(ModelEvent::Completed)
}

fn expect_strict_failed(outcome: &BoundedModelTurnOutcome, contains: &str) {
    match outcome {
        BoundedModelTurnOutcome::Failed { message } => {
            assert!(
                message.contains(contains),
                "message {message:?} does not contain {contains:?}"
            );
        }
        other => panic!("expected a strict failure, got {other:?}"),
    }
}

#[test]
fn strict_accepts_completed_text_and_tool_turn() {
    let mut input = json!({"path": "a"});
    let outcome = strict_events(vec![
        strict_text("inspect"),
        strict_call("call-1", "read", input.clone()),
        strict_completed(),
    ]);
    match outcome {
        BoundedModelTurnOutcome::Turn { text, tool_calls } => {
            assert_eq!(text, "inspect");
            assert_eq!(tool_calls.len(), 1);
            input["path"] = json!("mutated-after-validation");
            assert_eq!(tool_calls[0].input, json!({"path": "a"}));
        }
        other => panic!("expected a strict turn, got {other:?}"),
    }
}

#[test]
fn strict_requires_exactly_one_completion() {
    expect_strict_failed(
        &strict_events(vec![strict_text("unterminated")]),
        "without a completion event",
    );
    expect_strict_failed(
        &strict_events(vec![strict_completed(), strict_text("late")]),
        "after completion",
    );
}

#[test]
fn strict_bounds_text_bytes_and_event_count_cumulatively() {
    let limits = BoundedModelTurnLimits { max_text_bytes: 5, ..STRICT_LIMITS };
    expect_strict_failed(
        &strict_events_with_limits(
            vec![
                strict_text("\u{754c}"),
                strict_text("\u{754c}"),
                strict_completed(),
            ],
            &limits,
        ),
        "byte limit",
    );
    let limits =
        BoundedModelTurnLimits { max_text_events: 1, ..STRICT_LIMITS };
    expect_strict_failed(
        &strict_events_with_limits(
            vec![strict_text("a"), strict_text("b"), strict_completed()],
            &limits,
        ),
        "text-event",
    );
}

#[test]
fn strict_bounds_ids_names_arguments_count_and_aggregate() {
    let cases: Vec<(Vec<ProviderEvent>, BoundedModelTurnLimits, &str)> = vec![
        (
            vec![
                strict_call("long-id", "read", json!({})),
                strict_completed(),
            ],
            BoundedModelTurnLimits { max_call_id_bytes: 3, ..STRICT_LIMITS },
            "id byte limit",
        ),
        (
            vec![strict_call("c", "long-name", json!({})), strict_completed()],
            BoundedModelTurnLimits { max_tool_name_bytes: 3, ..STRICT_LIMITS },
            "tool-name byte limit",
        ),
        (
            vec![
                strict_call("c", "read", json!({"value": "large"})),
                strict_completed(),
            ],
            BoundedModelTurnLimits {
                max_tool_argument_bytes: 5,
                ..STRICT_LIMITS
            },
            "tool-argument byte limit",
        ),
        (
            vec![
                strict_call("c1", "read", json!({})),
                strict_call("c2", "read", json!({})),
                strict_completed(),
            ],
            BoundedModelTurnLimits { max_tool_calls: 1, ..STRICT_LIMITS },
            "tool-call limit",
        ),
        (
            vec![
                strict_text("1234"),
                strict_call("c", "read", json!({})),
                strict_completed(),
            ],
            BoundedModelTurnLimits { max_turn_bytes: 5, ..STRICT_LIMITS },
            "aggregate turn byte limit",
        ),
    ];
    for (events, limits, fragment) in cases {
        expect_strict_failed(
            &strict_events_with_limits(events, &limits),
            fragment,
        );
    }
}

#[test]
fn strict_rejects_unknown_discriminator_even_tool_call_shaped() {
    expect_strict_failed(
        &strict_events(vec![ProviderEvent::Raw(json!({
            "type": "unexpected",
            "callId": "call-1",
            "toolName": "read",
            "input": {"path": "a"}
        }))]),
        "unknown event type",
    );
}

#[test]
fn strict_still_accepts_valid_tool_call_after_hardening() {
    let outcome = strict_events(vec![
        strict_call("call-ok", "read", json!({})),
        strict_completed(),
    ]);
    assert!(matches!(outcome, BoundedModelTurnOutcome::Turn { .. }));
}

#[test]
fn strict_rejects_duplicate_call_id() {
    expect_strict_failed(
        &strict_events(vec![
            strict_call("call-dupe", "read", json!({})),
            strict_call("call-dupe", "read", json!({})),
            strict_completed(),
        ]),
        "duplicate",
    );
}

#[test]
fn strict_rejects_empty_ids_and_names() {
    expect_strict_failed(
        &strict_events(vec![
            strict_call("", "read", json!({})),
            strict_completed(),
        ]),
        "empty id or name",
    );
    expect_strict_failed(
        &strict_events(vec![
            strict_call("call-1", "", json!({})),
            strict_completed(),
        ]),
        "empty id or name",
    );
}

#[test]
fn strict_rejects_non_string_text_payload() {
    expect_strict_failed(
        &strict_events(vec![ProviderEvent::Raw(json!({
            "type": "text_delta",
            "text": 42
        }))]),
        "text event without a string payload",
    );
}

#[test]
fn strict_rejects_non_string_id_or_name() {
    expect_strict_failed(
        &strict_events(vec![ProviderEvent::Raw(json!({
            "type": "tool_call",
            "callId": 7,
            "toolName": "read",
            "input": {}
        }))]),
        "non-string id or name",
    );
    expect_strict_failed(
        &strict_events(vec![ProviderEvent::Raw(json!({
            "type": "tool_call",
            "callId": "call-1",
            "toolName": 9,
            "input": {}
        }))]),
        "non-string id or name",
    );
}

#[test]
fn strict_rejects_malformed_non_object_events() {
    expect_strict_failed(
        &strict_events(vec![ProviderEvent::Raw(Value::Null)]),
        "malformed event",
    );
    expect_strict_failed(
        &strict_events(vec![ProviderEvent::Raw(json!(5))]),
        "malformed event",
    );
}

#[test]
fn strict_reports_after_completion_for_unknown_event() {
    expect_strict_failed(
        &strict_events(vec![
            strict_completed(),
            ProviderEvent::Raw(json!({
                "type": "unexpected",
                "callId": "c1",
                "toolName": "read",
                "input": {}
            })),
        ]),
        "after completion",
    );
}

#[test]
fn strict_missing_input_is_not_json_serializable() {
    expect_strict_failed(
        &strict_events(vec![ProviderEvent::Raw(json!({
            "type": "tool_call",
            "callId": "call-1",
            "toolName": "read"
        }))]),
        "not JSON-serializable",
    );
}

#[test]
fn strict_rejects_call_id_reuse_across_turns() {
    let mut seen = std::collections::BTreeSet::new();
    let provider = StrictProvider {
        events: vec![
            strict_call("call-reused", "read", json!({})),
            strict_completed(),
        ],
    };
    let token = CancellationToken::new();
    let first = collect_bounded_model_turn(
        "The test actor",
        &provider,
        &[],
        &[],
        &token,
        &STRICT_LIMITS,
        Some(&mut seen),
    );
    assert!(matches!(first, BoundedModelTurnOutcome::Turn { .. }));

    let provider = StrictProvider {
        events: vec![
            strict_call("call-reused", "read", json!({})),
            strict_completed(),
        ],
    };
    let second = collect_bounded_model_turn(
        "The test actor",
        &provider,
        &[],
        &[],
        &token,
        &STRICT_LIMITS,
        Some(&mut seen),
    );
    expect_strict_failed(&second, "duplicate");
}

#[test]
fn strict_aborts_when_token_is_cancelled() {
    let token = CancellationToken::new();
    token.cancel();
    let provider = StrictProvider { events: vec![strict_completed()] };
    let outcome = collect_bounded_model_turn(
        "The test actor",
        &provider,
        &[],
        &[],
        &token,
        &STRICT_LIMITS,
        None,
    );
    assert_eq!(outcome, BoundedModelTurnOutcome::Aborted);
}

#[test]
fn strict_provider_failure_is_actor_qualified() {
    let outcome =
        strict_events(vec![ProviderEvent::Failed("boom".to_owned())]);
    expect_strict_failed(&outcome, "The test actor provider failed: boom");
}

#[test]
fn strict_external_validation_uses_the_production_validator() {
    assert_eq!(
        validate_external_event(&json!({"type": "completed", "x": 1})),
        Ok(ModelEvent::Completed)
    );
}
