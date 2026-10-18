//! Replay recording and response identity for provider calls.
//!
//! Owns [`ReplayHooks`], the request and body digests, and the terminal
//! record projections that persist sanitized provider outcomes.

use super::redaction::redact_body_with_secret;

/// Hooks for determinism replay recording of provider HTTP responses.
///
/// Holds an optional clock for `observed_at_ms` and an optional recorder for
/// the response identity. The struct is `pub(crate)` and intentionally keeps
/// providers' derived `Debug` intact via a manual `Debug` impl.
#[derive(Default)]
pub(crate) struct ReplayHooks {
    /// Clock for `observed_at_ms` when recording.
    pub clock: Option<std::rc::Rc<dyn siralos_core::determinism::Clock>>,
    /// Recorder for the response identity.
    pub recorder:
        Option<std::rc::Rc<dyn siralos_core::determinism::ReplayRecorder>>,
    /// Canonical digest of the request currently being served, if any.
    pub request_sha256: core::cell::RefCell<Option<String>>,
    /// Display-only usage observation from the last terminal outcome.
    ///
    /// Set on a replayable success and cleared on every other terminal
    /// record so a later round can never be attributed an earlier round's
    /// usage. Never persisted, digested, or consulted by replay matching.
    pub usage_observation: core::cell::RefCell<
        Option<
            siralos_core::determinism::provider_replay::ProviderUsageObservation,
        >,
    >,
    /// Wall-clock start of the provider round currently in flight.
    ///
    /// Set when a provider is about to send its request and consumed by
    /// that round's terminal record. Display-only: never persisted,
    /// digested, or consulted by replay matching.
    pub round_started: core::cell::Cell<Option<std::time::Instant>>,
    /// Display-only wall time of the last replayable round, milliseconds.
    ///
    /// Computed at the same terminal record that sets `usage_observation`
    /// and cleared wherever that observation is cleared, so the two can be
    /// taken as one round's facts.
    pub round_millis: core::cell::Cell<Option<u64>>,
}

impl ReplayHooks {
    /// Take the display-only usage observation recorded at the last
    /// terminal outcome, clearing it so one call is attributed once.
    #[must_use]
    pub(crate) fn take_usage_observation(
        &self,
    ) -> Option<
        siralos_core::determinism::provider_replay::ProviderUsageObservation,
    > {
        self.usage_observation.borrow_mut().take()
    }

    /// Start the display-only round timer, called immediately before a
    /// provider issues its HTTP request.
    pub(crate) fn begin_round(&self) {
        self.round_started.set(Some(std::time::Instant::now()));
    }

    /// Take the display-only wall time recorded at the last terminal
    /// outcome, clearing it so one round is attributed once.
    #[must_use]
    pub(crate) fn take_round_millis(&self) -> Option<u64> {
        self.round_millis.take()
    }
}

impl std::fmt::Debug for ReplayHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.clock.is_some() || self.recorder.is_some() {
            f.write_str("ReplayHooks(present)")
        } else {
            f.write_str("ReplayHooks(absent)")
        }
    }
}

/// Compute a stable, content-derived digest of a provider request. The raw
/// request is never retained; only this digest is attached to a recording.
pub(crate) fn request_sha256(
    request: &siralos_core::provider::ModelRequest,
) -> String {
    use siralos_core::provider::conversation::ConversationItem;
    use siralos_core::provider::result::ToolExecutionResult;

    let messages = request
        .messages
        .iter()
        .map(|item| match item {
            ConversationItem::UserMessage { content } => {
                serde_json::json!({"kind": "user", "content": content})
            }
            ConversationItem::AssistantMessage { content } => {
                serde_json::json!({"kind": "assistant", "content": content})
            }
            ConversationItem::AssistantToolCall {
                call_id,
                tool_name,
                input,
            } => {
                let input = match input {
                    siralos_core::provider::conversation::AssistantToolCallInput::Present(value) => {
                        serde_json::json!({"present": true, "value": value})
                    }
                    siralos_core::provider::conversation::AssistantToolCallInput::Omitted => {
                        serde_json::json!({"present": false})
                    }
                };
                serde_json::json!({
                    "kind": "assistant_tool_call",
                    "call_id": call_id,
                    "tool_name": tool_name,
                    "input": input,
                })
            }
            ConversationItem::ToolResult { call_id, tool_name, result } => {
                let result = match result {
                    ToolExecutionResult::Success { output, summary } => {
                        serde_json::json!({
                            "status": "success",
                            "output": output,
                            "summary": summary,
                        })
                    }
                    other => serde_json::json!({
                        "status": other.status_str(),
                        "message": other.message(),
                    }),
                };
                serde_json::json!({
                    "kind": "tool_result",
                    "call_id": call_id,
                    "tool_name": tool_name,
                    "result": result,
                })
            }
        })
        .collect::<Vec<_>>();
    let tools = request
        .tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema,
            })
        })
        .collect::<Vec<_>>();
    let canonical = serde_json::json!({
        "messages": messages,
        "tools": tools,
        "system": request.system,
    });
    let bytes = serde_json::to_vec(&canonical)
        .expect("serde_json::Value is always serializable");
    siralos_core::identity::sha256_hex(&bytes)
}

