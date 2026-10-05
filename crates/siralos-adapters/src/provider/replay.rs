//! Session replay-run composition for decision 77 — bounded record-then-replay.
//!
//! The in-process recorder retains credential-free response bodies in memory.
//! The separate [`crate::replay_store`] may persist only the bounded, sanitized
//! body shape after the same validation gates; this module never writes files
//! itself.

use crate::provider::CANCELLED_BEFORE_PROVIDER_START;
use crate::provider::tool_names::ToolNames;
use serde_json::Value;
use siralos_core::determinism::ReplayRecording;
use siralos_core::provider::{
    CancellationSignal, ModelEvent, ModelProvider, ModelRequest, ProviderEvent,
};
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) fn valid_replay_identifier(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && !value.trim().is_empty()
        && value.len() <= max_bytes
        && !value.chars().any(char::is_control)
}

fn reject_unknown_fields(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<(), String> {
    if object.keys().any(|field| !allowed.contains(&field.as_str())) {
        return Err("replay response contains an unsupported field".to_owned());
    }
    Ok(())
}

fn valid_success_status(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "success" | "succeeded" | "complete" | "completed" | "ok"
    )
}

fn valid_openai_finish_reason(reason: &str) -> bool {
    matches!(
        reason.to_ascii_lowercase().as_str(),
        "stop"
            | "length"
            | "tool_calls"
            | "function_call"
            | "content_filter"
            | "stop_sequence"
    )
}

fn valid_anthropic_stop_reason(reason: &str) -> bool {
    matches!(
        reason.to_ascii_lowercase().as_str(),
        "end_turn"
            | "max_tokens"
            | "stop_sequence"
            | "tool_use"
            | "pause_turn"
    )
}

/// A bounded replay recording failed one of the shared validation gates.
///
/// The variants are intentionally coarse: callers map them to their own
/// public error domains, while the writer, loader, and playback constructor
/// all make the same accept/reject decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayRecordingValidationError {
    /// Provider/model/status identity is outside the replay contract.
    InvalidIdentity,
    /// The body is larger than the provider response bound.
    BodyTooLarge,
    /// The recorded byte count disagrees with the retained body.
    BodyBytes,
    /// The body digest is malformed or does not match the body.
    BodyDigest,
    /// The optional request digest is malformed.
    RequestDigest,
    /// The body is not a supported replay response shape.
    BodyShape,
    /// Output text contains a control character replay cannot safely emit.
    BodyControl,
}

impl std::fmt::Display for ReplayRecordingValidationError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::InvalidIdentity => formatter.write_str("invalid replay identity"),
            Self::BodyTooLarge => formatter.write_str("replay body is too large"),
            Self::BodyBytes => {
                formatter.write_str("replay body byte count does not match")
            }
            Self::BodyDigest => {
                formatter.write_str("replay body digest does not match")
            }
            Self::RequestDigest => {
                formatter.write_str("replay request digest is invalid")
            }
            Self::BodyShape => {
                formatter.write_str("replay body has an unsupported shape")
            }
            Self::BodyControl => formatter.write_str(
                "replay response text contains an unsupported control character",
            ),
        }
    }
}

/// Validate the bounded response shape shared by store persistence and replay
/// playback.
///
/// This function is the single gate for body shape and output-control
/// validation. Keeping it next to the event parser prevents a store from
/// accepting a body that playback would later reject.
pub(crate) fn validate_replay_body_text(
    body: &str,
) -> Result<(), ReplayRecordingValidationError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| ReplayRecordingValidationError::BodyShape)?;
    if !value.is_object() {
        return Err(ReplayRecordingValidationError::BodyShape);
    }
    validate_replay_shape(&value)
        .map_err(|_| ReplayRecordingValidationError::BodyShape)?;
    if !replay_text_fields_valid(&value) {
        return Err(ReplayRecordingValidationError::BodyControl);
    }
    Ok(())
}

