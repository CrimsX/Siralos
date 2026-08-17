//! R7.3 Projection parity — generic Host projection.
//!
//! The frozen §14 contract: capacity, estimator, pressure, reduction,
//! context segments, deterministic ordering/serialization, stable and Tool
//! fingerprints, evidence model-view pipeline, watermark/revision cache,
//! generic Tool visibility with `ApprovedToolSurface` coupling,
//! `ProjectionMode`, and the one Host transformation into a
//! provider-neutral `ProjectedRequest`. No R7.4/R7.5, R8/R9, or R10
//! semantics are implemented here.

pub mod cache;
pub mod capacity;
pub mod estimator;
pub mod evidence;
pub mod pressure;
pub mod segments;
pub mod trim;
pub mod visibility;

#[cfg(test)]
mod adversarial;

use crate::provider::{ConversationItem, ToolDefinition, ToolExecutionResult};
use crate::tool::permission::PermissionPolicy;
use crate::tool::registry::RegisteredToolInfo;
use cache::RevisionBoundCache;
use capacity::ContextCapacity;
use estimator::{estimate_conversation_tokens, estimate_tokens};
use evidence::{
    EvidenceProjectorOptions, ModelEvidenceView, project_for_model,
};
use pressure::{
    ContextPressure, PressureLimits, PressureState, classify_pressure,
};
use segments::{
    ContextProjection, SegmentInput, project_segments, serialize_prefix,
};
use trim::trim_conversation_preserving_pairs;
use visibility::{
    ProjectionMode, ToolProjection, ToolProjectionInput, project_tools,
};

/// Maximum plan segment bytes (4 KiB) — informational for generic seam.
pub const MAX_PLAN_SEGMENT_BYTES: usize = 4 * 1024;
/// Maximum executor-brief segment bytes (4 KiB).
pub const MAX_EXECUTOR_BRIEF_SEGMENT_BYTES: usize = 4 * 1024;
/// Combined reference/research/scene volatile budget (12 KiB) — host-seam only.
pub const REFERENCE_RESEARCH_VOLATILE_BUDGET_BYTES: usize = 12 * 1024;

/// Typed blocked reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockedReason {
    /// Working budget exceeded after reduction.
    Hard(String),
    /// Provider does not support tool calling for a requiring mode.
    Unsupported(String),
}

impl BlockedReason {
    /// Wire type string.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Hard(_) => "hard",
            Self::Unsupported(_) => "unsupported",
        }
    }
    /// Message.
    pub fn message(&self) -> &str {
        match self {
            Self::Hard(m) | Self::Unsupported(m) => m,
        }
    }
}

/// The one Host transformation into a provider-neutral request.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedRequest {
    /// Projection mode.
    pub mode: ProjectionMode,
    /// Detached provider-visible messages (tool results projected).
    pub messages: Vec<ConversationItem>,
    /// Provider-visible tool definitions (available+gated, registry order).
    pub tools: Vec<ToolDefinition>,
    /// Provider system prefix (stable+contextual), or None when unsupported-block empty.
    pub system: Option<String>,
    /// Final pressure.
    pub pressure: ContextPressure,
    /// Tool projection (visibility, fingerprint, counts, approved names).
    pub tool_projection: ToolProjection,
    /// Context projection (segments, fingerprints, bytes).
    pub context_projection: ContextProjection,
    /// Final estimated tokens (system+tools+messages after reduction).
    pub estimated_tokens: usize,
    /// Non-null when the request must not be sent to the provider.
    pub blocked: Option<BlockedReason>,
}

/// Typed observability snapshot (future `/context` / `/tools`).
///
/// Disposable, detached, non-authoritative.
#[derive(Debug, Clone, PartialEq)]
pub struct LastProjection {
    /// Projected request (detached clone).
    pub request: ProjectedRequest,
    /// Cache size at projection time.
    pub evidence_cache_size: usize,
    /// Whether reduction occurred.
    pub reduced: bool,
    /// Dropped item count when reduced.
    pub dropped_items: usize,
}

