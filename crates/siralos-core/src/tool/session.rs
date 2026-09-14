//! Synchronous pull-based Application Tool Loop around the R7.1
//! bounded provider turn.
//!
//! The TypeScript reference exposes an async generator. Rust represents
//! the same observable behavior as an explicit pull machine: the Host
//! calls [`SiralosApplication::poll_event`] one event at a time,
//! cancellation is Host-owned between polls, and a second
//! [`SiralosApplication::send_prompt`] while a response is active
//! receives the typed `AlreadyResponding` rejection. No async runtime,
//! thread, lock, or shared synchronization is introduced.

use std::collections::VecDeque;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::projection::{
    LastProjection, ProjectedRequest, ProjectionInput, ProjectionService,
    capacity::ContextCapacity, evidence::EvidenceProjectorOptions,
    pressure::PressureState, segments::SegmentInput,
    visibility::ProjectionMode,
};
use crate::provider::{
    CancellationToken, ConversationItem, ModelProvider, ProviderEvent,
    ToolDefinition, ToolExecutionResult, TurnOutcome, TurnStep,
    open_provider_turn,
};
use crate::tool::budget::RoundBudget;
use crate::tool::events::ToolLoopEvent;
use crate::tool::permission::{
    PermissionDecision, PermissionPolicy, evaluate_permission,
};
use crate::tool::registry::{ApprovedToolSurface, Tool, ToolRegistry};
use crate::tool::round::{
    ExecutableToolCall, ToolCallExecution, ToolCallExecutor, ToolRoundKind,
    ToolRoundRunner, ToolRoundStep,
};

/// Why a prompt could not be started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStartError {
    /// Another prompt response is still active.
    AlreadyResponding,
}

impl PromptStartError {
    /// The exact externally observable message.
    pub fn message(&self) -> &'static str {
        match self {
            Self::AlreadyResponding => {
                "Siralos is already responding to a prompt."
            }
        }
    }
}

impl fmt::Display for PromptStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for PromptStartError {}

/// Host Tool-call executor: the exact per-call authorization gate order
/// plus guarded `Tool::execute`.
#[derive(Clone)]
struct HostToolExecutor<'a> {
    registry: &'a ToolRegistry,
    policy: PermissionPolicy,
    surface: Option<ApprovedToolSurface>,
}

impl HostToolExecutor<'_> {
    /// Policy-filtered provider-visible definitions (denied tools are
    /// never shown to the provider; ask remains visible).
    fn provider_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.registry
            .definitions()
            .into_iter()
            .filter(|info| {
                !matches!(
                    evaluate_permission(&info.capability, &self.policy),
                    PermissionDecision::Deny { .. }
                )
            })
            .map(|info| info.definition)
            .collect()
    }
}

impl ToolCallExecutor for HostToolExecutor<'_> {
    fn execute_call(
        &mut self,
        call: ExecutableToolCall<'_>,
        cancellation: crate::provider::CancellationSignal<'_>,
    ) -> ToolCallExecution {
        let mut events = vec![ToolLoopEvent::ToolStarted {
            call_id: call.call_id.to_owned(),
            tool_name: call.tool_name.to_owned(),
            display_input: call.display_input(),
        }];
        let Some(tool) = self.registry.get(call.tool_name) else {
            let message = format!("Unknown tool: {}.", call.tool_name);
            events.push(failure_event(call.call_id, call.tool_name, &message));
            return ToolCallExecution {
                events,
                result: ToolExecutionResult::Failed { message },
            };
        };
        if let Some(surface) = &self.surface {
            if !surface.contains(call.tool_name) {
                let message = format!(
                    "Tool {} is not in the projected tool schema for this session and was denied before execution.",
                    call.tool_name
                );
                events.push(failure_event(
                    call.call_id,
                    call.tool_name,
                    &message,
                ));
                return ToolCallExecution {
                    events,
                    result: ToolExecutionResult::Denied { message },
                };
            }
        }
        let capability = tool.capability();
        match evaluate_permission(capability, &self.policy) {
            PermissionDecision::Deny { reason } => {
                let message = format!(
                    "Capability {capability} is denied by policy: {reason}"
                );
                events.push(failure_event(
                    call.call_id,
                    call.tool_name,
                    &message,
                ));
                return ToolCallExecution {
                    events,
                    result: ToolExecutionResult::Denied { message },
                };
            }
            PermissionDecision::Ask { .. } => {
                let message = format!(
                    "Capability {capability} requires approval, but this tool does not support a reviewable preparation protocol; the call was denied without execution."
                );
                events.push(failure_event(
                    call.call_id,
                    call.tool_name,
                    &message,
                ));
                return ToolCallExecution {
                    events,
                    result: ToolExecutionResult::Denied { message },
                };
            }
            PermissionDecision::Allow => {}
        }
        let result = execute_guarded(tool, call.input, cancellation);
        events.push(outcome_event(call.call_id, call.tool_name, &result));
        ToolCallExecution { events, result }
    }
}