/// Validate one detached `ReplayRecording` using the same gates as the
/// persisted-store writer and loader.
///
/// `expected_provider_id` is supplied by route-bound playback. The persisted
/// store has no active route and therefore passes `None`; it still validates
/// the recording's own provider identity.
pub(crate) fn validate_replay_recording(
    recording: &ReplayRecording,
    expected_provider_id: Option<&str>,
) -> Result<(), ReplayRecordingValidationError> {
    let identity = &recording.identity;
    let provider_matches = expected_provider_id
        .is_none_or(|provider| provider == identity.provider_id);
    if identity.validate().is_err()
        || !provider_matches
        || !valid_replay_identifier(&identity.provider_id, 256)
        || !valid_replay_identifier(&identity.model, 256)
        || !identity.status.is_some_and(|status| (200..300).contains(&status))
    {
        return Err(ReplayRecordingValidationError::InvalidIdentity);
    }
    if recording.body.len() > crate::provider::MAX_RESPONSE_BYTES {
        return Err(ReplayRecordingValidationError::BodyTooLarge);
    }
    if identity.body_bytes != recording.body.len() as u64 {
        return Err(ReplayRecordingValidationError::BodyBytes);
    }
    let body_digest = identity.body_sha256.to_ascii_lowercase();
    if body_digest.len() != 64
        || !body_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        || body_digest
            != crate::provider::response_body_sha256(&recording.body)
    {
        return Err(ReplayRecordingValidationError::BodyDigest);
    }
    if recording.request_sha256.as_deref().is_some_and(|digest| {
        digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err(ReplayRecordingValidationError::RequestDigest);
    }
    validate_replay_body_text(&recording.body)
}

pub(crate) fn validate_replay_shape(value: &Value) -> Result<(), String> {
    let envelope = value
        .as_object()
        .ok_or_else(|| "replay recording is not a JSON object".to_owned())?;
    reject_unknown_fields(
        envelope,
        &[
            "id",
            "type",
            "role",
            "object",
            "created",
            "created_at",
            "model",
            "choices",
            "content",
            "output",
            "usage",
            "status",
            "stop_reason",
            "stop_sequence",
            "error",
            "system_fingerprint",
            "service_tier",
            "container",
            "response_id",
            "instructions",
            "warnings",
            "metadata",
            "index",
            "parallel_tool_calls",
            "tool_choice",
            "tools",
            "previous_response_id",
            "reasoning",
            "max_output_tokens",
            "truncation",
            "text",
            "include",
            "temperature",
            "top_p",
            "store",
            "user",
            "prompt_cache_key",
            "background",
            "max_tool_calls",
            "anthropic_version",
        ],
    )?;
    if let Some(kind) = value.get("type") {
        if kind.as_str() != Some("message") {
            return Err("replay response type is invalid".to_owned());
        }
    }
    if let Some(role) = value.get("role") {
        if role.as_str() != Some("assistant") {
            return Err("replay response role is invalid".to_owned());
        }
    }
    if let Some(stop_sequence) = value.get("stop_sequence") {
        if !stop_sequence.is_null()
            && !stop_sequence
                .as_str()
                .is_some_and(|value| valid_replay_identifier(value, 256))
        {
            return Err("replay response stop_sequence is invalid".to_owned());
        }
    }
    if value.get("error").is_some() {
        return Err("replay recording contains an explicit error".to_owned());
    }
    if let Some(status) = value.get("status") {
        match status {
            Value::String(status) => {
                if !valid_success_status(status) {
                    return Err(
                        "replay response reports a non-success status"
                            .to_owned(),
                    );
                }
            }
            Value::Number(status) => {
                let successful = status
                    .as_u64()
                    .is_some_and(|value| (200..300).contains(&value));
                if !successful {
                    return Err(
                        "replay response reports a non-success status"
                            .to_owned(),
                    );
                }
            }
            Value::Null => {}
            _ => {
                return Err(
                    "replay response status has an invalid type".to_owned()
                );
            }
        }
    }
    if let Some(stop_reason) = value.get("stop_reason") {
        if !stop_reason.is_null() {
            let reason = stop_reason.as_str().ok_or_else(|| {
                "replay response stop_reason must be text".to_owned()
            })?;
            if !valid_anthropic_stop_reason(reason) {
                return Err(
                    "replay response reports an unsupported stop reason"
                        .to_owned(),
                );
            }
        }
    }
    let recognized_shapes = ["choices", "content", "output"]
        .into_iter()
        .filter(|field| value.get(*field).is_some())
        .count();
    if recognized_shapes > 1 {
        return Err(
            "replay response contains multiple provider shapes".to_owned()
        );
    }
    if let Some(choices) = value.get("choices") {
        let choices = choices
            .as_array()
            .ok_or_else(|| "replay choices must be an array".to_owned())?;
        if choices.is_empty() {
            return Err("replay recording contains no choices".to_owned());
        }
        let mut usable = false;
        for choice in choices {
            let choice_object = choice
                .as_object()
                .ok_or_else(|| "replay choice must be an object".to_owned())?;
            reject_unknown_fields(
                choice_object,
                &["index", "message", "finish_reason", "logprobs"],
            )?;
            if choice.get("error").is_some() {
                return Err(
                    "replay choice contains an explicit error".to_owned()
                );
            }
            if let Some(reason) = choice.get("finish_reason") {
                if !reason.is_null() {
                    let reason = reason.as_str().ok_or_else(|| {
                        "replay choice finish_reason must be text".to_owned()
                    })?;
                    if !valid_openai_finish_reason(reason) {
                        return Err(
                            "replay choice reports an unsupported finish reason"
                                .to_owned(),
                        );
                    }
                }
            }
            let message =
                choice.get("message").and_then(Value::as_object).ok_or_else(
                    || "replay choice needs an object message".to_owned(),
                )?;
            if !validate_openai_message(message)? {
                return Err(
                    "replay choice contains no usable completion event"
                        .to_owned(),
                );
            }
            usable = true;
        }
        return if usable {
            Ok(())
        } else {
            Err("replay recording contains no usable completion event"
                .to_owned())
        };
    }
    if let Some(content) = value.get("content") {
        let content = content
            .as_array()
            .ok_or_else(|| "replay content must be an array".to_owned())?;
        if content.is_empty() {
            return Err(
                "replay recording contains no content blocks".to_owned()
            );
        }
        let mut usable = false;
        for block in content {
            let object = block.as_object().ok_or_else(|| {
                "replay content block must be an object".to_owned()
            })?;
            let kind = object.get("type").and_then(Value::as_str).ok_or_else(
                || "replay content block needs a type".to_owned(),
            )?;
            match kind {
                "text" | "output_text" | "input_text" => {
                    reject_unknown_fields(
                        object,
                        &["type", "text", "annotations", "citations"],
                    )?;
                    let text = object
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                        .ok_or_else(|| {
                            "replay text block needs non-empty text".to_owned()
                        })?;
                    usable |= !text.is_empty();
                }
                "tool_use" | "function_call" => {
                    reject_unknown_fields(
                        object,
                        &[
                            "type",
                            "id",
                            "call_id",
                            "name",
                            "input",
                            "arguments",
                            "cache_control",
                            "stop_reason",
                            "index",
                            "text",
                        ],
                    )?;
                    if object.get("input").is_some()
                        && object.get("arguments").is_some()
                    {
                        return Err(
                            "replay tool_use must not declare both input and arguments"
                                .to_owned(),
                        );
                    }
                    let id = object
                        .get("id")
                        .or_else(|| object.get("call_id"))
                        .and_then(Value::as_str);
                    let name = object.get("name").and_then(Value::as_str);
                    if !id.is_some_and(|id| valid_replay_identifier(id, 256))
                        || !name.is_some_and(|name| {
                            valid_replay_identifier(
                                name,
                                crate::provider::tool_names::MAX_PROVIDER_TOOL_NAME_BYTES,
                            )
                        })
                        || (object.get("input").is_none()
                            && object.get("arguments").is_none())
                    {
                        return Err(
                            "replay tool_use block is incomplete".to_owned()
                        );
                    }
                    if let Some(arguments) = object.get("arguments") {
                        let arguments =
                            arguments.as_str().ok_or_else(|| {
                                "replay tool_use arguments must be JSON text"
                                    .to_owned()
                            })?;
                        let parsed: Value = serde_json::from_str(arguments)
                            .map_err(|_| {
                                "replay tool_use arguments are not JSON"
                                    .to_owned()
                            })?;
                        if !parsed.is_object() {
                            return Err(
                                "replay tool_use arguments must be an object"
                                    .to_owned(),
                            );
                        }
                    } else if !object
                        .get("input")
                        .is_some_and(Value::is_object)
                    {
                        return Err("replay tool_use input must be an object"
                            .to_owned());
                    }
                    usable = true;
                }
                "thinking" => {
                    reject_unknown_fields(
                        object,
                        &["type", "thinking", "signature"],
                    )?;
                    if !object
                        .get("thinking")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.is_empty())
                    {
                        return Err(
                            "replay thinking block needs non-empty text"
                                .to_owned(),
                        );
                    }
                    usable = true;
                }
                "redacted_thinking" => {
                    reject_unknown_fields(object, &["type", "data"])?;
                    if !object.get("data").is_some_and(Value::is_string) {
                        return Err(
                            "replay redacted thinking block is incomplete"
                                .to_owned(),
                        );
                    }
                    // This is intentionally not replayed as model text; it is
                    // safe to ignore but cannot be the only successful event.
                }
                // Unknown provider content is not replayable.  Silently
                // skipping it would let a recording with a recognized text
                // block plus a malformed/unsupported block produce a false
                // success, so the whole recording is rejected.
                _ => {
                    return Err("replay content block type is not supported"
                        .to_owned());
                }
            }
        }
        return if usable {
            Ok(())
        } else {
            Err("replay recording contains no usable content block".to_owned())
        };
    }
    if let Some(output) = value.get("output") {
        let output = output
            .as_array()
            .ok_or_else(|| "replay output must be an array".to_owned())?;
        if output.is_empty() {
            return Err("replay recording contains no output items".to_owned());
        }
        let mut usable = false;
        for item in output {
            let object = item.as_object().ok_or_else(|| {
                "replay output item must be an object".to_owned()
            })?;
            let item_type = object
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| "replay output item needs a type".to_owned())?;
            if let Some(status) = object.get("status") {
                if !status.is_null() {
                    let status = status.as_str().ok_or_else(|| {
                        "replay output item status must be text".to_owned()
                    })?;
                    if !valid_success_status(status) {
                        return Err(
                            "replay output item reports a non-success status"
                                .to_owned(),
                        );
                    }
                }
            }
            match item_type {
                "message" => {
                    reject_unknown_fields(
                        object,
                        &["type", "id", "status", "role", "content", "index"],
                    )?;
                    if object.get("role").is_some_and(|role| {
                        !role.is_null() && role.as_str() != Some("assistant")
                    }) {
                        return Err(
                            "replay output message role is invalid".to_owned()
                        );
                    }
                    let content = object.get("content").ok_or_else(|| {
                        "replay output message needs content".to_owned()
                    })?;
                    let blocks = content.as_array().ok_or_else(|| {
                        "replay output content must be an array".to_owned()
                    })?;
                    if blocks.is_empty() {
                        return Err(
                            "replay output message has no content blocks"
                                .to_owned(),
                        );
                    }
                    let mut item_usable = false;
                    for block in blocks {
                        let block_object =
                            block.as_object().ok_or_else(|| {
                                "replay output block must be an object"
                                    .to_owned()
                            })?;
                        let kind = block_object
                            .get("type")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                "replay output block needs a type".to_owned()
                            })?;
                        match kind {
                            "text" | "output_text" | "input_text" => {
                                reject_unknown_fields(
                                    block_object,
                                    &[
                                        "type",
                                        "text",
                                        "annotations",
                                        "citations",
                                    ],
                                )?;
                                if !block_object
                                    .get("text")
                                    .and_then(Value::as_str)
                                    .is_some_and(|text| !text.is_empty())
                                {
                                    return Err(
                                        "replay output text block needs non-empty text"
                                            .to_owned(),
                                    );
                                }
                                item_usable = true;
                            }
                            "thinking" => {
                                reject_unknown_fields(
                                    block_object,
                                    &["type", "thinking", "signature"],
                                )?;
                                if !block_object
                                    .get("thinking")
                                    .and_then(Value::as_str)
                                    .is_some_and(|text| !text.is_empty())
                                {
                                    return Err(
                                        "replay output thinking block needs non-empty text"
                                            .to_owned(),
                                    );
                                }
                                item_usable = true;
                            }
                            "redacted_thinking" => {
                                reject_unknown_fields(
                                    block_object,
                                    &["type", "data"],
                                )?;
                                if !block_object
                                    .get("data")
                                    .is_some_and(Value::is_string)
                                {
                                    return Err(
                                        "replay output redacted thinking block is incomplete"
                                            .to_owned(),
                                    );
                                }
                            }
                            _ => {
                                return Err(
                                    "replay output content block type is not supported"
                                        .to_owned(),
                                );
                            }
                        }
                    }
                    if !item_usable {
                        return Err(
                            "replay output message has no usable content"
                                .to_owned(),
                        );
                    }
                    usable = true;
                }
                "function_call" => {
                    reject_unknown_fields(
                        object,
                        &[
                            "type",
                            "id",
                            "call_id",
                            "name",
                            "arguments",
                            "status",
                            "index",
                        ],
                    )?;
                    if object.get("id").is_some()
                        && object.get("call_id").is_some()
                    {
                        return Err(
                            "replay function call declares two identifiers"
                                .to_owned(),
                        );
                    }
                    let id = object
                        .get("call_id")
                        .or_else(|| object.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let name = object
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let arguments = object
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !valid_replay_identifier(id, 256)
                        || !valid_replay_identifier(
                            name,
                            crate::provider::tool_names::MAX_PROVIDER_TOOL_NAME_BYTES,
                        )
                        || arguments.trim().is_empty()
                    {
                        return Err(
                            "replay function call is incomplete".to_owned()
                        );
                    }
                    let parsed: Value = serde_json::from_str(arguments)
                        .map_err(|_| {
                            "replay function call arguments are not JSON"
                                .to_owned()
                        })?;
                    if !parsed.is_object() {
                        return Err(
                            "replay function call arguments must be an object"
                                .to_owned(),
                        );
                    }
                    usable = true;
                }
                "reasoning" => {
                    reject_unknown_fields(
                        object,
                        &[
                            "type",
                            "id",
                            "summary",
                            "encrypted_content",
                            "status",
                            "index",
                        ],
                    )?;
                    if let Some(encrypted) = object.get("encrypted_content") {
                        if !encrypted.is_string() {
                            return Err(
                                "replay reasoning encrypted_content must be text"
                                    .to_owned(),
                            );
                        }
                    }
                    if let Some(summary) = object.get("summary") {
                        let summary = summary.as_array().ok_or_else(|| {
                            "replay reasoning summary must be an array"
                                .to_owned()
                        })?;
                        for part in summary {
                            let part = part.as_object().ok_or_else(|| {
                                "replay reasoning summary part must be an object"
                                    .to_owned()
                            })?;
                            reject_unknown_fields(part, &["type", "text"])?;
                            let part_type =
                                part.get("type").and_then(Value::as_str);
                            if part_type != Some("summary_text")
                                || !part
                                    .get("text")
                                    .and_then(Value::as_str)
                                    .is_some_and(|text| !text.is_empty())
                            {
                                return Err(
                                    "replay reasoning summary is malformed"
                                        .to_owned(),
                                );
                            }
                        }
                    }
                }
                _ => {
                    return Err(
                        "replay output item type is not supported".to_owned()
                    );
                }
            }
        }
        return if usable {
            Ok(())
        } else {
            Err("replay recording contains no usable output item".to_owned())
        };
    }
    Err("replay recording has no recognized response shape".to_owned())
}