/// Input to the one-shot projection.
#[derive(Debug, Clone)]
pub struct ProjectionInput<'a> {
    /// Projection mode (validated, domain-neutral).
    pub mode: ProjectionMode,
    /// Authoritative Host messages (detached copy is projected).
    pub messages: &'a [ConversationItem],
    /// Registered tools in registry order.
    pub registered_tools: &'a [RegisteredToolInfo],
    /// Whether the selected provider supports tool calling.
    pub provider_tool_calling: bool,
    /// Context capacity (working maximum is authority).
    pub capacity: ContextCapacity,
    /// Pressure limits (usually default 0.70/0.85/1.00).
    pub pressure_limits: PressureLimits,
    /// Host-supplied stable/contextual/volatile segments (already bounded).
    pub segments: Vec<SegmentInput>,
    /// Evidence projector options (secrets, max bytes).
    pub evidence_options: EvidenceProjectorOptions,
    /// Optional Host-allowed exact Tool names for this mode (None = generic).
    pub allowed_tool_names: Option<Vec<String>>,
    /// Host permission policy (R7.2 owner) for `available/gated/hidden`.
    pub policy: &'a PermissionPolicy,
    /// Task contract revision for cache invalidation (None = no revision).
    pub task_revision: Option<u64>,
}

/// Stateless pure projection function (no cache, no revision).
///
/// Suitable for differential tests and for callers that do not need the
/// watermark cache. For cached tool-result views, use [`ProjectionService`].
pub fn project_request(input: ProjectionInput<'_>) -> ProjectedRequest {
    let mut service = ProjectionService::new();
    service.project(input)
}

/// Stateful projection service holding the disposable evidence cache,
/// revision binding, and the detached last projection snapshot.
#[derive(Debug, Clone)]
pub struct ProjectionService {
    cache: RevisionBoundCache<ModelEvidenceView>,
    last_projection: Option<LastProjection>,
}

impl Default for ProjectionService {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectionService {
    /// Create a fresh service with an empty cache.
    pub fn new() -> Self {
        Self { cache: RevisionBoundCache::new(), last_projection: None }
    }

    /// Current disposable cache size.
    pub fn evidence_cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Bound revision.
    pub fn bound_revision(&self) -> Option<u64> {
        self.cache.bound_revision()
    }

    /// Detached last projection snapshot (disposable, for future /context).
    pub fn last_projection(&self) -> Option<&LastProjection> {
        self.last_projection.as_ref()
    }