/// Invoke a Tool exactly once; no automatic retries. A panicking Tool
/// implementation is converted to the reference non-Error throw result
/// (the Tool boundary is trusted Host code and the conversion is the
/// observable TypeScript catch semantics).
fn execute_guarded(
    tool: &dyn Tool,
    input: &serde_json::Value,
    cancellation: crate::provider::CancellationSignal<'_>,
) -> ToolExecutionResult {
    match catch_unwind(AssertUnwindSafe(|| tool.execute(input, cancellation)))
    {
        Ok(result) => result,
        Err(_) => ToolExecutionResult::Failed {
            message: "The provider failed with an unknown error.".to_owned(),
        },
    }
}

fn failure_event(
    call_id: &str,
    tool_name: &str,
    message: &str,
) -> ToolLoopEvent {
    ToolLoopEvent::ToolFailed {
        call_id: call_id.to_owned(),
        tool_name: tool_name.to_owned(),
        message: message.to_owned(),
    }
}

fn outcome_event(
    call_id: &str,
    tool_name: &str,
    result: &ToolExecutionResult,
) -> ToolLoopEvent {
    match result {
        ToolExecutionResult::Success { summary, .. } => {
            ToolLoopEvent::ToolCompleted {
                call_id: call_id.to_owned(),
                tool_name: tool_name.to_owned(),
                summary: summary.clone(),
            }
        }
        ToolExecutionResult::Cancelled { .. } => {
            ToolLoopEvent::ToolCancelled {
                call_id: call_id.to_owned(),
                tool_name: tool_name.to_owned(),
            }
        }
        other => ToolLoopEvent::ToolFailed {
            call_id: call_id.to_owned(),
            tool_name: tool_name.to_owned(),
            message: other.message().to_owned(),
        },
    }
}

/// One collected successful provider turn pending text replay and Tool
/// Round handling.
struct CollectedTurn {
    assistant_text: String,
    text_deltas: VecDeque<String>,
    tool_calls: Vec<crate::provider::TurnToolCall>,
}

enum Phase<'a> {
    Start,
    CollectTurn,
    // This request is disposable: after a Tool round, the machine returns to
    // CollectTurn and projects again from the current authoritative history.
    InvokeProvider {
        request: ProviderRequest,
        pressure_pending: bool,
    },
    EmitText {
        turn: CollectedTurn,
    },
    /// Streaming collection (S2 chunk 2): the machine pulls ONE provider
    /// event per step, so a frontend gets control (and can repaint)
    /// between events. Text deltas leave as they arrive; the collector
    /// keeps validating to the same outcome as the whole-turn path.
    StreamTurn {
        stream: Box<dyn Iterator<Item = ProviderEvent> + 'a>,
        collector: crate::provider::ProviderTurnCollector,
    },
    RunningRound {
        runner: ToolRoundRunner<HostToolExecutor<'a>>,
        assistant_text: String,
    },
    Terminal {
        event: ToolLoopEvent,
    },
    Done,
}

enum ProviderRequest {
    Raw,
    Projected(Box<ProjectedRequest>),
}