fn validate_openai_message(
    message: &serde_json::Map<String, Value>,
) -> Result<bool, String> {
    reject_unknown_fields(
        message,
        &[
            "role",
            "content",
            "reasoning",
            "tool_calls",
            "name",
            "annotations",
            "refusal",
            "audio",
            "function_call",
        ],
    )?;
    if message.get("role").is_some_and(|role| {
        !role.is_null() && role.as_str() != Some("assistant")
    }) {
        return Err("replay message role is invalid".to_owned());
    }
    if message.get("refusal").is_some_and(|refusal| !refusal.is_null()) {
        return Err("replay message contains a refusal".to_owned());
    }
    if message
        .get("function_call")
        .is_some_and(|function_call| !function_call.is_null())
    {
        return Err(
            "replay message contains an unsupported function_call".to_owned()
        );
    }
    if message.get("audio").is_some_and(|audio| !audio.is_null()) {
        return Err("replay message contains unsupported audio".to_owned());
    }
    let mut usable = false;
    match message.get("content") {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) => {
            if text.is_empty() {
                return Err("replay message content is empty".to_owned());
            }
            usable = true;
        }
        Some(Value::Array(blocks)) => {
            if blocks.is_empty() {
                return Err("replay message content array is empty".to_owned());
            }
            for block in blocks {
                let object = block.as_object().ok_or_else(|| {
                    "replay content block must be an object".to_owned()
                })?;
                let kind =
                    object.get("type").and_then(Value::as_str).ok_or_else(
                        || "replay content block needs a type".to_owned(),
                    )?;
                if matches!(kind, "text" | "output_text" | "input_text") {
                    reject_unknown_fields(
                        object,
                        &["type", "text", "annotations", "citations"],
                    )?;
                    let text = object
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                        .ok_or_else(|| {
                            "replay content block needs non-empty text"
                                .to_owned()
                        })?;
                    usable |= !text.is_empty();
                } else {
                    return Err("replay content block type is not supported"
                        .to_owned());
                }
            }
        }
        Some(_) => {
            return Err(
                "replay message content has an invalid shape".to_owned()
            );
        }
    }
    if let Some(reasoning) = message.get("reasoning") {
        if !reasoning.is_null() {
            let text = reasoning.as_str().ok_or_else(|| {
                "replay message reasoning must be a string".to_owned()
            })?;
            if text.is_empty() {
                return Err("replay message reasoning is empty".to_owned());
            }
            usable = true;
        }
    }
    if let Some(calls) = message.get("tool_calls") {
        let calls = calls
            .as_array()
            .ok_or_else(|| "replay tool_calls must be an array".to_owned())?;
        for call in calls {
            let call = call.as_object().ok_or_else(|| {
                "replay tool call must be an object".to_owned()
            })?;
            reject_unknown_fields(call, &["id", "type", "function", "index"])?;
            let call_type = call
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| "replay tool call needs a type".to_owned())?;
            if call_type != "function" {
                return Err(
                    "replay tool call type is not supported".to_owned()
                );
            }
            let function =
                call.get("function").and_then(Value::as_object).ok_or_else(
                    || "replay tool call needs a function object".to_owned(),
                )?;
            reject_unknown_fields(function, &["name", "arguments"])?;
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "replay tool call needs an id".to_owned())?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "replay tool call needs a name".to_owned())?;
            let arguments =
                function.get("arguments").and_then(Value::as_str).ok_or_else(
                    || "replay tool call needs arguments text".to_owned(),
                )?;
            if !valid_replay_identifier(id, 256)
                || !valid_replay_identifier(
                    name,
                    crate::provider::tool_names::MAX_PROVIDER_TOOL_NAME_BYTES,
                )
                || arguments.trim().is_empty()
            {
                return Err(
                    "replay tool call is missing id, name, or arguments"
                        .to_owned(),
                );
            }
            let parsed: Value =
                serde_json::from_str(arguments).map_err(|_| {
                    "replay tool call arguments are not JSON".to_owned()
                })?;
            if !parsed.is_object() {
                return Err(
                    "replay tool call arguments must be an object".to_owned()
                );
            }
            if name.len()
                > crate::provider::tool_names::MAX_PROVIDER_TOOL_NAME_BYTES
                || name.chars().any(char::is_control)
            {
                return Err("replay tool call name is invalid".to_owned());
            }
            usable = true;
        }
    }
    Ok(usable)
}

