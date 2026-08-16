//! Strict `tool-loop` scenario input validation (mirrors contract.mjs).

use serde_json::Value;

use super::super::{HarnessError, is_repeat_marker};

const MAX_TOOL_LOOP_CASES: usize = 64;
const MAX_TOOL_LOOP_TOOLS: usize = 32;
const MAX_TOOL_LOOP_RULES: usize = 32;
const MAX_TOOL_LOOP_VISIBLE_TOOLS: usize = 32;
const MAX_TOOL_LOOP_EVENTS: usize = 4096;
const MAX_CAPABILITY_BYTES: usize = 64;

const SUPPORTED_TOOLS: &[&str] = &[
    "workspace.read",
    "stub.success",
    "stub.invalid_input",
    "stub.denied",
    "stub.failed",
    "stub.cancelled",
    "a.tool",
    "b.tool",
];

fn validate_bounded_tool_names(
    value: &Value,
    maximum: usize,
    label: &str,
) -> Result<(), HarnessError> {
    let names = value.as_array().ok_or_else(|| {
        HarnessError::corpus(format!("{label} must be an array"))
    })?;
    if names.len() > maximum {
        return Err(HarnessError::corpus(format!(
            "{label} exceeds {maximum} entries"
        )));
    }
    for name in names {
        let text = name.as_str().ok_or_else(|| {
            HarnessError::corpus(format!("{label} entries must be strings"))
        })?;
        if text.is_empty() || text.len() > 256 {
            return Err(HarnessError::corpus(format!(
                "{label} entries must be non-empty tool names"
            )));
        }
    }
    Ok(())
}

fn validate_capability_identifier(
    value: &str,
    label: &str,
) -> Result<(), HarnessError> {
    if value.is_empty() || value.len() > MAX_CAPABILITY_BYTES {
        return Err(HarnessError::corpus(format!("{label} is invalid")));
    }
    let mut previous_separator = false;
    for (index, byte) in value.bytes().enumerate() {
        let separator = matches!(byte, b'.' | b'_' | b'-');
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || separator)
            || (separator && (index == 0 || previous_separator))
        {
            return Err(HarnessError::corpus(format!("{label} is invalid")));
        }
        previous_separator = separator;
    }
    if previous_separator {
        return Err(HarnessError::corpus(format!("{label} is invalid")));
    }
    Ok(())
}

fn validate_tool_loop_case(
    case: &Value,
    label: &str,
) -> Result<(), HarnessError> {
    let object = case.as_object().ok_or_else(|| {
        HarnessError::corpus(format!("{label} must be an object"))
    })?;
    let allowed = [
        "prompt",
        "maxToolRounds",
        "tools",
        "rules",
        "visibleTools",
        "provider",
        "cancelAfterCompletedToolCalls",
    ];
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(HarnessError::corpus(format!(
                "{label} has an unknown field {key}"
            )));
        }
    }
    let prompt = case.get("prompt").ok_or_else(|| {
        HarnessError::corpus(format!("{label} requires prompt"))
    })?;
    if prompt.as_str().is_none() && !is_repeat_marker(prompt) {
        return Err(HarnessError::corpus(format!(
            "{label}.prompt must be a string or repeat marker"
        )));
    }
    if let Some(max_rounds) = case.get("maxToolRounds") {
        let valid = matches!(max_rounds, Value::Null)
            || max_rounds.as_str().is_some_and(|value| value == "non-finite")
            || max_rounds.as_f64().is_some_and(f64::is_finite);
        if !valid {
            return Err(HarnessError::corpus(format!(
                "{label}.maxToolRounds is invalid"
            )));
        }
    }
    let tools = case.get("tools").ok_or_else(|| {
        HarnessError::corpus(format!("{label} requires tools"))
    })?;
    validate_bounded_tool_names(
        tools,
        MAX_TOOL_LOOP_TOOLS,
        &format!("{label}.tools"),
    )?;
    for name in tools.as_array().expect("validated array") {
        let name = name.as_str().expect("validated string");
        if !SUPPORTED_TOOLS.contains(&name) {
            return Err(HarnessError::corpus(format!(
                "{label}.tools contains unsupported tool {name}"
            )));
        }
    }
    if let Some(rules) = case.get("rules") {
        let rules = rules.as_array().ok_or_else(|| {
            HarnessError::corpus(format!("{label}.rules must be an array"))
        })?;
        if rules.len() > MAX_TOOL_LOOP_RULES {
            return Err(HarnessError::corpus(format!(
                "{label}.rules exceeds {MAX_TOOL_LOOP_RULES} entries"
            )));
        }
        for rule in rules {
            let Some(capability) =
                rule.get("capability").and_then(Value::as_str)
            else {
                return Err(HarnessError::corpus(format!(
                    "{label}.rules capability is invalid"
                )));
            };
            validate_capability_identifier(
                capability,
                &format!("{label}.rules.capability"),
            )?;
            let decision = rule.get("decision").and_then(Value::as_str);
            if !matches!(decision, Some("allow" | "ask" | "deny")) {
                return Err(HarnessError::corpus(format!(
                    "{label}.rules decision is invalid"
                )));
            }
        }
    }
    if let Some(visible) = case.get("visibleTools") {
        validate_bounded_tool_names(
            visible,
            MAX_TOOL_LOOP_VISIBLE_TOOLS,
            &format!("{label}.visibleTools"),
        )?;
    }
    if let Some(cancel_after) = case.get("cancelAfterCompletedToolCalls") {
        if !cancel_after.as_u64().is_some_and(|value| value <= 128) {
            return Err(HarnessError::corpus(format!(
                "{label}.cancelAfterCompletedToolCalls is invalid"
            )));
        }
    }
    let provider = case.get("provider").ok_or_else(|| {
        HarnessError::corpus(format!("{label} requires provider"))
    })?;
    validate_tool_loop_provider(provider, &format!("{label}.provider"))?;
    Ok(())
}