    /// Project a Host request into a provider-neutral request.
    ///
    /// Never mutates authoritative Host history or raw tool results.
    pub fn project(&mut self, input: ProjectionInput<'_>) -> ProjectedRequest {
        // Ensure cache revision binding before any view is retained.
        self.cache.ensure_revision(input.task_revision);

        // 1. Context projection first (needed for system tokens and pressure).
        let context_projection = project_segments(input.segments);
        let system_text = serialize_prefix(&context_projection);
        let system_tokens = estimate_tokens(&system_text);

        // 2. Tool-calling compatibility check before building the request.
        if !input.provider_tool_calling && input.mode.requires_tool_calling() {
            let empty_projection = empty_tool_projection();
            let pressure = classify_pressure(
                0,
                input.capacity.working_maximum,
                input.pressure_limits,
            );
            let projected = ProjectedRequest {
                mode: input.mode,
                messages: input.messages.to_vec(),
                tools: Vec::new(),
                system: Some(system_text.clone()),
                pressure,
                tool_projection: empty_projection.clone(),
                context_projection: context_projection.clone(),
                estimated_tokens: 0,
                blocked: Some(BlockedReason::Unsupported(
                    "The selected provider route does not support tool calling, which this task requires; the session cannot proceed with hidden or missing tools."
                        .to_owned(),
                )),
            };
            self.last_projection = Some(LastProjection {
                request: projected.clone(),
                evidence_cache_size: self.cache.len(),
                reduced: false,
                dropped_items: 0,
            });
            return projected;
        }

        // 3. Tool visibility → same visible list for request tools and ApprovedToolSurface.
        let tool_projection = project_tools(ToolProjectionInput {
            registered_tools: input.registered_tools,
            policy: input.policy,
            allowed_tool_names: input.allowed_tool_names.as_deref(),
            mode: input.mode,
        });

        // Tool tokens
        let tool_tokens: usize = tool_projection
            .request_tools
            .iter()
            .map(|def| {
                estimate_tokens(&def.name)
                    + estimate_tokens(&def.description)
                    + estimate_tokens(
                        &serde_json::to_string(&def.input_schema)
                            .unwrap_or_default(),
                    )
            })
            .sum();

        let original_estimated = system_tokens
            + tool_tokens
            + estimate_conversation_tokens(input.messages);
        let mut pressure = classify_pressure(
            original_estimated,
            input.capacity.working_maximum,
            input.pressure_limits,
        );
        let mut messages = input.messages.to_vec();
        let mut reduced = false;
        let mut dropped_items = 0usize;

        if matches!(pressure.state, PressureState::Auto | PressureState::Hard)
        {
            let message_budget = (input.capacity.working_maximum
                - system_tokens as i64
                - tool_tokens as i64)
                .max(0) as usize;
            let trimmed =
                trim_conversation_preserving_pairs(&messages, message_budget);
            if trimmed.dropped_items > 0 {
                reduced = true;
                dropped_items = trimmed.dropped_items;
                messages = trimmed.items;
                let new_estimated =
                    system_tokens + tool_tokens + trimmed.estimated_tokens;
                pressure = classify_pressure(
                    new_estimated,
                    input.capacity.working_maximum,
                    input.pressure_limits,
                );
            }
        }

        // Evidence projection for the disposable request copy.
        let mut projected_messages: Vec<ConversationItem> =
            Vec::with_capacity(messages.len());
        for item in messages {
            match item {
                ConversationItem::ToolResult {
                    call_id,
                    tool_name,
                    result,
                } => {
                    let projected = self.project_tool_result(
                        input.task_revision,
                        &tool_name,
                        &result,
                    );
                    projected_messages.push(ConversationItem::ToolResult {
                        call_id,
                        tool_name,
                        result: projected,
                    });
                }
                other => projected_messages.push(other),
            }
        }

        // TS oracle classifies pressure BEFORE evidence sanitization and
        // does not re-derive it from sanitized projected_messages. Sanitization
        // is a disposable view concern; the already-classified pressure and
        // its estimatedTokens remain authoritative for hard-block reasoning.
        let estimated_tokens = pressure.estimated_tokens;
        let final_pressure = pressure;

        let blocked = if final_pressure.state == PressureState::Hard {
            let reason = format!(
                "Projected context is {} tokens against a working maximum of {}; the provider call was blocked.{}",
                estimated_tokens,
                input.capacity.working_maximum,
                if reduced { " (reduction was already applied)" } else { "" }
            );
            Some(BlockedReason::Hard(reason))
        } else {
            None
        };

        let projected = ProjectedRequest {
            mode: input.mode,
            messages: projected_messages,
            tools: tool_projection.request_tools.clone(),
            system: Some(system_text.clone()),
            pressure: final_pressure,
            tool_projection: tool_projection.clone(),
            context_projection: context_projection.clone(),
            estimated_tokens,
            blocked: blocked.clone(),
        };
        self.last_projection = Some(LastProjection {
            request: projected.clone(),
            evidence_cache_size: self.cache.len(),
            reduced,
            dropped_items,
        });
        projected
    }

    /// Project one tool result through the evidence pipeline with caching.
    fn project_tool_result(
        &mut self,
        revision: Option<u64>,
        tool_name: &str,
        result: &ToolExecutionResult,
    ) -> ToolExecutionResult {
        match result {
            ToolExecutionResult::Success { output, summary } => {
                // Cache key: toolName:sha256(canonical string of output+summary)
                let canonical = format!(
                    "{}:{}",
                    serde_json::to_string(output).unwrap_or_default(),
                    summary
                );
                let key = format!(
                    "{}:{}",
                    tool_name,
                    crate::identity::sha256_hex(canonical.as_bytes())
                );
                if let Some(cached) = self.cache.get(&key) {
                    return ToolExecutionResult::Success {
                        output: output.clone(),
                        summary: cached.text.clone(),
                    };
                }
                let view = project_for_model(
                    None,
                    None,
                    summary,
                    &self.evidence_options_for_revision(revision),
                );
                self.cache.insert(revision, key, view.clone());
                ToolExecutionResult::Success {
                    output: output.clone(),
                    summary: view.text,
                }
            }
            other => {
                let view = project_for_model(
                    None,
                    None,
                    other.message(),
                    &self.evidence_options_for_revision(revision),
                );
                match other {
                    ToolExecutionResult::InvalidInput { .. } => {
                        ToolExecutionResult::InvalidInput {
                            message: view.text,
                        }
                    }
                    ToolExecutionResult::Denied { .. } => {
                        ToolExecutionResult::Denied { message: view.text }
                    }
                    ToolExecutionResult::Conflict { .. } => {
                        ToolExecutionResult::Conflict { message: view.text }
                    }
                    ToolExecutionResult::Failed { .. } => {
                        ToolExecutionResult::Failed { message: view.text }
                    }
                    ToolExecutionResult::Cancelled { .. } => {
                        ToolExecutionResult::Cancelled { message: view.text }
                    }
                    ToolExecutionResult::TimedOut { .. } => {
                        ToolExecutionResult::TimedOut { message: view.text }
                    }
                    ToolExecutionResult::OutputLimit { .. } => {
                        ToolExecutionResult::OutputLimit { message: view.text }
                    }
                    ToolExecutionResult::SandboxDenied { .. } => {
                        ToolExecutionResult::SandboxDenied {
                            message: view.text,
                        }
                    }
                    ToolExecutionResult::SandboxUnavailable { .. } => {
                        ToolExecutionResult::SandboxUnavailable {
                            message: view.text,
                        }
                    }
                    ToolExecutionResult::WorkspaceViolation { .. } => {
                        ToolExecutionResult::WorkspaceViolation {
                            message: view.text,
                        }
                    }
                    ToolExecutionResult::Unavailable { .. } => {
                        ToolExecutionResult::Unavailable { message: view.text }
                    }
                    ToolExecutionResult::Success { .. } => unreachable!(),
                }
            }
        }
    }