/// Convert a sanitized bounded body text into `ProviderEvent`s.
///
/// Mirrors the tail of `GenericProvider::call_generic`: OpenAI `choices` shape
/// first, then the Anthropic `content`/`tool_use` fallback shape, then the
/// empty-events fallback that pushes an empty `TextDelta`, and finally a
/// `Completed` event.
fn valid_replay_text(text: &str) -> bool {
    text.chars().all(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
}

fn normalize_replay_event(event: ProviderEvent) -> ProviderEvent {
    match event {
        ProviderEvent::Event(ModelEvent::TextDelta { text }) => {
            ProviderEvent::Event(ModelEvent::TextDelta {
                text: text.replace('\r', ""),
            })
        }
        ProviderEvent::Event(ModelEvent::ReasoningDelta { text }) => {
            ProviderEvent::Event(ModelEvent::ReasoningDelta {
                text: text.replace('\r', ""),
            })
        }
        other => other,
    }
}

pub(crate) fn validate_replay_body(
    value: &serde_json::Value,
) -> Result<(), String> {
    validate_replay_shape(value)?;
    if !replay_text_fields_valid(value) {
        return Err(
            "replay response text contains an unsupported control character"
                .to_owned(),
        );
    }
    Ok(())
}

fn replay_text_fields_valid(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => {
            object.iter().all(|(key, value)| {
                if matches!(
                    key.as_str(),
                    "arguments" | "input" | "tool_calls" | "function"
                ) {
                    // Tool payloads are arbitrary bounded JSON, not provider
                    // text. A user-controlled `{"text": 42}` must not be
                    // mistaken for an output field.
                    true
                } else if key == "content" {
                    match value {
                        serde_json::Value::String(text) => {
                            valid_replay_text(text)
                        }
                        _ => replay_text_fields_valid(value),
                    }
                } else if matches!(
                    key.as_str(),
                    "text" | "thinking" | "reasoning"
                ) {
                    (key == "reasoning" && value.is_null())
                        || value.as_str().is_some_and(valid_replay_text)
                } else {
                    replay_text_fields_valid(value)
                }
            })
        }
        serde_json::Value::Array(values) => {
            values.iter().all(replay_text_fields_valid)
        }
        _ => true,
    }
}

