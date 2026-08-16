//! Focused Core Tool-loop tests (Stage 3R R7.2).
//!
//! These tests exercise the production registry, round, session, and
//! permission machinery through the pull-based application boundary.
//! Exact message/order assertions are intentional: the frozen contract
//! is observable behavior, not `is_ok()` shape.

use std::cell::Cell;
use std::rc::Rc;

use serde_json::{Value, json};

use crate::provider::{
    ConversationItem, ModelEvent, ModelProvider, ModelRequest, ProviderEvent,
    ToolCallInput, ToolDefinition, ToolExecutionResult,
};
use crate::tool::capability::CapabilityId;
use crate::tool::events::ToolLoopEvent;
use crate::tool::permission::{PermissionPolicy, PermissionRule, PolicyRule};
use crate::tool::registry::{ApprovedToolSurface, Tool, ToolRegistry};
use crate::tool::session::SiralosApplication;

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).unwrap()
}

fn static_workspace_read() -> &'static CapabilityId {
    static CAPABILITY: std::sync::OnceLock<CapabilityId> =
        std::sync::OnceLock::new();
    CAPABILITY.get_or_init(|| capability("workspace.read"))
}

#[derive(Clone)]
struct FixedTool {
    name: String,
    result: ToolExecutionResult,
    calls: Rc<Cell<usize>>,
}

impl FixedTool {
    fn new(name: &str, result: ToolExecutionResult) -> Self {
        Self { name: name.to_owned(), result, calls: Rc::new(Cell::new(0)) }
    }
}

impl Tool for FixedTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: format!("stub {}", self.name),
            input_schema: json!({}),
        }
    }

    fn capability(&self) -> &CapabilityId {
        static_workspace_read()
    }

    fn execute(
        &self,
        _input: &Value,
        _cancellation: crate::provider::CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        self.calls.set(self.calls.get() + 1);
        self.result.clone()
    }
}

struct EntryValidatingTool {
    entered: Rc<Cell<bool>>,
    worked: Rc<Cell<bool>>,
}

impl Tool for EntryValidatingTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "strict.tool".to_owned(),
            description: "strict stub".to_owned(),
            input_schema: json!({}),
        }
    }

    fn capability(&self) -> &CapabilityId {
        static_workspace_read()
    }

    fn execute(
        &self,
        input: &Value,
        _cancellation: crate::provider::CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        self.entered.set(true);
        if !input.is_object() {
            return ToolExecutionResult::InvalidInput {
                message: "Input must be an object.".to_owned(),
            };
        }
        self.worked.set(true);
        ToolExecutionResult::Success {
            output: json!({ "ok": true }),
            summary: "ok".to_owned(),
        }
    }
}

struct TurnScriptProvider {
    turns: Vec<Vec<ProviderEvent>>,
    next: Cell<usize>,
    requests: Rc<Cell<usize>>,
}

impl TurnScriptProvider {
    fn new(turns: Vec<Vec<ProviderEvent>>) -> Self {
        Self { turns, next: Cell::new(0), requests: Rc::new(Cell::new(0)) }
    }

    fn request_count(&self) -> usize {
        self.requests.get()
    }
}

impl ModelProvider for TurnScriptProvider {
    type Stream<'a>
        = std::vec::IntoIter<ProviderEvent>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "turn-script"
    }

    fn stream<'a>(
        &'a self,
        _request: &'a ModelRequest,
        _cancellation: crate::provider::CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        self.requests.set(self.requests.get() + 1);
        let index = self.next.get().min(self.turns.len().saturating_sub(1));
        self.next.set(index + 1);
        self.turns[index].clone().into_iter()
    }
}

fn completed() -> ProviderEvent {
    ProviderEvent::Event(ModelEvent::Completed)
}

fn text(value: &str) -> ProviderEvent {
    ProviderEvent::Event(ModelEvent::TextDelta { text: value.to_owned() })
}

fn tool_call(call_id: &str, tool_name: &str, input: Value) -> ProviderEvent {
    ProviderEvent::Event(ModelEvent::ToolCall {
        call_id: call_id.to_owned(),
        tool_name: tool_name.to_owned(),
        input: ToolCallInput::from_value(input),
    })
}

fn ordered_tool_call(
    call_id: &str,
    tool_name: &str,
    input: Value,
    ordered_json: &str,
) -> ProviderEvent {
    ProviderEvent::Event(ModelEvent::ToolCall {
        call_id: call_id.to_owned(),
        tool_name: tool_name.to_owned(),
        input: ToolCallInput::from_ordered_json(
            input,
            ordered_json.to_owned(),
        ),
    })
}