    fn evidence_options_for_revision(
        &self,
        _revision: Option<u64>,
    ) -> EvidenceProjectorOptions {
        // Default options; callers that need secrets should use the policy-aware entry.
        EvidenceProjectorOptions::default()
    }
}

fn empty_tool_projection() -> ToolProjection {
    ToolProjection {
        fingerprint: "unsupported".to_owned(),
        tools: Vec::new(),
        counts: visibility::ToolCounts { available: 0, gated: 0, hidden: 0 },
        request_tools: Vec::new(),
        approved_names: Vec::new(),
    }
}

/// Policy-aware projection — the production Host path.
///
/// When the Host supplies a real `PermissionPolicy`, tool visibility respects it.
pub fn project_with_policy(
    cache: &mut RevisionBoundCache<ModelEvidenceView>,
    input: ProjectionInput<'_>,
    policy: &PermissionPolicy,
    evidence_options: EvidenceProjectorOptions,
) -> ProjectedRequest {
    cache.ensure_revision(input.task_revision);
    let context_projection = project_segments(input.segments);
    let system_text = serialize_prefix(&context_projection);
    let system_tokens = estimate_tokens(&system_text);

    if !input.provider_tool_calling && input.mode.requires_tool_calling() {
        let pressure = classify_pressure(
            0,
            input.capacity.working_maximum,
            input.pressure_limits,
        );
        return ProjectedRequest {
            mode: input.mode,
            messages: input.messages.to_vec(),
            tools: Vec::new(),
            system: Some(system_text),
            pressure,
            tool_projection: empty_tool_projection(),
            context_projection,
            estimated_tokens: 0,
            blocked: Some(BlockedReason::Unsupported(
                "The selected provider route does not support tool calling, which this task requires; the session cannot proceed with hidden or missing tools."
                    .to_owned(),
            )),
        };
    }

    let tool_projection = project_tools(ToolProjectionInput {
        registered_tools: input.registered_tools,
        policy,
        allowed_tool_names: input.allowed_tool_names.as_deref(),
        mode: input.mode,
    });
    let tool_tokens: usize = tool_projection
        .request_tools
        .iter()
        .map(|def| {
            estimate_tokens(&def.name)
                + estimate_tokens(&def.description)
                + estimate_tokens(
                    &serde_json::to_string(&def.input_schema)
                        .unwrap_or_default(),
                )
        })
        .sum();

    let original_estimated = system_tokens
        + tool_tokens
        + estimate_conversation_tokens(input.messages);
    let mut pressure = classify_pressure(
        original_estimated,
        input.capacity.working_maximum,
        input.pressure_limits,
    );
    let mut messages = input.messages.to_vec();
    let mut reduced = false;
    if matches!(pressure.state, PressureState::Auto | PressureState::Hard) {
        let message_budget = (input.capacity.working_maximum
            - system_tokens as i64
            - tool_tokens as i64)
            .max(0) as usize;
        let trimmed =
            trim_conversation_preserving_pairs(&messages, message_budget);
        if trimmed.dropped_items > 0 {
            reduced = true;
            messages = trimmed.items;
            let new_estimated =
                system_tokens + tool_tokens + trimmed.estimated_tokens;
            pressure = classify_pressure(
                new_estimated,
                input.capacity.working_maximum,
                input.pressure_limits,
            );
        }
    }

    let mut projected_messages: Vec<ConversationItem> =
        Vec::with_capacity(messages.len());
    for item in messages {
        match item {
            ConversationItem::ToolResult { call_id, tool_name, result } => {
                let projected = project_tool_result_cached(
                    cache,
                    input.task_revision,
                    &tool_name,
                    &result,
                    &evidence_options,
                );
                projected_messages.push(ConversationItem::ToolResult {
                    call_id,
                    tool_name,
                    result: projected,
                });
            }
            other => projected_messages.push(other),
        }
    }
    // Same as above: do not re-derive from sanitized projected_messages.
    let estimated_tokens = pressure.estimated_tokens;
    let final_pressure = pressure;
    let blocked = if final_pressure.state == PressureState::Hard {
        Some(BlockedReason::Hard(format!(
            "Projected context is {} tokens against a working maximum of {}; the provider call was blocked.{}",
            estimated_tokens,
            input.capacity.working_maximum,
            if reduced { " (reduction was already applied)" } else { "" }
        )))
    } else {
        None
    };
    ProjectedRequest {
        mode: input.mode,
        messages: projected_messages,
        tools: tool_projection.request_tools.clone(),
        system: Some(system_text),
        pressure: final_pressure,
        tool_projection,
        context_projection,
        estimated_tokens,
        blocked,
    }
}

fn project_tool_result_cached(
    cache: &mut RevisionBoundCache<ModelEvidenceView>,
    revision: Option<u64>,
    tool_name: &str,
    result: &ToolExecutionResult,
    options: &EvidenceProjectorOptions,
) -> ToolExecutionResult {
    match result {
        ToolExecutionResult::Success { output, summary } => {
            let canonical = format!(
                "{}:{}",
                serde_json::to_string(output).unwrap_or_default(),
                summary
            );
            let key = format!(
                "{}:{}",
                tool_name,
                crate::identity::sha256_hex(canonical.as_bytes())
            );
            if let Some(cached) = cache.get(&key) {
                return ToolExecutionResult::Success {
                    output: output.clone(),
                    summary: cached.text.clone(),
                };
            }
            let view = project_for_model(None, None, summary, options);
            cache.insert(revision, key, view.clone());
            ToolExecutionResult::Success {
                output: output.clone(),
                summary: view.text,
            }
        }
        other => {
            let view = project_for_model(None, None, other.message(), options);
            match other {
                ToolExecutionResult::InvalidInput { .. } => {
                    ToolExecutionResult::InvalidInput { message: view.text }
                }
                ToolExecutionResult::Denied { .. } => {
                    ToolExecutionResult::Denied { message: view.text }
                }
                ToolExecutionResult::Conflict { .. } => {
                    ToolExecutionResult::Conflict { message: view.text }
                }
                ToolExecutionResult::Failed { .. } => {
                    ToolExecutionResult::Failed { message: view.text }
                }
                ToolExecutionResult::Cancelled { .. } => {
                    ToolExecutionResult::Cancelled { message: view.text }
                }
                ToolExecutionResult::TimedOut { .. } => {
                    ToolExecutionResult::TimedOut { message: view.text }
                }
                ToolExecutionResult::OutputLimit { .. } => {
                    ToolExecutionResult::OutputLimit { message: view.text }
                }
                ToolExecutionResult::SandboxDenied { .. } => {
                    ToolExecutionResult::SandboxDenied { message: view.text }
                }
                ToolExecutionResult::SandboxUnavailable { .. } => {
                    ToolExecutionResult::SandboxUnavailable {
                        message: view.text,
                    }
                }
                ToolExecutionResult::WorkspaceViolation { .. } => {
                    ToolExecutionResult::WorkspaceViolation {
                        message: view.text,
                    }
                }
                ToolExecutionResult::Unavailable { .. } => {
                    ToolExecutionResult::Unavailable { message: view.text }
                }
                ToolExecutionResult::Success { .. } => unreachable!(),
            }
        }
    }
}