pub(crate) fn completion_events_from_body(text: &str) -> Vec<ProviderEvent> {
    let value: Value = match serde_json::from_str::<Value>(text) {
        Ok(value) if value.is_object() => value,
        _ => {
            return vec![ProviderEvent::Failed(
                "replay recording is not a JSON object".to_owned(),
            )];
        }
    };
    if let Err(reason) = validate_replay_body(&value) {
        return vec![ProviderEvent::Failed(reason)];
    }
    let mut events = Vec::new();
    let choices = value
        .get("choices")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let has_choices = !choices.is_empty();
    for choice in choices {
        let message = choice.get("message").cloned().unwrap_or(Value::Null);
        // S3: a recorded body carries the thinking it streamed, so a replay
        // reproduces the reasoning channel too.
        if let Some(reasoning) =
            message.get("reasoning").and_then(|v| v.as_str())
        {
            if !reasoning.is_empty() {
                events.push(ProviderEvent::Event(
                    ModelEvent::ReasoningDelta { text: reasoning.to_owned() },
                ));
            }
        }
        if let Some(content) = message.get("content") {
            if let Some(text) =
                content.as_str().filter(|text| !text.is_empty())
            {
                events.push(ProviderEvent::Event(ModelEvent::TextDelta {
                    text: text.to_owned(),
                }));
            } else if let Some(blocks) = content.as_array() {
                for block in blocks {
                    let kind = block.get("type").and_then(Value::as_str);
                    if kind == Some("thinking") {
                        if let Some(text) = block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            events.push(ProviderEvent::Event(
                                ModelEvent::ReasoningDelta {
                                    text: text.to_owned(),
                                },
                            ));
                        }
                        continue;
                    }
                    if has_choices
                        && !matches!(
                            kind,
                            Some("text" | "output_text" | "input_text")
                        )
                    {
                        continue;
                    }
                    if let Some(text) = block
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                    {
                        events.push(ProviderEvent::Event(
                            ModelEvent::TextDelta { text: text.to_owned() },
                        ));
                    }
                }
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
                let Some(args_str) = call
                    .get("function")
                    .and_then(|v| v.get("arguments"))
                    .and_then(|v| v.as_str())
                else {
                    events.push(ProviderEvent::Failed(
                        "replay tool call arguments are missing".to_owned(),
                    ));
                    return events;
                };
                let Ok(input_val) = serde_json::from_str::<Value>(args_str)
                else {
                    events.push(ProviderEvent::Failed(
                        "replay tool call arguments are invalid JSON"
                            .to_owned(),
                    ));
                    return events;
                };
                if !input_val.is_object() {
                    events.push(ProviderEvent::Failed(
                        "replay tool call arguments must be an object"
                            .to_owned(),
                    ));
                    return events;
                }
                if id.is_empty() || name.is_empty() {
                    events.push(ProviderEvent::Failed(
                        "replay tool call is incomplete".to_owned(),
                    ));
                    return events;
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
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        for item in output {
            if let Some(blocks) = item.get("content").and_then(Value::as_array)
            {
                for block in blocks {
                    let kind = block.get("type").and_then(Value::as_str);
                    if kind == Some("thinking") {
                        if let Some(text) = block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            events.push(ProviderEvent::Event(
                                ModelEvent::ReasoningDelta {
                                    text: text.to_owned(),
                                },
                            ));
                        }
                        continue;
                    }
                    if let Some(text) = block
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                    {
                        events.push(ProviderEvent::Event(
                            ModelEvent::TextDelta { text: text.to_owned() },
                        ));
                    }
                }
            }
            if item
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "function_call")
            {
                let id = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let Some(arguments) =
                    item.get("arguments").and_then(Value::as_str)
                else {
                    events.push(ProviderEvent::Failed(
                        "replay function call arguments are missing"
                            .to_owned(),
                    ));
                    return events;
                };
                let Ok(input_value) = serde_json::from_str::<Value>(arguments)
                else {
                    events.push(ProviderEvent::Failed(
                        "replay function call arguments are invalid JSON"
                            .to_owned(),
                    ));
                    return events;
                };
                if !input_value.is_object() {
                    events.push(ProviderEvent::Failed(
                        "replay function call arguments must be an object"
                            .to_owned(),
                    ));
                    return events;
                }
                if id.is_empty()
                    || name.is_empty()
                    || id.len() > 256
                    || id.chars().any(char::is_control)
                    || name.len()
                        > crate::provider::tool_names::MAX_PROVIDER_TOOL_NAME_BYTES
                    || name.chars().any(char::is_control)
                {
                    events.push(ProviderEvent::Failed(
                        "replay function call is incomplete".to_owned(),
                    ));
                    return events;
                }
                let input = siralos_core::provider::ToolCallInput::from_value(
                    input_value,
                );
                events.push(ProviderEvent::Event(ModelEvent::ToolCall {
                    call_id: id,
                    tool_name: name,
                    input,
                }));
            }
        }
    }
    if value.get("content").is_some()
        && value.get("choices").is_none()
        && value.get("output").is_none()
    {
        if let Some(content_arr) =
            value.get("content").and_then(|v| v.as_array())
        {
            for block in content_arr {
                let kind = block.get("type").and_then(Value::as_str);
                match kind {
                    Some("text") | Some("output_text")
                    | Some("input_text") => {
                        if let Some(text) = block
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            events.push(ProviderEvent::Event(
                                ModelEvent::TextDelta {
                                    text: text.to_owned(),
                                },
                            ));
                        }
                    }
                    Some("thinking") => {
                        if let Some(text) = block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            events.push(ProviderEvent::Event(
                                ModelEvent::ReasoningDelta {
                                    text: text.to_owned(),
                                },
                            ));
                        }
                    }
                    Some("tool_use") | Some("function_call") => {
                        let id = block
                            .get("id")
                            .or_else(|| block.get("call_id"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let input_value = block
                            .get("input")
                            .or_else(|| block.get("arguments"))
                            .map(|value| {
                                value
                                    .as_str()
                                    .and_then(|text| {
                                        serde_json::from_str::<Value>(text)
                                            .ok()
                                    })
                                    .unwrap_or_else(|| value.clone())
                            })
                            .unwrap_or(Value::Null);
                        if valid_replay_identifier(id, 256)
                            && valid_replay_identifier(
                                name,
                                crate::provider::tool_names::MAX_PROVIDER_TOOL_NAME_BYTES,
                            )
                            && input_value.is_object()
                        {
                            events.push(ProviderEvent::Event(
                                ModelEvent::ToolCall {
                                    call_id: id.to_owned(),
                                    tool_name: name.to_owned(),
                                    input: siralos_core::provider::ToolCallInput::from_value(
                                        input_value,
                                    ),
                                },
                            ));
                        } else {
                            events.push(ProviderEvent::Failed(
                                "replay tool call is incomplete".to_owned(),
                            ));
                            return events;
                        }
                    }
                    Some("redacted_thinking") => {
                        if !block.get("data").is_some_and(Value::is_string) {
                            events.push(ProviderEvent::Failed(
                                "replay redacted thinking block is incomplete"
                                    .to_owned(),
                            ));
                            return events;
                        }
                    }
                    Some(_) => {}
                    None => {
                        events.push(ProviderEvent::Failed(
                            "replay content block needs a type".to_owned(),
                        ));
                        return events;
                    }
                }
            }
        }
    }
    let has_openai_shape = value.get("choices").is_some();
    let has_responses_shape = value.get("output").is_some();
    let has_anthropic_shape = value.get("content").is_some();
    if !has_openai_shape && !has_anthropic_shape && !has_responses_shape {
        return vec![ProviderEvent::Failed(
            "replay recording has an unsupported response shape".to_owned(),
        )];
    }
    if has_openai_shape
        && value.get("choices").and_then(Value::as_array).is_none()
    {
        return vec![ProviderEvent::Failed(
            "replay recording choices must be an array".to_owned(),
        )];
    }
    if has_anthropic_shape
        && value.get("content").and_then(Value::as_array).is_none()
    {
        return vec![ProviderEvent::Failed(
            "replay recording content must be an array".to_owned(),
        )];
    }
    if has_openai_shape
        && value
            .get("choices")
            .and_then(Value::as_array)
            .is_some_and(|choices| choices.is_empty())
    {
        return vec![ProviderEvent::Failed(
            "replay recording contains no choices".to_owned(),
        )];
    }
    if has_anthropic_shape
        && value
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|content| content.is_empty())
    {
        return vec![ProviderEvent::Failed(
            "replay recording contains no content blocks".to_owned(),
        )];
    }
    if events.is_empty() {
        return vec![ProviderEvent::Failed(
            "replay recording contains no usable completion event".to_owned(),
        )];
    }
    events.push(ProviderEvent::Event(ModelEvent::Completed));
    events
}

struct ReplayCursorEvents<'a> {
    inner: std::vec::IntoIter<ProviderEvent>,
    cursor: &'a core::cell::Cell<usize>,
    cancellation: CancellationSignal<'a>,
    index: usize,
    advanced: bool,
    cancelled_emitted: bool,
}

impl Iterator for ReplayCursorEvents<'_> {
    type Item = ProviderEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cancellation.is_cancelled() {
            if self.cancelled_emitted {
                return None;
            }
            self.cancelled_emitted = true;
            if !self.advanced {
                self.cursor.set(self.index + 1);
                self.advanced = true;
            }
            return Some(ProviderEvent::Cancelled {
                message: "replay cancelled by host".to_owned(),
            });
        }
        let event = self.inner.next()?;
        if matches!(event, ProviderEvent::Event(ModelEvent::Completed))
            && !self.advanced
        {
            self.cursor.set(self.index + 1);
            self.advanced = true;
        }
        Some(event)
    }
}

/// Recorded-response replay provider for determinism-port recordings.
///
/// Serves in-memory [`ReplayRecording`]s as `ProviderEvent`s. Recording data
/// lives in memory only and is consumed sequentially.
///
/// The `model` is a live cell like the HTTP adapters (a `/model` switch
/// updates the label it reports; playback itself serves the fixed
/// recordings and never re-matches on the model).
pub struct RecordedReplayProvider {
    /// Provider identifier.
    provider_id: String,
    /// Model identifier.
    model: Rc<RefCell<String>>,
    /// Effective credential-free route binding, when constructed with one.
    route: Option<(String, String, String)>,
    /// Recordings to serve in order.
    recordings: Vec<ReplayRecording>,
    /// Cursor into `recordings`.
    cursor: core::cell::Cell<usize>,
    /// Constructor validation failure retained as a fail-closed state.
    invalid: bool,
}

impl std::fmt::Debug for RecordedReplayProvider {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("RecordedReplayProvider")
            .field("route_bound", &self.route.is_some())
            .field("provider_id", &"[CONFIGURED]")
            .field("model", &"[CONFIGURED]")
            .field("recording_count", &self.recordings.len())
            .field("cursor", &self.cursor.get())
            .finish()
    }
}

fn valid_provider_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.chars().all(|ch| !ch.is_control())
}

/// Project a recorder snapshot onto the replay-only persistence contract.
///
/// Transport/error identities and non-replayable bodies remain available in
/// the recorder for diagnostics, but are deliberately excluded from a store
/// that playback can consume. The same shared validator used by the store
/// writer and loader makes this projection fail closed.
#[must_use]
pub fn replayable_recording_snapshot(
    recordings: &[ReplayRecording],
) -> Vec<ReplayRecording> {
    recordings
        .iter()
        .filter(|recording| validate_replay_recording(recording, None).is_ok())
        .cloned()
        .collect()
}

fn valid_replay_recording(
    recording: &ReplayRecording,
    provider_id: &str,
) -> bool {
    validate_replay_recording(recording, Some(provider_id)).is_ok()
}