/// One active prompt response machine.
struct ResponseMachine<'a, P: ModelProvider> {
    provider: &'a P,
    host: HostToolExecutor<'a>,
    token: CancellationToken,
    max_tool_rounds: RoundBudget,
    history: Vec<ConversationItem>,
    attempted_tool_rounds: u32,
    completed_tool_rounds: u32,
    provider_turns: u32,
    /// Whether to emit the `ProviderPending` keep-alive tick (S2 chunk
    /// 4b). OFF by default so the pinned event sequences -- the corpus and
    /// every unit test -- stay byte-identical; a frontend that can repaint
    /// turns it on.
    progress_ticks: bool,
    phase: Phase<'a>,
}

impl<'a, P: ModelProvider> ResponseMachine<'a, P> {
    fn new(
        provider: &'a P,
        host: HostToolExecutor<'a>,
        max_tool_rounds: RoundBudget,
    ) -> Self {
        Self {
            provider,
            host,
            token: CancellationToken::new(),
            max_tool_rounds,
            history: Vec::new(),
            attempted_tool_rounds: 0,
            completed_tool_rounds: 0,
            provider_turns: 0,
            progress_ticks: false,
            phase: Phase::Start,
        }
    }

    fn cancel(&self) {
        self.token.cancel();
    }

    fn next_event(
        &mut self,
        mut projection_service: Option<&mut ProjectionService>,
        projection_config: &ApplicationProjectionConfig,
    ) -> Option<ToolLoopEvent> {
        loop {
            let phase = std::mem::replace(&mut self.phase, Phase::Done);
            match phase {
                Phase::Start => {
                    self.phase = Phase::CollectTurn;
                    return Some(ToolLoopEvent::ResponseStarted);
                }
                Phase::CollectTurn => {
                    if self.token.is_cancelled() {
                        self.phase = Phase::Terminal {
                            event: ToolLoopEvent::ResponseCancelled,
                        };
                        continue;
                    }
                    if let Some(service) = projection_service.as_deref_mut() {
                        let request = self.project_current_provider_turn(
                            service,
                            projection_config,
                        );
                        let pressure_pending =
                            request.pressure.state != PressureState::Normal;
                        self.phase = Phase::InvokeProvider {
                            request: ProviderRequest::Projected(Box::new(
                                request,
                            )),
                            pressure_pending,
                        };
                    } else {
                        self.phase = Phase::InvokeProvider {
                            request: ProviderRequest::Raw,
                            pressure_pending: false,
                        };
                    }
                }
                Phase::InvokeProvider { request, pressure_pending } => {
                    if self.token.is_cancelled() {
                        self.phase = Phase::Terminal {
                            event: ToolLoopEvent::ResponseCancelled,
                        };
                        continue;
                    }
                    if pressure_pending {
                        let (state, estimated_tokens, working_maximum) =
                            match &request {
                                ProviderRequest::Projected(request) => (
                                    request.pressure.state.as_str().to_owned(),
                                    request.pressure.estimated_tokens,
                                    request.pressure.working_maximum,
                                ),
                                ProviderRequest::Raw => {
                                    unreachable!(
                                        "raw provider requests have no pressure"
                                    )
                                }
                            };
                        self.phase = Phase::InvokeProvider {
                            request,
                            pressure_pending: false,
                        };
                        return Some(ToolLoopEvent::ContextPressure {
                            state,
                            estimated_tokens,
                            working_maximum,
                        });
                    }
                    if let ProviderRequest::Projected(projected) = &request {
                        if let Some(blocked) = &projected.blocked {
                            let message = blocked.message().to_owned();
                            self.phase = Phase::Terminal {
                                event: ToolLoopEvent::ResponseFailed {
                                    message,
                                },
                            };
                            continue;
                        }
                    }
                    self.provider_turns += 1;
                    // S2 chunk 2: OPEN the turn, then stream it one event
                    // per step instead of collecting it in one call.
                    let opened = match request {
                        ProviderRequest::Raw => {
                            let definitions =
                                self.host.provider_tool_definitions();
                            open_provider_turn(
                                self.provider,
                                self.history.as_slice(),
                                &definitions,
                                None,
                                &self.token,
                            )
                        }
                        ProviderRequest::Projected(projected) => {
                            let projected = *projected;
                            open_provider_turn(
                                self.provider,
                                projected.messages.as_slice(),
                                &projected.tools,
                                projected.system,
                                &self.token,
                            )
                        }
                    };
                    self.phase = match opened {
                        Ok((stream, collector)) => {
                            Phase::StreamTurn { stream, collector }
                        }
                        Err(outcome) => self.handle_provider_outcome(outcome),
                    };
                    // S2 chunk 4b: one keep-alive tick BEFORE the first
                    // pull. Waiting for the first byte can take seconds, and
                    // this is the last moment a frontend can paint
                    // "working" and read an interrupt key.
                    if self.progress_ticks
                        && matches!(self.phase, Phase::StreamTurn { .. })
                    {
                        return Some(ToolLoopEvent::ProviderPending);
                    }
                }
                Phase::StreamTurn { mut stream, mut collector } => {
                    // The stream holds no cancellation signal; the Host is
                    // the authority and checks its own token between pulls.
                    if self.token.is_cancelled() {
                        self.phase = Phase::Terminal {
                            event: ToolLoopEvent::ResponseCancelled,
                        };
                        continue;
                    }
                    match stream.next() {
                        None => {
                            let outcome =
                                Self::without_text_deltas(collector.finish());
                            self.phase = self.handle_provider_outcome(outcome);
                        }
                        Some(event) => {
                            // Ask the collector what this event ADDED
                            // instead of matching the event shape: a RAW
                            // event is validated (and becomes a delta)
                            // inside the collector.
                            let before = collector.text_delta_count();
                            let before_reasoning = collector.reasoning_count();
                            let step = collector.push(event);
                            let live_text = collector
                                .text_delta_at(before)
                                .map(str::to_owned);
                            let live_reasoning = collector
                                .reasoning_at(before_reasoning)
                                .map(str::to_owned);
                            match step {
                                TurnStep::Terminal(outcome) => {
                                    self.phase =
                                        self.handle_provider_outcome(outcome);
                                }
                                TurnStep::Stop => {
                                    let outcome = Self::without_text_deltas(
                                        collector.finish(),
                                    );
                                    self.phase =
                                        self.handle_provider_outcome(outcome);
                                }
                                TurnStep::Continue => {
                                    self.phase = Phase::StreamTurn {
                                        stream,
                                        collector,
                                    };
                                    if let Some(text) = live_text {
                                        // Live: the frontend sees the
                                        // answer as it arrives.
                                        return Some(
                                            ToolLoopEvent::TextDelta { text },
                                        );
                                    }
                                    if let Some(text) = live_reasoning {
                                        // Thinking streams too, on its own
                                        // channel: it never becomes the
                                        // answer or the history.
                                        return Some(
                                            ToolLoopEvent::ReasoningDelta {
                                                text,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Phase::EmitText { mut turn } => {
                    match turn.text_deltas.pop_front() {
                        Some(text) => {
                            self.phase = Phase::EmitText { turn };
                            return Some(ToolLoopEvent::TextDelta { text });
                        }
                        None => {
                            self.phase = self.handle_collected_turn(turn);
                        }
                    }
                }
                Phase::RunningRound { mut runner, assistant_text } => {
                    match runner.next(&self.token) {
                        Some(ToolRoundStep::Event(event)) => {
                            self.phase =
                                Phase::RunningRound { runner, assistant_text };
                            return Some(event);
                        }
                        Some(ToolRoundStep::Finished(outcome)) => {
                            self.phase =
                                self.finish_round(assistant_text, outcome);
                        }
                        None => {
                            // A finished runner is always consumed by the
                            // Finished step above; reaching this branch
                            // would indicate an internal state error.
                            self.phase = Phase::Done;
                        }
                    }
                }
                Phase::Terminal { event } => {
                    self.phase = Phase::Done;
                    return Some(event);
                }
                Phase::Done => return None,
            }
        }
    }

    fn project_current_provider_turn(
        &mut self,
        service: &mut ProjectionService,
        config: &ApplicationProjectionConfig,
    ) -> ProjectedRequest {
        let registered_tools = self.host.registry.definitions();
        let projected = service.project(ProjectionInput {
            mode: config.mode.unwrap_or(ProjectionMode::Generic),
            messages: &self.history,
            registered_tools: &registered_tools,
            provider_tool_calling: config
                .provider_tool_calling
                .unwrap_or(true),
            capacity: config.capacity.clone().unwrap_or_default(),
            pressure_limits:
                crate::projection::pressure::PressureLimits::default(),
            segments: config.segments.clone(),
            evidence_options: config
                .evidence_options
                .clone()
                .unwrap_or_default(),
            allowed_tool_names: config.allowed_tool_names.clone(),
            policy: &self.host.policy,
            task_revision: config.task_revision,
        });
        self.host.surface = Some(ApprovedToolSurface::new(
            projected.tool_projection.approved_names.clone(),
        ));
        projected
    }

    /// The turn outcome with its text deltas REMOVED: the streaming phase
    /// already emitted them as they arrived (S2 chunk 2), so replaying them
    /// through `EmitText` would double the answer. The assistant text and the
    /// tool calls -- everything the later phases act on -- are kept.
    fn without_text_deltas(outcome: TurnOutcome) -> TurnOutcome {
        match outcome {
            TurnOutcome::Turn { assistant_text, tool_calls, .. } => {
                TurnOutcome::Turn {
                    assistant_text,
                    text_deltas: Vec::new(),
                    tool_calls,
                }
            }
            other => other,
        }
    }

    fn handle_provider_outcome(&mut self, outcome: TurnOutcome) -> Phase<'a> {
        match outcome {
            TurnOutcome::Cancelled => {
                Phase::Terminal { event: ToolLoopEvent::ResponseCancelled }
            }
            TurnOutcome::Failed { failure } => Phase::Terminal {
                event: ToolLoopEvent::ResponseFailed {
                    message: failure.application_message(),
                },
            },
            TurnOutcome::Turn { assistant_text, text_deltas, tool_calls } => {
                let turn = CollectedTurn {
                    assistant_text,
                    text_deltas: text_deltas.into(),
                    tool_calls,
                };
                if turn.text_deltas.is_empty() {
                    self.handle_collected_turn(turn)
                } else {
                    Phase::EmitText { turn }
                }
            }
        }
    }

    fn handle_collected_turn(&mut self, turn: CollectedTurn) -> Phase<'a> {
        if turn.tool_calls.is_empty() {
            if !turn.assistant_text.is_empty() {
                self.history.push(ConversationItem::AssistantMessage {
                    content: turn.assistant_text,
                });
            }
            return Phase::Terminal {
                event: ToolLoopEvent::ResponseCompleted,
            };
        }
        if self.attempted_tool_rounds >= self.max_tool_rounds.get() {
            return Phase::Terminal {
                event: ToolLoopEvent::ResponseFailed {
                    message: self.max_tool_rounds.cap_message(),
                },
            };
        }
        self.attempted_tool_rounds += 1;
        let runner = ToolRoundRunner::new(turn.tool_calls, self.host.clone());
        Phase::RunningRound { runner, assistant_text: turn.assistant_text }
    }

    fn finish_round(
        &mut self,
        assistant_text: String,
        outcome: crate::tool::round::ToolRoundOutcome,
    ) -> Phase<'a> {
        match outcome.kind {
            ToolRoundKind::Completed => {
                self.completed_tool_rounds += 1;
                if !assistant_text.is_empty() {
                    self.history.push(ConversationItem::AssistantMessage {
                        content: assistant_text,
                    });
                }
                self.history.extend(outcome.transcript);
                Phase::CollectTurn
            }
            ToolRoundKind::Cancelled => {
                // A cancelled mixed turn commits the full paired round
                // transcript but never the assistant text.
                self.history.extend(outcome.transcript);
                Phase::Terminal { event: ToolLoopEvent::ResponseCancelled }
            }
        }
    }

    fn provider_turn_count(&self) -> u32 {
        self.provider_turns
    }
}

enum AppState<'a, P: ModelProvider> {
    Idle,
    Responding(Box<ResponseMachine<'a, P>>),
}

/// Per-application projection configuration (R7.3).
///
/// When present, the application projects every provider request through
/// [`ProjectionService`] before invoking the provider. The same projection
/// supplies both provider-visible `ToolDefinition`s and the
/// `ApprovedToolSurface`; the R7.2 lower-level loop remains available when
/// no projection is configured.
#[derive(Debug, Clone, Default)]
pub struct ApplicationProjectionConfig {
    /// Projection mode (validated, domain-neutral).
    pub mode: Option<ProjectionMode>,
    /// Whether the selected provider supports tool calling (default true).
    pub provider_tool_calling: Option<bool>,
    /// Working context capacity (default 32_768).
    pub capacity: Option<ContextCapacity>,
    /// Explicit segments for this application (empty = default stable instructions).
    pub segments: Vec<SegmentInput>,
    /// Evidence projector options (secrets/bytes).
    pub evidence_options: Option<EvidenceProjectorOptions>,
    /// Optional allow-list of exact Tool names for the mode.
    pub allowed_tool_names: Option<Vec<String>>,
    /// Optional task revision for cache invalidation.
    pub task_revision: Option<u64>,
}

/// Generic single-flight Application Tool Loop.
///
/// The application owns authoritative history. One prompt response is
/// active at a time; starting another while responding fails with the
/// exact `AlreadyResponding` message.
pub struct SiralosApplication<'a, P: ModelProvider> {
    provider: &'a P,
    registry: &'a ToolRegistry,
    policy: PermissionPolicy,
    surface: Option<ApprovedToolSurface>,
    max_tool_rounds: RoundBudget,
    history: Vec<ConversationItem>,
    completed_tool_rounds: u32,
    provider_turn_count: u32,
    state: AppState<'a, P>,
    /// R7.3 projection service (None = R7.2 direct loop).
    projection_service: Option<ProjectionService>,
    /// R7.3 projection configuration (only when service is Some).
    projection_config: ApplicationProjectionConfig,
    /// Whether a started turn carries the `ProviderPending` keep-alive tick.
    /// Session configuration (set by the CLI for a frontend that can
    /// repaint), so `send_prompt` can hand it to every machine it builds.
    progress_ticks: bool,
}

impl<'a, P: ModelProvider> SiralosApplication<'a, P> {
    /// Compose the loop from Host-owned dependencies.
    ///
    /// `max_tool_rounds` is normalized with the exact reference rules
    /// (missing/non-finite → 8; floor; clamp to `0..=32`).
    pub fn new(
        provider: &'a P,
        registry: &'a ToolRegistry,
        policy: PermissionPolicy,
        surface: Option<ApprovedToolSurface>,
        max_tool_rounds: Option<f64>,
    ) -> Self {
        Self {
            provider,
            registry,
            policy,
            surface,
            max_tool_rounds: RoundBudget::normalize(max_tool_rounds),
            history: Vec::new(),
            completed_tool_rounds: 0,
            provider_turn_count: 0,
            state: AppState::Idle,
            projection_service: None,
            projection_config: ApplicationProjectionConfig::default(),
            progress_ticks: false,
        }
    }

    /// Configure R7.3 projection for this application.
    ///
    /// When configured, every provider request is projected through
    /// `ProjectionService` before the provider is invoked. The same
    /// projection supplies both provider-visible tool definitions and
    /// `ApprovedToolSurface`; the R7.2 lower-level per-call permission
    /// check remains mandatory.
    pub fn with_projection(
        mut self,
        service: ProjectionService,
        config: ApplicationProjectionConfig,
    ) -> Self {
        self.projection_service = Some(service);
        self.projection_config = config;
        self
    }

    /// Set or replace the R7.3 projection configuration on a live
    /// application (only when not currently responding).
    pub fn set_projection(
        &mut self,
        service: ProjectionService,
        config: ApplicationProjectionConfig,
    ) -> Result<(), PromptStartError> {
        if matches!(&self.state, AppState::Responding(_)) {
            return Err(PromptStartError::AlreadyResponding);
        }
        self.projection_service = Some(service);
        self.projection_config = config;
        Ok(())
    }

    /// Detached last projection snapshot (disposable, for CLI observability).
    pub fn last_projection(&self) -> Option<&LastProjection> {
        self.projection_service.as_ref().and_then(|svc| svc.last_projection())
    }

    /// Whether R7.3 projection is configured.
    pub fn has_projection(&self) -> bool {
        self.projection_service.is_some()
    }

    /// Start one prompt response.
    ///
    /// Appends the user message exactly once and emits
    /// `response_started` as the first pull event. Returns
    /// [`PromptStartError::AlreadyResponding`] while another response is
    /// active.
    pub fn send_prompt(
        &mut self,
        text: String,
    ) -> Result<(), PromptStartError> {
        if matches!(&self.state, AppState::Responding(_)) {
            return Err(PromptStartError::AlreadyResponding);
        }
        let mut history = std::mem::take(&mut self.history);
        history.push(ConversationItem::UserMessage { content: text });
        let host = HostToolExecutor {
            registry: self.registry,
            policy: self.policy.clone(),
            // R7.3 refreshes the surface from the current projection before
            // each provider request. The direct R7.2 path keeps its caller-
            // supplied surface unchanged.
            surface: if self.projection_service.is_some() {
                None
            } else {
                self.surface.clone()
            },
        };
        let mut machine =
            ResponseMachine::new(self.provider, host, self.max_tool_rounds);
        machine.history = history;
        // Session configuration, not per-machine: a frontend that asked for
        // the keep-alive tick gets it on EVERY turn it starts.
        machine.progress_ticks = self.progress_ticks;
        self.state = AppState::Responding(Box::new(machine));
        Ok(())
    }

    /// Pull the next Tool-loop event, advancing at most one observable
    /// step.
    ///
    /// Returns `None` after the terminal event has been consumed; the
    /// authoritative history and counters are restored to the
    /// application at that point.
    pub fn poll_event(&mut self) -> Option<ToolLoopEvent> {
        let event = {
            let projection_service = self.projection_service.as_mut();
            match &mut self.state {
                AppState::Idle => return None,
                AppState::Responding(machine) => machine
                    .next_event(projection_service, &self.projection_config),
            }
        };
        if event.is_none() {
            self.restore_machine();
        }
        event
    }

    /// Host cancellation authority. Tools and providers only ever
    /// observe the read-only signal.
    pub fn cancel(&mut self) {
        if let AppState::Responding(machine) = &self.state {
            machine.cancel();
        }
    }

    /// Ask for the `ProviderPending` keep-alive tick while a response is
    /// being collected (S2 chunk 4b).
    ///
    /// A frontend that can repaint (and read an interrupt key) turns this
    /// on; it is OFF by default so the pinned event sequences stay
    /// byte-identical. It is SESSION configuration: the frontend calls it
    /// once, and every turn it starts afterwards carries the tick. (Setting
    /// it only on an already-running machine made the tick unreachable, since
    /// `send_prompt` builds a fresh machine.)
    pub fn enable_provider_progress_ticks(&mut self) {
        self.progress_ticks = true;
        if let AppState::Responding(machine) = &mut self.state {
            machine.progress_ticks = true;
        }
    }

    /// Whether a response is currently active.
    pub fn is_responding(&self) -> bool {
        matches!(&self.state, AppState::Responding(_))
    }

    /// The authoritative Host-owned conversation history.
    pub fn history(&self) -> &[ConversationItem] {
        match &self.state {
            AppState::Idle => &self.history,
            AppState::Responding(machine) => &machine.history,
        }
    }

    /// The number of completed Tool Rounds in the current (or most
    /// recent) response.
    pub fn completed_tool_rounds(&self) -> u32 {
        match &self.state {
            AppState::Idle => self.completed_tool_rounds,
            AppState::Responding(machine) => machine.completed_tool_rounds,
        }
    }

    /// The number of provider turns collected by the current (or most
    /// recent) response.
    pub fn provider_turn_count(&self) -> u32 {
        match &self.state {
            AppState::Idle => self.provider_turn_count,
            AppState::Responding(machine) => machine.provider_turn_count(),
        }
    }

    fn restore_machine(&mut self) {
        let state = std::mem::replace(&mut self.state, AppState::Idle);
        if let AppState::Responding(machine) = state {
            let provider_turn_count = machine.provider_turn_count();
            self.history = machine.history;
            self.completed_tool_rounds = machine.completed_tool_rounds;
            self.provider_turn_count = provider_turn_count;
        }
    }
}
