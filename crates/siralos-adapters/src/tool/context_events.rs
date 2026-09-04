//! Host-owned demand edge — pure derivation of AccessEvents from tool results.
//! Never inside pure tool functions, never in ContextToolState, never model-supplied.
//! The helper is deterministic, integer-only, and host-owned.

use serde_json::Value;
use siralos_core::context_scheduler::AccessEvent;
use siralos_core::provider::ToolExecutionResult;

/// Derive canonical AccessEvents from a host-observed tool call + result.
///
/// `tool_name` is the registered tool name (`context.inspect`, `context.search`,
/// `context.expand`). `input` is the JSON input, `result` is the execution result.
/// The derivation is host-owned: only the host seam calls this, tools remain
/// read-only over immutable snapshots.
///
/// - `context.search`: every hit's `node_id` becomes an event.
/// - `context.inspect`/`context.expand`: the input `node_id` becomes an event on success.
/// - otherwise: no events.
///
/// The returned vec is not yet canonicalized; callers should canonicalize via
/// `canonicalize_events` or `TickInput::new` which does it.
#[must_use]
pub fn derive_access_events(
    tool_name: &str,
    input: &Value,
    result: &ToolExecutionResult,
) -> Vec<AccessEvent> {
    match result {
        ToolExecutionResult::Success { output, .. } => {
            derive_from_success(tool_name, input, output)
        }
        _ => Vec::new(),
    }
}

fn derive_from_success(
    tool_name: &str,
    input: &Value,
    output: &Value,
) -> Vec<AccessEvent> {
    match tool_name {
        "context.search" => {
            // output: { query, hits: [{ node_id, ... }], ... }
            let Some(hits) = output.get("hits").and_then(|v| v.as_array())
            else {
                return Vec::new();
            };
            let mut events = Vec::new();
            for hit in hits {
                if let Some(node_id) =
                    hit.get("node_id").and_then(|v| v.as_str())
                {
                    if !node_id.is_empty() && node_id.len() <= 256 {
                        events.push(AccessEvent::new(node_id.to_owned()));
                    }
                } else if let Some(node_id) =
                    hit.get("nodeId").and_then(|v| v.as_str())
                {
                    if !node_id.is_empty() && node_id.len() <= 256 {
                        events.push(AccessEvent::new(node_id.to_owned()));
                    }
                }
            }
            events
        }
        "context.inspect" | "context.expand" => {
            // Use input node_id on success
            if let Some(node_id) = input
                .get("node_id")
                .and_then(|v| v.as_str())
                .or_else(|| input.get("nodeId").and_then(|v| v.as_str()))
            {
                if !node_id.is_empty() && node_id.len() <= 256 {
                    return vec![AccessEvent::new(node_id.to_owned())];
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use siralos_core::provider::ToolExecutionResult;

    fn success(output: Value) -> ToolExecutionResult {
        ToolExecutionResult::Success { output, summary: String::new() }
    }

    #[test]
    fn derive_from_search_hits() {
        let input = json!({ "query": "auth" });
        let output = json!({ "query": "auth", "hits": [{ "node_id": "ctx-a" }, { "node_id": "ctx-b" }], "truncated": false, "hitCount": 2 });
        let events =
            derive_access_events("context.search", &input, &success(output));
        assert_eq!(
            events,
            vec![AccessEvent::new("ctx-a"), AccessEvent::new("ctx-b")]
        );
    }

    #[test]
    fn derive_from_inspect_success() {
        let input = json!({ "node_id": "ctx-a" });
        let output = json!({ "id": "ctx-a" });
        let events =
            derive_access_events("context.inspect", &input, &success(output));
        assert_eq!(events, vec![AccessEvent::new("ctx-a")]);
    }

    #[test]
    fn derive_from_expand_success() {
        let input = json!({ "node_id": "ctx-a", "level": "structured" });
        let output = json!({ "node_id": "ctx-a" });
        let events =
            derive_access_events("context.expand", &input, &success(output));
        assert_eq!(events, vec![AccessEvent::new("ctx-a")]);
    }

    #[test]
    fn derive_empty_on_failure() {
        let input = json!({ "query": "x" });
        let fail =
            ToolExecutionResult::Failed { message: "not found".to_owned() };
        assert!(
            derive_access_events("context.search", &input, &fail).is_empty()
        );
        assert!(
            derive_access_events("context.inspect", &input, &fail).is_empty()
        );
    }

    #[test]
    fn derive_no_mutation_purity() {
        // Pure function: same input -> same output, no state mutation observed
        let input = json!({ "query": "auth" });
        let output = json!({ "hits": [{ "node_id": "ctx-a" }] });
        let r1 = derive_access_events(
            "context.search",
            &input,
            &success(output.clone()),
        );
        let r2 =
            derive_access_events("context.search", &input, &success(output));
        assert_eq!(r1, r2);
    }
}