impl RecordedReplayProvider {
    /// Create a legacy, route-unbound replay provider over the given
    /// recordings.
    ///
    /// A mixed persisted cache is accepted, but this mode serves only entries
    /// whose `request_sha256` is `None`; route-bound entries remain available
    /// to [`Self::new_with_route`]. A non-empty route-bound-only input is
    /// rejected rather than replayed without route binding.
    #[must_use]
    pub fn new(
        provider_id: String,
        model: String,
        recordings: Vec<ReplayRecording>,
    ) -> Self {
        Self::new_internal(provider_id, model, None, recordings)
    }

    /// Construct a replay provider bound to an exact credential-free route.
    /// Recordings without a request digest are rejected in this mode; the
    /// legacy [`Self::new`] constructor remains available for old recordings.
    #[must_use]
    pub fn new_with_route(
        provider_id: String,
        model: String,
        endpoint: String,
        protocol: String,
        recordings: Vec<ReplayRecording>,
    ) -> Self {
        let route = (provider_id.clone(), endpoint, protocol);
        Self::new_internal(provider_id, model, Some(route), recordings)
    }

    fn new_internal(
        provider_id: String,
        model: String,
        route: Option<(String, String, String)>,
        recordings: Vec<ReplayRecording>,
    ) -> Self {
        // Keep constructor diagnostics and public accessors bounded even when
        // a caller supplies an invalid configured label.  The original values
        // still make the instance fail closed, but are never exposed through
        // the live provider surface.
        let provider_valid = valid_provider_identity(&provider_id);
        let model_valid = valid_provider_identity(&model);
        let provider_id = if provider_valid {
            provider_id
        } else {
            "invalid-provider".to_owned()
        };
        let model =
            if model_valid { model } else { "invalid-model".to_owned() };
        // Normalize the public protocol alias before it is used in the route
        // digest.  `openai-compatible` and `anthropic` are accepted at the
        // profile boundary, but the live adapters always bind the canonical
        // protocol name.  Keeping the canonical spelling here prevents a
        // legacy alias from producing a different request identity.
        let route = route.map(|(provider, endpoint, protocol)| {
            let protocol =
                siralos_core::composition::Protocol::parse(&protocol)
                    .map_or_else(
                        || protocol.clone(),
                        |parsed| parsed.as_str().to_owned(),
                    );
            (provider, endpoint, protocol)
        });
        let route_valid =
            route.as_ref().is_none_or(|(provider, endpoint, protocol)| {
                valid_provider_identity(provider)
                    && endpoint.len()
                        <= siralos_core::composition::MAX_PROFILE_ENDPOINT_BYTES
                    && siralos_core::composition::is_valid_http_endpoint(
                        endpoint,
                    )
                    && siralos_core::composition::Protocol::parse(protocol)
                        .is_some()
            });
        // Select the mode-specific set before validating bounds.  A legacy
        // cache may contain a route-bound entry that is intentionally ignored;
        // an invalid entry in the ignored set must not poison otherwise valid
        // legacy playback.  Route-bound playback, however, validates the whole
        // supplied set and fails closed.
        let (recordings, legacy_mode_invalid) = if route.is_some() {
            (recordings, false)
        } else {
            let supplied_nonempty = !recordings.is_empty();
            let legacy: Vec<_> = recordings
                .into_iter()
                .filter(|recording| recording.request_sha256.is_none())
                .collect();
            let legacy_empty = legacy.is_empty();
            (legacy, supplied_nonempty && legacy_empty)
        };
        let selected_bounds_valid =
            siralos_core::determinism::validate_replay_store_bounds(
                &recordings,
            )
            .is_ok();
        let selected_recordings_valid = recordings.iter().all(|recording| {
            recording.identity.model == model
                && valid_replay_recording(recording, &provider_id)
        });
        let invalid = legacy_mode_invalid
            || !route_valid
            || !provider_valid
            || !model_valid
            || !selected_bounds_valid
            || !selected_recordings_valid
            || route
                .as_ref()
                .is_some_and(|(provider, _, _)| provider != &provider_id)
            || (route.is_some()
                && recordings
                    .iter()
                    .any(|recording| recording.request_sha256.is_none()));
        let recordings = if invalid { Vec::new() } else { recordings };
        Self {
            provider_id,
            model: Rc::new(RefCell::new(model)),
            route,
            recordings,
            cursor: core::cell::Cell::new(0),
            invalid,
        }
    }

    /// Fallible constructor for callers that need an explicit rejection
    /// instead of the compatibility constructor's fail-closed state.
    pub fn try_new(
        provider_id: String,
        model: String,
        recordings: Vec<ReplayRecording>,
    ) -> Result<Self, String> {
        let provider = Self::new(provider_id, model, recordings);
        if provider.invalid {
            return Err("replay recordings exceed safety bounds".to_owned());
        }
        Ok(provider)
    }

    /// Fallible route-bound constructor.
    pub fn try_new_with_route(
        provider_id: String,
        model: String,
        endpoint: String,
        protocol: String,
        recordings: Vec<ReplayRecording>,
    ) -> Result<Self, String> {
        let provider = Self::new_with_route(
            provider_id,
            model,
            endpoint,
            protocol,
            recordings,
        );
        if provider.invalid {
            return Err(
                "replay recordings are not valid for this route".to_owned()
            );
        }
        Ok(provider)
    }

    /// Number of recordings remaining to be served.
    #[must_use]
    pub fn recordings_remaining(&self) -> usize {
        self.recordings.len().saturating_sub(self.cursor.get())
    }

    /// Replace the live model label in place (playback still serves the
    /// fixed recordings; the label is what the session reports).
    pub fn set_model(&self, model: String) {
        if valid_provider_identity(&model) {
            *self.model.borrow_mut() = model;
        }
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
        if self.invalid {
            return Box::new(std::iter::once(ProviderEvent::Failed(
                "replay recordings failed constructor validation".to_owned(),
            )));
        }
        if cancellation.is_cancelled() {
            return Box::new(std::iter::once(ProviderEvent::Cancelled {
                message: CANCELLED_BEFORE_PROVIDER_START.to_owned(),
            }));
        }
        let index = self.cursor.get();
        if index >= self.recordings.len() {
            return Box::new(std::iter::once(ProviderEvent::Failed(
                "no recorded response for replay: recording exhausted"
                    .to_owned(),
            )));
        }
        let recording = &self.recordings[index];
        if recording.identity.provider_id != self.provider_id {
            return Box::new(std::iter::once(ProviderEvent::Failed(
                "replay recording identity does not match the active provider"
                    .to_owned(),
            )));
        }
        let request_sha256 = self.route.as_ref().map_or_else(
            || crate::provider::request_sha256(_request),
            |(provider, endpoint, protocol)| {
                crate::provider::request_sha256_for_route(
                    _request, provider, endpoint, protocol,
                )
            },
        );
        if let Some(expected) = recording.request_sha256.as_deref() {
            if !expected.eq_ignore_ascii_case(&request_sha256) {
                return Box::new(std::iter::once(ProviderEvent::Failed(
                    "replay recording request identity does not match"
                        .to_owned(),
                )));
            }
        }
        if !recording
            .identity
            .status
            .is_some_and(|status| (200..300).contains(&status))
        {
            return Box::new(std::iter::once(ProviderEvent::Failed(
                "replay response has no successful HTTP status".to_owned(),
            )));
        }
        if recording.identity.body_bytes != recording.body.len() as u64
            || !recording.identity.body_sha256.eq_ignore_ascii_case(
                &siralos_core::identity::sha256_hex(recording.body.as_bytes()),
            )
        {
            return Box::new(std::iter::once(ProviderEvent::Failed(
                "replay response body failed integrity verification"
                    .to_owned(),
            )));
        }
        let events = completion_events_from_body(&recording.body);
        if events.is_empty()
            || events
                .iter()
                .any(|event| matches!(event, ProviderEvent::Failed(_)))
        {
            return Box::new(std::iter::once(ProviderEvent::Failed(
                "replay response has no usable successful completion"
                    .to_owned(),
            )));
        }
        let events =
            events.into_iter().map(normalize_replay_event).collect::<Vec<_>>();
        let tool_names = ToolNames::new(
            _request.tools.iter().map(|tool| tool.name.as_str()),
        );
        let events = tool_names.restore_events(events);
        Box::new(ReplayCursorEvents {
            inner: events.into_iter(),
            cursor: &self.cursor,
            cancellation,
            index,
            advanced: false,
            cancelled_emitted: false,
        })
    }
}