fn registry(tools: Vec<Box<dyn Tool>>) -> ToolRegistry {
    ToolRegistry::new(tools).unwrap()
}

fn boxed(tool: impl Tool + 'static) -> Box<dyn Tool> {
    Box::new(tool)
}

fn default_policy() -> PermissionPolicy {
    PermissionPolicy::from_rules([PolicyRule {
        capability: capability("workspace.read"),
        rule: PermissionRule::Allow,
    }])
}

fn run_to_end<P: ModelProvider>(
    application: &mut SiralosApplication<'_, P>,
) -> Vec<ToolLoopEvent> {
    let mut events = Vec::new();
    while let Some(event) = application.poll_event() {
        events.push(event);
    }
    events
}

fn result_in_history<'a>(
    history: &'a [ConversationItem],
    call_id: &str,
) -> Option<&'a ToolExecutionResult> {
    history.iter().find_map(|item| match item {
        ConversationItem::ToolResult {
            call_id: item_call_id, result, ..
        } if item_call_id == call_id => Some(result),
        _ => None,
    })
}

fn success_result() -> ToolExecutionResult {
    ToolExecutionResult::Success {
        output: json!({ "ok": true }),
        summary: "ok".to_owned(),
    }
}

#[test]
fn terminal_text_turn_commits_assistant_and_completes() {
    let provider = TurnScriptProvider::new(vec![vec![
        text("one"),
        text("two"),
        completed(),
    ]]);
    let registry = registry(vec![]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let events = run_to_end(&mut app);
    assert_eq!(
        events,
        vec![
            ToolLoopEvent::ResponseStarted,
            ToolLoopEvent::TextDelta { text: "one".to_owned() },
            ToolLoopEvent::TextDelta { text: "two".to_owned() },
            ToolLoopEvent::ResponseCompleted,
        ]
    );
    assert_eq!(app.history().len(), 2);
    assert!(matches!(
        &app.history()[1],
        ConversationItem::AssistantMessage { content } if content == "onetwo"
    ));
}

#[test]
fn provider_failure_is_terminal_and_commits_nothing() {
    let provider = TurnScriptProvider::new(vec![vec![ProviderEvent::Failed(
        "provider exploded".to_owned(),
    )]]);
    let registry = registry(vec![]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let events = run_to_end(&mut app);
    assert!(matches!(
        events.last(),
        Some(ToolLoopEvent::ResponseFailed { message })
            if message == "provider exploded"
    ));
    assert_eq!(app.history().len(), 1);
}

#[test]
fn single_flight_rejects_while_responding_and_recovers_after_drop() {
    let provider = TurnScriptProvider::new(vec![vec![completed()]]);
    let registry = registry(vec![]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("first".to_owned()).unwrap();
    assert_eq!(app.poll_event(), Some(ToolLoopEvent::ResponseStarted));
    assert!(app.is_responding());
    let error = app
        .send_prompt("second".to_owned())
        .expect_err("second prompt must be rejected");
    assert_eq!(error.message(), "Siralos is already responding to a prompt.");
    assert_eq!(app.poll_event(), Some(ToolLoopEvent::ResponseCompleted));
    assert!(app.is_responding());
    assert_eq!(app.poll_event(), None);
    assert!(!app.is_responding());
    app.send_prompt("second".to_owned()).unwrap();
    assert_eq!(app.poll_event(), Some(ToolLoopEvent::ResponseStarted));
}

#[test]
fn pre_cancelled_prompt_emits_cancelled_without_provider_use() {
    let provider = TurnScriptProvider::new(vec![vec![completed()]]);
    let registry = registry(vec![]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    assert_eq!(app.poll_event(), Some(ToolLoopEvent::ResponseStarted));
    app.cancel();
    assert_eq!(app.poll_event(), Some(ToolLoopEvent::ResponseCancelled));
    assert_eq!(provider.request_count(), 0);
    assert_eq!(app.history().len(), 1);
}

#[test]
fn unknown_tool_pairs_failed_result_and_provider_recovers() {
    let provider = TurnScriptProvider::new(vec![
        vec![tool_call("c1", "mystery.tool", json!({})), completed()],
        vec![text("recovered"), completed()],
    ]);
    let registry = registry(vec![]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let events = run_to_end(&mut app);
    let failed = events.iter().find_map(|event| match event {
        ToolLoopEvent::ToolFailed { call_id, tool_name, message }
            if call_id == "c1" && tool_name == "mystery.tool" =>
        {
            Some(message.as_str())
        }
        _ => None,
    });
    assert_eq!(failed, Some("Unknown tool: mystery.tool."));
    assert!(matches!(
        result_in_history(app.history(), "c1"),
        Some(ToolExecutionResult::Failed { message })
            if message == "Unknown tool: mystery.tool."
    ));
    assert_eq!(events.last(), Some(&ToolLoopEvent::ResponseCompleted));
    assert_eq!(app.completed_tool_rounds(), 1);
}

#[test]
fn five_authority_gates_are_typed_and_rechecked_per_call() {
    let tool = FixedTool::new("plain.tool", success_result());
    let cases: Vec<(Option<Vec<String>>, PermissionRule, &str)> = vec![
        (Some(vec![]), PermissionRule::Allow, "hidden"),
        (None, PermissionRule::Deny, "deny"),
        (None, PermissionRule::Ask, "ask"),
    ];
    for (surface_names, rule, label) in cases {
        let tool_calls = tool.calls.clone();
        let policy = PermissionPolicy::from_rules(match rule {
            PermissionRule::Allow => vec![PolicyRule {
                capability: capability("workspace.read"),
                rule: PermissionRule::Allow,
            }],
            other => vec![PolicyRule {
                capability: capability("workspace.read"),
                rule: other,
            }],
        });
        let surface = surface_names.map(ApprovedToolSurface::new);
        let provider = TurnScriptProvider::new(vec![
            vec![tool_call("c1", "plain.tool", json!({})), completed()],
            vec![text("recovered"), completed()],
        ]);
        let registry = registry(vec![boxed(tool.clone())]);
        let mut app = SiralosApplication::new(
            &provider, &registry, policy, surface, None,
        );
        app.send_prompt("hello".to_owned()).unwrap();
        let events = run_to_end(&mut app);
        assert_eq!(tool_calls.get(), 0, "{label}");
        let message = events.iter().find_map(|event| match event {
            ToolLoopEvent::ToolFailed { message, .. } => {
                Some(message.as_str())
            }
            _ => None,
        });
        match label {
            "hidden" => assert_eq!(
                message,
                Some(
                    "Tool plain.tool is not in the projected tool schema for this session and was denied before execution."
                )
            ),
            "deny" => assert_eq!(
                message,
                Some(
                    "Capability workspace.read is denied by policy: Policy denies workspace.read."
                )
            ),
            "ask" => assert_eq!(
                message,
                Some(
                    "Capability workspace.read requires approval, but this tool does not support a reviewable preparation protocol; the call was denied without execution."
                )
            ),
            _ => unreachable!(),
        }
        assert!(matches!(
            result_in_history(app.history(), "c1"),
            Some(ToolExecutionResult::Denied { .. })
        ));
        assert_eq!(events.last(), Some(&ToolLoopEvent::ResponseCompleted));
    }
}

#[test]
fn allow_gate_invokes_tool_exactly_once() {
    let tool = FixedTool::new("plain.tool", success_result());
    let calls = tool.calls.clone();
    let provider = TurnScriptProvider::new(vec![
        vec![tool_call("c1", "plain.tool", json!({})), completed()],
        vec![text("done"), completed()],
    ]);
    let registry = registry(vec![boxed(tool)]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let events = run_to_end(&mut app);
    assert_eq!(calls.get(), 1);
    assert!(events.iter().any(|event| matches!(
        event,
        ToolLoopEvent::ToolCompleted { call_id, tool_name, summary }
            if call_id == "c1" && tool_name == "plain.tool" && summary == "ok"
    )));
}

#[test]
fn invalid_input_enters_tool_boundary_but_not_substantive_work() {
    let entered = Rc::new(Cell::new(false));
    let worked = Rc::new(Cell::new(false));
    let tool = EntryValidatingTool {
        entered: entered.clone(),
        worked: worked.clone(),
    };
    let provider = TurnScriptProvider::new(vec![
        vec![tool_call("c1", "strict.tool", json!(42)), completed()],
        vec![completed()],
    ]);
    let registry = registry(vec![boxed(tool)]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let events = run_to_end(&mut app);
    assert!(entered.get(), "Tool.execute must be invoked");
    assert!(!worked.get(), "substantive work must not run");
    assert!(events.iter().any(|event| matches!(
        event,
        ToolLoopEvent::ToolFailed { call_id, message, .. }
            if call_id == "c1" && message == "Input must be an object."
    )));
    assert!(matches!(
        result_in_history(app.history(), "c1"),
        Some(ToolExecutionResult::InvalidInput { message })
            if message == "Input must be an object."
    ));
}

#[test]
fn cancellation_between_calls_pairs_skipped_tail() {
    let first = FixedTool::new("a.tool", success_result());
    let second = FixedTool::new("b.tool", success_result());
    let second_calls = second.calls.clone();
    let provider = TurnScriptProvider::new(vec![vec![
        tool_call("c1", "a.tool", json!({})),
        tool_call("c2", "b.tool", json!({})),
        completed(),
    ]]);
    let registry = registry(vec![boxed(first), boxed(second)]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let mut events = Vec::new();
    while let Some(event) = app.poll_event() {
        events.push(event.clone());
        if matches!(
            event,
            ToolLoopEvent::ToolCompleted { call_id, .. }
                if call_id == "c1"
        ) {
            app.cancel();
        }
    }
    assert_eq!(second_calls.get(), 0);
    assert!(matches!(events.last(), Some(ToolLoopEvent::ResponseCancelled)));
    assert!(matches!(
        result_in_history(app.history(), "c2"),
        Some(ToolExecutionResult::Cancelled { message })
            if message == "The tool call was cancelled before it executed."
    ));
}

#[test]
fn tool_returned_cancelled_stops_round_and_response() {
    let cancelled = FixedTool::new(
        "cancel.tool",
        ToolExecutionResult::Cancelled {
            message: "tool cancelled".to_owned(),
        },
    );
    let later = FixedTool::new("later.tool", success_result());
    let later_calls = later.calls.clone();
    let provider = TurnScriptProvider::new(vec![vec![
        tool_call("c1", "cancel.tool", json!({})),
        tool_call("c2", "later.tool", json!({})),
        completed(),
    ]]);
    let registry = registry(vec![boxed(cancelled), boxed(later)]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let events = run_to_end(&mut app);
    assert_eq!(later_calls.get(), 0);
    assert!(events.iter().any(|event| matches!(
        event,
        ToolLoopEvent::ToolCancelled { call_id, .. } if call_id == "c1"
    )));
    assert!(matches!(
        result_in_history(app.history(), "c2"),
        Some(ToolExecutionResult::Cancelled { message })
            if message == "The tool call was cancelled before it executed."
    ));
}

#[test]
fn round_budget_zero_blocks_the_requested_round() {
    let tool = FixedTool::new("a.tool", success_result());
    let calls = tool.calls.clone();
    let provider = TurnScriptProvider::new(vec![vec![
        tool_call("c1", "a.tool", json!({})),
        completed(),
    ]]);
    let registry = registry(vec![boxed(tool)]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        Some(0.0),
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let events = run_to_end(&mut app);
    assert_eq!(calls.get(), 0);
    assert!(matches!(
        events.last(),
        Some(ToolLoopEvent::ResponseFailed { message })
            if message == "Siralos reached the maximum of 0 tool rounds; the requested tool round was not executed."
    ));
    assert_eq!(app.history().len(), 1);
}

#[test]
fn final_answer_after_last_permitted_round_succeeds() {
    let tool = FixedTool::new("a.tool", success_result());
    let provider = TurnScriptProvider::new(vec![
        vec![tool_call("c1", "a.tool", json!({})), completed()],
        vec![text("final answer"), completed()],
    ]);
    let registry = registry(vec![boxed(tool)]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        Some(1.0),
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let events = run_to_end(&mut app);
    assert_eq!(events.last(), Some(&ToolLoopEvent::ResponseCompleted));
    assert_eq!(app.completed_tool_rounds(), 1);
    assert_eq!(app.history().len(), 4);
}

#[test]
fn mixed_success_turn_commits_assistant_before_round() {
    let tool = FixedTool::new("a.tool", success_result());
    let provider = TurnScriptProvider::new(vec![
        vec![
            text("thinking"),
            tool_call("c1", "a.tool", json!({})),
            completed(),
        ],
        vec![text("done"), completed()],
    ]);
    let registry = registry(vec![boxed(tool)]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    run_to_end(&mut app);
    let history = app.history();
    assert_eq!(history.len(), 5);
    assert!(matches!(
        &history[1],
        ConversationItem::AssistantMessage { content } if content == "thinking"
    ));
    assert!(matches!(
        &history[2],
        ConversationItem::AssistantToolCall { call_id, .. } if call_id == "c1"
    ));
    assert!(matches!(
        &history[3],
        ConversationItem::ToolResult { call_id, .. } if call_id == "c1"
    ));
    assert!(matches!(
        &history[4],
        ConversationItem::AssistantMessage { content } if content == "done"
    ));
}

#[test]
fn mixed_cancelled_turn_keeps_round_but_drops_assistant_text() {
    let tool = FixedTool::new(
        "cancel.tool",
        ToolExecutionResult::Cancelled { message: "cancelled".to_owned() },
    );
    let provider = TurnScriptProvider::new(vec![vec![
        text("thinking"),
        tool_call("c1", "cancel.tool", json!({})),
        completed(),
    ]]);
    let registry = registry(vec![boxed(tool)]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let events = run_to_end(&mut app);
    assert_eq!(events.last(), Some(&ToolLoopEvent::ResponseCancelled));
    let history = app.history();
    assert_eq!(history.len(), 3);
    assert!(!matches!(history[1], ConversationItem::AssistantMessage { .. }));
}

#[test]
fn tool_result_statuses_map_to_the_closed_event_surface() {
    let cases: Vec<(ToolExecutionResult, ToolLoopEvent)> = vec![
        (
            success_result(),
            ToolLoopEvent::ToolCompleted {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                summary: "ok".to_owned(),
            },
        ),
        (
            ToolExecutionResult::InvalidInput { message: "m".to_owned() },
            ToolLoopEvent::ToolFailed {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                message: "m".to_owned(),
            },
        ),
        (
            ToolExecutionResult::Denied { message: "m".to_owned() },
            ToolLoopEvent::ToolFailed {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                message: "m".to_owned(),
            },
        ),
        (
            ToolExecutionResult::Conflict { message: "m".to_owned() },
            ToolLoopEvent::ToolFailed {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                message: "m".to_owned(),
            },
        ),
        (
            ToolExecutionResult::Failed { message: "m".to_owned() },
            ToolLoopEvent::ToolFailed {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                message: "m".to_owned(),
            },
        ),
        (
            ToolExecutionResult::Cancelled { message: "m".to_owned() },
            ToolLoopEvent::ToolCancelled {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
            },
        ),
        (
            ToolExecutionResult::TimedOut { message: "m".to_owned() },
            ToolLoopEvent::ToolFailed {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                message: "m".to_owned(),
            },
        ),
        (
            ToolExecutionResult::OutputLimit { message: "m".to_owned() },
            ToolLoopEvent::ToolFailed {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                message: "m".to_owned(),
            },
        ),
        (
            ToolExecutionResult::SandboxDenied { message: "m".to_owned() },
            ToolLoopEvent::ToolFailed {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                message: "m".to_owned(),
            },
        ),
        (
            ToolExecutionResult::SandboxUnavailable {
                message: "m".to_owned(),
            },
            ToolLoopEvent::ToolFailed {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                message: "m".to_owned(),
            },
        ),
        (
            ToolExecutionResult::WorkspaceViolation {
                message: "m".to_owned(),
            },
            ToolLoopEvent::ToolFailed {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                message: "m".to_owned(),
            },
        ),
        (
            ToolExecutionResult::Unavailable { message: "m".to_owned() },
            ToolLoopEvent::ToolFailed {
                call_id: "c1".to_owned(),
                tool_name: "stub.tool".to_owned(),
                message: "m".to_owned(),
            },
        ),
    ];
    for (result, expected_event) in cases {
        let tool = FixedTool::new("stub.tool", result);
        let registry = registry(vec![boxed(tool)]);
        let provider = TurnScriptProvider::new(vec![vec![
            tool_call("c1", "stub.tool", json!({})),
            completed(),
        ]]);
        let mut app = SiralosApplication::new(
            &provider,
            &registry,
            default_policy(),
            None,
            None,
        );
        app.send_prompt("hello".to_owned()).unwrap();
        let events = run_to_end(&mut app);
        assert!(events.contains(&expected_event), "{events:?}");
    }
}

#[test]
fn display_input_uses_host_supplied_object_key_order() {
    let tool = FixedTool::new("display.tool", success_result());
    let registry = registry(vec![boxed(tool)]);
    let provider = TurnScriptProvider::new(vec![vec![
        ordered_tool_call(
            "c1",
            "display.tool",
            json!({ "a": 1, "z": 2 }),
            r#"{"z":2,"a":1}"#,
        ),
        completed(),
    ]]);
    let mut app = SiralosApplication::new(
        &provider,
        &registry,
        default_policy(),
        None,
        None,
    );
    app.send_prompt("hello".to_owned()).unwrap();
    let events = run_to_end(&mut app);
    let started = events.iter().find_map(|event| match event {
        ToolLoopEvent::ToolStarted { display_input, .. } => {
            Some(display_input)
        }
        _ => None,
    });
    let expected = r#"{"z":2,"a":1}"#.encode_utf16().collect::<Vec<_>>();
    assert_eq!(started.unwrap().units(), expected);
}