/// Bind a request digest to the effective provider route. The route is
/// deliberately credential-free; model remains a live label and is not part
/// of playback identity.
pub(crate) fn request_sha256_for_route(
    request: &siralos_core::provider::ModelRequest,
    provider_id: &str,
    endpoint: &str,
    protocol: &str,
) -> String {
    let canonical = serde_json::json!({
        "request": request_sha256(request),
        "provider": provider_id,
        "endpoint": endpoint,
        "protocol": protocol,
    });
    let bytes = serde_json::to_vec(&canonical)
        .expect("serde_json::Value is always serializable");
    siralos_core::identity::sha256_hex(&bytes)
}

/// Compute the `sha256` hex of the sanitized bounded body text.
pub(crate) fn response_body_sha256(text: &str) -> String {
    siralos_core::identity::sha256_hex(text.as_bytes())
}

fn response_identity(
    hooks: &ReplayHooks,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
) -> siralos_core::determinism::ProviderResponseIdentity {
    let body_sha256 = response_body_sha256(body_text);
    let observed_at_ms = hooks.clock.as_ref().map(|clock| clock.now_ms());
    let usage =
        siralos_core::determinism::provider_replay::parse_provider_usage(
            body_text,
        );
    siralos_core::determinism::ProviderResponseIdentity {
        provider_id: provider_id.to_owned(),
        model: model.to_owned(),
        status,
        body_sha256,
        body_bytes: body_text.len() as u64,
        observed_at_ms,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cached_tokens: usage.cached_tokens,
    }
}

fn record_evidence_identity(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    identity: &siralos_core::determinism::ProviderResponseIdentity,
    body_text: &str,
) {
    if let Some(recorder) = hooks.recorder.as_ref() {
        let body_retained = recorder
            .try_record_provider_response_with_body_as_evidence(
                identity, body_text,
            );
        *last_replay.borrow_mut() =
            siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                reason: if body_retained && !body_text.is_empty() {
                    "response is not a successful replayable completion"
                        .to_owned()
                } else {
                    "replay body was not retained".to_owned()
                },
            };
    } else {
        *last_replay.borrow_mut() =
            siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                reason: "live call not recorded".to_owned(),
            };
    }
}

/// Record a provider outcome as terminal evidence, bypassing the replay-shape
/// classifier. Use this for provider-specific protocol/validation failures.
pub(crate) fn record_evidence_outcome(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
) {
    *hooks.usage_observation.borrow_mut() = None;
    hooks.round_started.set(None);
    hooks.round_millis.set(None);
    let identity =
        response_identity(hooks, provider_id, model, status, body_text);
    record_evidence_identity(hooks, last_replay, &identity, body_text);
}

/// Record terminal evidence after exact-credential redaction.
pub(crate) fn record_evidence_outcome_with_secret(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
    secret: Option<&str>,
) {
    let safe_body = redact_body_with_secret(body_text, secret);
    record_evidence_outcome(
        hooks,
        last_replay,
        provider_id,
        model,
        status,
        &safe_body,
    );
}

/// Record one provider outcome after exact-credential redaction. The
/// unredacted body is never handed to the recorder.
pub(crate) fn record_outcome_with_secret(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
    secret: Option<&str>,
) {
    let safe_body = redact_body_with_secret(body_text, secret);
    record_outcome(hooks, last_replay, provider_id, model, status, &safe_body);
}

/// Record one provider HTTP outcome for replay.
///
/// `body_text` must be the sanitized bounded text (never the credential).
/// A successful response sets `Recorded` only after the recorder accepts its
/// body; a request digest, when present, is retained through the route-bound
/// operation. A low-level caller with no digest uses the explicit legacy body
/// method, so `Recorded` there means legacy playback only, not route binding.
/// Non-replayable or rejected outcomes remain `Unavailable`.
pub(crate) fn record_outcome(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
) {
    record_outcome_bound(
        hooks,
        last_replay,
        provider_id,
        model,
        status,
        body_text,
        None,
    );
}