impl std::fmt::Display for RecordedReplayProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RecordedReplayProvider([REDACTED])")
    }
}

/// composes a replay provider from the recorder's detached snapshot
/// (recordings_snapshot()); the recorder keeps its recordings — the provider
/// serves a copy.
#[must_use]
pub fn replay_provider_from_recorder_with_route(
    provider_id: String,
    model: String,
    endpoint: String,
    protocol: String,
    recorder: &siralos_core::determinism::RetainingReplayRecorder,
) -> RecordedReplayProvider {
    RecordedReplayProvider::new_with_route(
        provider_id,
        model,
        endpoint,
        protocol,
        replayable_recording_snapshot(&recorder.replayable_records_snapshot()),
    )
}

#[must_use]
/// Build a legacy, route-unbound replay provider from a recorder.
///
/// Use this constructor only for legacy recordings that predate
/// `request_sha256`. New recordings should use
/// [`replay_provider_from_recorder_with_route`] so replay is bound to the
/// exact provider, endpoint, protocol, and request identity.
pub fn replay_provider_from_recorder(
    provider_id: String,
    model: String,
    recorder: &siralos_core::determinism::RetainingReplayRecorder,
) -> RecordedReplayProvider {
    RecordedReplayProvider::new(
        provider_id,
        model,
        replayable_recording_snapshot(&recorder.replayable_records_snapshot()),
    )
}

/// Session replay-run composition: in-process record-then-replay.
///
/// The composer owns a retaining recorder for the record phase and composes a
/// replay provider from its detached snapshot. Nothing is persisted.
pub struct SessionReplayComposer {
    provider_id: String,
    model: String,
    recorder: std::rc::Rc<siralos_core::determinism::RetainingReplayRecorder>,
    route: Option<(String, String)>,
}

impl std::fmt::Debug for SessionReplayComposer {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("SessionReplayComposer")
            .field("provider_id", &"[CONFIGURED]")
            .field("model", &"[CONFIGURED]")
            .field("route_bound", &self.route.is_some())
            .field("recorder", &"[PRESENT]")
            .finish()
    }
}

impl SessionReplayComposer {
    /// Create a legacy, route-unbound composer that owns its retaining
    /// recorder.
    ///
    /// Use [`Self::new_with_route`] when the live provider prepares a
    /// route-bound request digest; a route digest cannot be verified by this
    /// constructor because it has no endpoint or protocol.
    #[must_use]
    pub fn new(provider_id: String, model: String) -> Self {
        Self {
            provider_id,
            model,
            route: None,
            recorder: std::rc::Rc::new(
                siralos_core::determinism::RetainingReplayRecorder::new(),
            ),
        }
    }