fn validate_tool_loop_provider(
    provider: &Value,
    label: &str,
) -> Result<(), HarnessError> {
    let object = provider.as_object().ok_or_else(|| {
        HarnessError::corpus(format!("{label} must be an object"))
    })?;
    match provider.get("kind").and_then(Value::as_str) {
        Some("fake") => {
            if object.len() != 1 {
                return Err(HarnessError::corpus(format!(
                    "{label} fake provider has unknown fields"
                )));
            }
        }
        Some("scripted") => {
            if object.len() != 2 {
                return Err(HarnessError::corpus(format!(
                    "{label} scripted provider has unknown fields"
                )));
            }
            let event_value = provider.get("events").ok_or_else(|| {
                HarnessError::corpus(format!("{label}.events is missing"))
            })?;
            let events = if let Some(marker) = event_value
                .as_object()
                .and_then(|object| object.get("$eventsRepeat"))
            {
                let marker = marker.as_object().ok_or_else(|| {
                    HarnessError::corpus(format!(
                        "{label}.events $eventsRepeat marker is invalid"
                    ))
                })?;
                let events = marker
                    .get("events")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        HarnessError::corpus(format!(
                            "{label}.events $eventsRepeat marker is invalid"
                        ))
                    })?;
                let count = marker
                    .get("count")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        HarnessError::corpus(format!(
                            "{label}.events $eventsRepeat marker is invalid"
                        ))
                    })?;
                if count == 0
                    || count > MAX_TOOL_LOOP_EVENTS as u64
                    || events.len().saturating_mul(count as usize)
                        > MAX_TOOL_LOOP_EVENTS
                {
                    return Err(HarnessError::corpus(format!(
                        "{label}.events $eventsRepeat marker is invalid"
                    )));
                }
                events
            } else {
                event_value.as_array().ok_or_else(|| {
                    HarnessError::corpus(format!(
                        "{label}.events must be an array"
                    ))
                })?
            };
            if events.is_empty() || events.len() > MAX_TOOL_LOOP_EVENTS {
                return Err(HarnessError::corpus(format!(
                    "{label}.events must contain 1-{MAX_TOOL_LOOP_EVENTS} entries"
                )));
            }
            for event in events {
                let Some(event_object) = event.as_object() else {
                    continue;
                };
                if event_object.get("type").and_then(Value::as_str)
                    == Some("provider_error")
                    && (event_object.len() != 2
                        || !event_object
                            .get("message")
                            .and_then(Value::as_str)
                            .is_some_and(|message| !message.is_empty()))
                {
                    return Err(HarnessError::corpus(format!(
                        "{label}.events provider_error is invalid"
                    )));
                }
                if event_object.get("type").and_then(Value::as_str)
                    == Some("tool_call")
                {
                    let has_input = event_object.contains_key("input");
                    let has_input_json =
                        event_object.contains_key("inputJson");
                    if has_input_json {
                        let valid_json = event_object
                            .get("inputJson")
                            .and_then(Value::as_str)
                            .is_some_and(|text| {
                                serde_json::from_str::<Value>(text).is_ok()
                            })
                            || event_object
                                .get("inputJson")
                                .is_some_and(is_repeat_marker);
                        if !valid_json || (has_input && has_input_json) {
                            return Err(HarnessError::corpus(format!(
                                "{label}.events tool_call inputJson is invalid"
                            )));
                        }
                    }
                }
            }
        }
        _ => {
            return Err(HarnessError::corpus(format!(
                "{label}.kind must be fake or scripted"
            )));
        }
    }
    Ok(())
}

/// Strict tool-loop input shape validation (mirrors contract.mjs).
pub(in crate::harness) fn validate_tool_loop_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let object = input.as_object().ok_or_else(|| {
        HarnessError::corpus("tool-loop input must be an object")
    })?;
    if object.len() != 1 || !object.contains_key("cases") {
        return Err(HarnessError::corpus(
            "tool-loop input must contain exactly the cases field",
        ));
    }
    let cases =
        input.get("cases").and_then(Value::as_array).ok_or_else(|| {
            HarnessError::corpus("tool-loop cases must be an array")
        })?;
    if cases.is_empty() || cases.len() > MAX_TOOL_LOOP_CASES {
        return Err(HarnessError::corpus(format!(
            "tool-loop cases must contain 1-{MAX_TOOL_LOOP_CASES} entries"
        )));
    }
    for (index, case) in cases.iter().enumerate() {
        validate_tool_loop_case(case, &format!("tool-loop case {index}"))?;
    }
    Ok(())
}