fn record_outcome_bound(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
    request_sha256_override: Option<&str>,
) {
    let body_sha256 = response_body_sha256(body_text);
    let body_bytes = body_text.len() as u64;
    let observed_at_ms = hooks.clock.as_ref().map(|c| c.now_ms());
    let usage =
        siralos_core::determinism::provider_replay::parse_provider_usage(
            body_text,
        );
    let identity = siralos_core::determinism::ProviderResponseIdentity {
        provider_id: provider_id.to_owned(),
        model: model.to_owned(),
        status,
        body_sha256,
        body_bytes,
        observed_at_ms,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cached_tokens: usage.cached_tokens,
    };
    let replayable = status.is_some_and(|status| (200..300).contains(&status))
        && !body_text.is_empty()
        && !crate::provider::replay::completion_events_from_body(body_text)
            .iter()
            .any(|event| {
                matches!(
                    event,
                    siralos_core::provider::ProviderEvent::Failed(_)
                )
            });
    let round_millis = hooks.round_started.take().map(|started| {
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
    });
    *hooks.usage_observation.borrow_mut() = if replayable {
        siralos_core::determinism::provider_replay::parse_provider_usage_observation(
            body_text,
        )
    } else {
        None
    };
    hooks.round_millis.set(if replayable { round_millis } else { None });
    if !replayable {
        if let Some(recorder) = hooks.recorder.as_ref() {
            // Preserve sanitized terminal evidence in the recorder, but keep
            // availability unavailable: the shared persistence projection
            // filters it from the replay-only store. Evidence has its own
            // bounded budget, so replay saturation must not suppress it.
            let body_retained = recorder
                .try_record_provider_response_with_body_as_evidence(
                    &identity, body_text,
                );
            *last_replay.borrow_mut() =
                siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                    reason: if body_retained && !body_text.is_empty() {
                        "response is not a successful replayable completion"
                            .to_owned()
                    } else {
                        "replay body was not retained".to_owned()
                    },
                };
        } else {
            *last_replay.borrow_mut() =
                siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                    reason: "live call not recorded".to_owned(),
                };
        }
        return;
    }
    if hooks.recorder.as_ref().is_some_and(|r| r.is_recording()) {
        if let Some(recorder) = hooks.recorder.as_ref() {
            // A digest is bound whenever the normal provider stream path
            // prepared the route. Low-level legacy callers may invoke the
            // transport helper without that preparation; retain their body via
            // the explicitly legacy method instead of manufacturing a binding.
            let request_sha256 = request_sha256_override
                .map(str::to_owned)
                .or_else(|| hooks.request_sha256.borrow().clone());
            let body_recorded = if let Some(request_sha256) = request_sha256 {
                recorder.try_record_provider_response_with_body_and_request(
                    &identity,
                    body_text,
                    &request_sha256,
                )
            } else {
                recorder.try_record_provider_response_with_body(
                    &identity, body_text,
                )
            };
            if !body_recorded {
                *last_replay.borrow_mut() =
                    siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                        reason: "replay body was not retained".to_owned(),
                    };
                return;
            }
        }
        let digest =
            siralos_core::determinism::compute_provider_response_identity_digest(
                &identity,
            );
        match digest {
            Ok(digest) => {
                *last_replay.borrow_mut() =
                    siralos_core::determinism::ProviderReplayAvailability::Recorded {
                        digest,
                    };
            }
            Err(_) => {
                *last_replay.borrow_mut() =
                    siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                        reason: "response identity digest failed".to_owned(),
                    };
            }
        }
    } else {
        *last_replay.borrow_mut() =
            siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                reason: "live call not recorded".to_owned(),
            };
    }
}

/// Record one provider outcome after exact-credential redaction, binding the
/// request digest this turn computed rather than the shared hook cell. The
/// streaming turn owns its digest so two interleaved turns cannot persist
/// response A under request B.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_outcome_with_secret_and_request_digest(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
    secret: Option<&str>,
    request_sha256: &str,
) {
    let safe_body = redact_body_with_secret(body_text, secret);
    record_outcome_bound(
        hooks,
        last_replay,
        provider_id,
        model,
        status,
        &safe_body,
        Some(request_sha256),
    );
}