    /// Create a route-bound composer. Recorded playback then computes the
    /// same request digest as the live adapter, including fixed routes.
    #[must_use]
    pub fn new_with_route(
        provider_id: String,
        model: String,
        endpoint: String,
        protocol: String,
    ) -> Self {
        let mut composer = Self::new(provider_id, model);
        composer.route = Some((endpoint, protocol));
        composer
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
        if let Some((endpoint, protocol)) = &self.route {
            replay_provider_from_recorder_with_route(
                self.provider_id.clone(),
                self.model.clone(),
                endpoint.clone(),
                protocol.clone(),
                &self.recorder,
            )
        } else {
            replay_provider_from_recorder(
                self.provider_id.clone(),
                self.model.clone(),
                &self.recorder,
            )
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use siralos_core::determinism::ReplayRecorder;
    use siralos_core::provider::{
        CancellationToken, ModelProvider, ModelRequest, ProviderEvent,
        ToolDefinition,
    };

    fn recording(
        body: &str,
        request_sha256: Option<String>,
    ) -> ReplayRecording {
        ReplayRecording {
            identity: siralos_core::determinism::ProviderResponseIdentity {
                provider_id: "route-provider".to_owned(),
                model: "route-model".to_owned(),
                status: Some(200),
                body_sha256: crate::provider::response_body_sha256(body),
                body_bytes: body.len() as u64,
                observed_at_ms: Some(1),
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
            },
            body: body.to_owned(),
            request_sha256,
        }
    }

    #[test]
    fn replayable_snapshot_drops_terminal_evidence_without_dropping_success() {
        let body = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
        let mut terminal = recording(body, None);
        terminal.identity.status = Some(404);
        let valid = recording(body, None);
        let filtered =
            replayable_recording_snapshot(&[terminal, valid.clone()]);
        assert_eq!(filtered, vec![valid]);
    }

    #[test]
    fn marker_aware_recorder_projection_excludes_shape_valid_evidence() {
        let body = r#"{"choices":[{"message":{"content":"not replayable"}}]}"#;
        let identity = recording(body, None).identity;
        let recorder =
            siralos_core::determinism::RetainingReplayRecorder::new();
        assert!(recorder.try_record_provider_response_with_body_as_evidence(
            &identity, body,
        ));
        assert!(
            replayable_recording_snapshot(
                &recorder.replayable_records_snapshot(),
            )
            .is_empty()
        );
        let provider = replay_provider_from_recorder(
            "route-provider".to_owned(),
            "route-model".to_owned(),
            &recorder,
        );
        assert_eq!(provider.recordings_remaining(), 0);
    }

    #[test]
    fn replay_identifier_validation_rejects_whitespace_only_identity() {
        let body = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
        let mut whitespace = recording(body, None);
        whitespace.identity.provider_id = " ".to_owned();
        assert!(replayable_recording_snapshot(&[whitespace]).is_empty());
    }

    #[test]
    fn unsupported_replay_shapes_are_rejected_before_persistence() {
        let value = serde_json::json!({"choices": []});
        assert!(validate_replay_shape(&value).is_err());
    }

    #[test]
    fn route_bound_playback_matches_canonical_request_and_provider_route() {
        let body = r#"{"choices":[{"message":{"content":"route ok"}}]}"#;
        let request = ModelRequest {
            messages: vec![],
            tools: vec![],
            system: Some("route request".to_owned()),
        };
        let endpoint = "https://route.example.test";
        let protocol = "openai-completions";
        let request_sha256 = crate::provider::request_sha256_for_route(
            &request,
            "route-provider",
            endpoint,
            protocol,
        );
        let provider = RecordedReplayProvider::new_with_route(
            "route-provider".to_owned(),
            "route-model".to_owned(),
            endpoint.to_owned(),
            protocol.to_owned(),
            vec![recording(body, Some(request_sha256.to_ascii_uppercase()))],
        );
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert!(
            events
                .iter()
                .all(|event| { !matches!(event, ProviderEvent::Failed(_)) })
        );
        assert!(matches!(
            events.last(),
            Some(ProviderEvent::Event(ModelEvent::Completed))
        ));
    }

    #[test]
    fn legacy_playback_uses_only_unbound_entries_in_a_mixed_cache() {
        let legacy_body = r#"{"choices":[{"message":{"content":"legacy"}}]}"#;
        let route_body = r#"{"choices":[{"message":{"content":"route"}}]}"#;
        let request =
            ModelRequest { messages: vec![], tools: vec![], system: None };
        let endpoint = "https://route.example.test";
        let protocol = "openai-completions";
        let request_sha256 = crate::provider::request_sha256_for_route(
            &request,
            "route-provider",
            endpoint,
            protocol,
        );
        let provider = RecordedReplayProvider::new(
            "route-provider".to_owned(),
            "route-model".to_owned(),
            vec![
                recording(legacy_body, None),
                recording(route_body, Some(request_sha256)),
            ],
        );
        assert_eq!(provider.recordings_remaining(), 1);
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert!(
            events
                .iter()
                .all(|event| { !matches!(event, ProviderEvent::Failed(_)) })
        );
        assert!(format!("{events:?}").contains("legacy"));
        assert!(!format!("{events:?}").contains("route"));
    }

    #[test]
    fn route_bound_composer_records_and_replays_the_live_route_digest() {
        let body = r#"{"choices":[{"message":{"content":"composed"}}]}"#;
        let request = ModelRequest {
            messages: vec![],
            tools: vec![],
            system: Some("composer request".to_owned()),
        };
        let endpoint = "https://composer.example.test";
        let protocol = "openai-completions";
        let request_sha256 = crate::provider::request_sha256_for_route(
            &request,
            "route-provider",
            endpoint,
            protocol,
        );
        let composer = SessionReplayComposer::new_with_route(
            "route-provider".to_owned(),
            "route-model".to_owned(),
            endpoint.to_owned(),
            protocol.to_owned(),
        );
        let identity = recording(body, Some(request_sha256.clone())).identity;
        assert!(
            composer
                .recorder()
                .try_record_provider_response_with_body_and_request(
                    &identity,
                    body,
                    &request_sha256,
                )
        );
        let provider = composer.compose();
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert!(
            events
                .iter()
                .all(|event| { !matches!(event, ProviderEvent::Failed(_)) })
        );
        assert!(matches!(
            events.last(),
            Some(ProviderEvent::Event(ModelEvent::Completed))
        ));
    }

    #[test]
    fn composer_projects_terminal_evidence_before_replay() {
        let body = r#"{"choices":[{"message":{"content":"composed"}}]}"#;
        let request =
            ModelRequest { messages: vec![], tools: vec![], system: None };
        let composer = SessionReplayComposer::new(
            "route-provider".to_owned(),
            "route-model".to_owned(),
        );
        let mut terminal = recording(body, None);
        terminal.identity.status = Some(404);
        assert!(
            composer
                .recorder()
                .try_record_provider_response_with_body_as_evidence(
                    &terminal.identity,
                    body,
                )
        );
        let valid = recording(body, None);
        assert!(
            composer
                .recorder()
                .try_record_provider_response_with_body(&valid.identity, body)
        );
        let provider = composer.compose();
        assert_eq!(provider.recordings_remaining(), 1);
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert!(
            events
                .iter()
                .all(|event| { !matches!(event, ProviderEvent::Failed(_)) })
        );
    }

    #[test]
    fn route_bound_playback_rejects_a_request_digest_mismatch() {
        let body = r#"{"choices":[{"message":{"content":"route ok"}}]}"#;
        let request =
            ModelRequest { messages: vec![], tools: vec![], system: None };
        let provider = RecordedReplayProvider::new_with_route(
            "route-provider".to_owned(),
            "route-model".to_owned(),
            "https://route.example.test".to_owned(),
            "openai-completions".to_owned(),
            vec![recording(body, Some("a".repeat(64)))],
        );
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert!(matches!(events.first(), Some(ProviderEvent::Failed(_))));
    }

    #[test]
    fn legacy_playback_ignores_invalid_route_bound_entries_in_mixed_cache() {
        let legacy = recording(
            r#"{"choices":[{"message":{"content":"legacy"}}]}"#,
            None,
        );
        let mut invalid_route = recording("{}", Some("a".repeat(64)));
        invalid_route.identity.body_sha256 =
            crate::provider::response_body_sha256("{}");
        invalid_route.identity.body_bytes = 2;
        let provider = RecordedReplayProvider::new(
            "route-provider".to_owned(),
            "route-model".to_owned(),
            vec![invalid_route, legacy],
        );
        let request =
            ModelRequest { messages: vec![], tools: vec![], system: None };
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert!(
            events
                .iter()
                .all(|event| { !matches!(event, ProviderEvent::Failed(_)) })
        );
        assert!(matches!(
            events.last(),
            Some(ProviderEvent::Event(ModelEvent::Completed))
        ));
    }

    #[test]
    fn route_bound_playback_normalizes_legacy_protocol_aliases() {
        let body = r#"{"choices":[{"message":{"content":"alias"}}]}"#;
        let request =
            ModelRequest { messages: vec![], tools: vec![], system: None };
        let endpoint = "https://route.example.test";
        let request_sha256 = crate::provider::request_sha256_for_route(
            &request,
            "route-provider",
            endpoint,
            "openai-completions",
        );
        let provider = RecordedReplayProvider::new_with_route(
            "route-provider".to_owned(),
            "route-model".to_owned(),
            endpoint.to_owned(),
            "openai-compatible".to_owned(),
            vec![recording(body, Some(request_sha256))],
        );
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert!(
            events
                .iter()
                .all(|event| { !matches!(event, ProviderEvent::Failed(_)) })
        );
        assert!(matches!(
            events.last(),
            Some(ProviderEvent::Event(ModelEvent::Completed))
        ));
    }

    #[test]
    fn unknown_replay_block_fails_instead_of_allowing_partial_success() {
        let body = serde_json::json!({
            "content": [
                {"type": "text", "text": "visible"},
                {"type": "unrecognized"}
            ]
        });
        let events = completion_events_from_body(&body.to_string());
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ProviderEvent::Failed(_)));
    }

    #[test]
    fn failed_status_or_malformed_tool_wire_shape_fails_closed() {
        let failed = serde_json::json!({
            "status": 503,
            "choices": [{"message": {"content": "not successful"}}]
        });
        let failed_events = completion_events_from_body(&failed.to_string());
        assert_eq!(failed_events.len(), 1);
        assert!(matches!(failed_events[0], ProviderEvent::Failed(_)));

        let malformed = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "not-function",
                        "function": {"name": "n", "arguments": "{}"}
                    }]
                }
            }]
        });
        let malformed_events =
            completion_events_from_body(&malformed.to_string());
        assert_eq!(malformed_events.len(), 1);
        assert!(matches!(malformed_events[0], ProviderEvent::Failed(_)));
    }

    #[test]
    fn replay_restores_request_scoped_tool_aliases_before_emission() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "workspace_read",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        })
        .to_string();
        let request = ModelRequest {
            messages: vec![],
            tools: vec![ToolDefinition {
                name: "workspace.read".to_owned(),
                description: "read".to_owned(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
            system: None,
        };
        let provider = RecordedReplayProvider::new(
            "route-provider".to_owned(),
            "route-model".to_owned(),
            vec![recording(&body, None)],
        );
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert!(events.iter().any(|event| {
            matches!(
                event,
                ProviderEvent::Event(ModelEvent::ToolCall {
                    tool_name,
                    ..
                }) if tool_name == "workspace.read"
            )
        }));
    }

    #[test]
    fn constructor_rejects_a_recording_for_another_model() {
        let body = r#"{"choices":[{"message":{"content":"model"}}]}"#;
        let provider = RecordedReplayProvider::new(
            "route-provider".to_owned(),
            "different-model".to_owned(),
            vec![recording(body, None)],
        );
        let request =
            ModelRequest { messages: vec![], tools: vec![], system: None };
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert!(matches!(events.first(), Some(ProviderEvent::Failed(_))));
    }
}
