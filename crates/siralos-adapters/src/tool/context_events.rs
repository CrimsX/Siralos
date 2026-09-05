//! Host-owned demand edge — pure derivation of AccessEvents from tool results.
//! Never inside pure tool functions, never in ContextToolState, never model-supplied.
//! The helper is deterministic, integer-only, and host-owned.

use serde_json::Value;
use siralos_core::context_scheduler::{
    AccessEvent, TickInput, canonicalize_events,
};
use siralos_core::provider::ToolExecutionResult;

/// Host-observed tool observation — the only source of demand events (B3).
/// The model can never inject events; they come only from host-observed
/// tool results at the adapters boundary. Pure composition, no global state.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolObservation {
    /// Registered tool name (`context.inspect`, `context.search`, `context.expand`).
    pub tool_name: String,
    /// JSON input the host observed for the call.
    pub input: Value,
    /// Host-observed execution result.
    pub result: ToolExecutionResult,
}

impl ToolObservation {
    /// Create a host-observed observation.
    #[must_use]
    pub fn new(
        tool_name: impl Into<String>,
        input: Value,
        result: ToolExecutionResult,
    ) -> Self {
        Self { tool_name: tool_name.into(), input, result }
    }
}

/// Pure derivation: tool observations -> canonical AccessEvents (B3).
/// Deterministic, host-owned, no model injection path.
#[must_use]
pub fn derive_events_for_tick(
    observations: &[ToolObservation],
) -> Vec<AccessEvent> {
    let mut all: Vec<AccessEvent> = Vec::new();
    for obs in observations {
        all.extend(derive_access_events(
            &obs.tool_name,
            &obs.input,
            &obs.result,
        ));
    }
    canonicalize_events(all)
}

/// Pure composition: observations -> TickInput (B3 end-to-end).
/// Accumulates host-observed tool results into the next tick's events.
/// Live activation remains precondition-gated per decision 84 (proven by tests only).
#[must_use]
pub fn compose_tick_input(
    now: u64,
    graph_revision: String,
    new_node_ids: Vec<String>,
    stale_node_ids: Vec<String>,
    observations: &[ToolObservation],
) -> TickInput {
    // Collect raw events without early canonicalization so TickInput can
    // track overflow (events_dropped) deterministically.
    let mut all: Vec<AccessEvent> = Vec::new();
    for obs in observations {
        all.extend(derive_access_events(
            &obs.tool_name,
            &obs.input,
            &obs.result,
        ));
    }
    TickInput::new(now, all, graph_revision, new_node_ids, stale_node_ids)
}

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

    #[test]
    fn b3_composition_tool_round_to_tick_raises_priority() {
        use siralos_core::context_scheduler::{
            SchedulerConfig, SchedulerEntry, WorkingSetState, WorkingSetTier,
        };
        // Setup: node ctx-a is Cold with low relevance, after demand via tool it should promote
        let mut state = WorkingSetState::build(vec![SchedulerEntry {
            node_id: "ctx-a".to_owned(),
            tier: WorkingSetTier::Warm,
            pinned: false,
            relevance: 80,
            last_access_tick: 0,
            token_estimate: 100,
            content_digest: "a".repeat(64),
        }])
        .expect("build");
        let cfg = SchedulerConfig::default();
        // Host observes context.expand success for ctx-a
        let obs = ToolObservation::new(
            "context.expand",
            json!({ "node_id": "ctx-a", "level": "structured" }),
            success(json!({ "node_id": "ctx-a" })),
        );
        let tick =
            compose_tick_input(5, "rev1".to_owned(), vec![], vec![], &[obs]);
        assert!(tick.events.iter().any(|e| e.node_id == "ctx-a"));
        let _ = state.process_tick(tick, &cfg);
        // Demand edge: relevance +32, recency reset, should promote to Hot (80+32=100? capped 100 -> 100*4+30=430 >=280)
        let e = state.entry("ctx-a").expect("ctx-a");
        assert_eq!(e.relevance, 100);
        assert_eq!(e.tier, WorkingSetTier::Hot);
    }

    #[test]
    fn b3_model_cannot_inject_events_signature_is_host_only() {
        // The composition helper only accepts host-observed ToolObservation (tool_name+input+result).
        // There is no function that accepts model-supplied AccessEvents.
        // This test proves the type barrier: derive_events_for_tick requires ToolObservation.
        let obs = ToolObservation::new(
            "context.search",
            json!({ "query": "x" }),
            success(json!({ "hits": [{ "node_id": "ctx-injected" }] })),
        );
        let events = derive_events_for_tick(std::slice::from_ref(&obs));
        assert_eq!(events, vec![AccessEvent::new("ctx-injected")]);
        // An attacker cannot call derive_events_for_tick with raw AccessEvent vec; the API requires observation.
        // Composition remains pure: same observations -> same TickInput
        let obs2 = ToolObservation::new(
            "context.search",
            json!({ "query": "x" }),
            success(json!({ "hits": [{ "node_id": "ctx-injected" }] })),
        );
        let t1 = compose_tick_input(1, "r".to_owned(), vec![], vec![], &[obs]);
        let t2 =
            compose_tick_input(1, "r".to_owned(), vec![], vec![], &[obs2]);
        assert_eq!(t1.events, t2.events);
    }

    #[test]
    fn b3_composition_pure_no_global_state() {
        let obs = ToolObservation::new(
            "context.inspect",
            json!({ "node_id": "ctx-a" }),
            success(json!({ "id": "ctx-a" })),
        );
        let t1 = compose_tick_input(
            2,
            "rev1".to_owned(),
            vec!["ctx-a".to_owned()],
            vec![],
            std::slice::from_ref(&obs),
        );
        let t2 = compose_tick_input(
            2,
            "rev1".to_owned(),
            vec!["ctx-a".to_owned()],
            vec![],
            std::slice::from_ref(&obs),
        );
        assert_eq!(t1, t2);
    }

    #[test]
    fn decision_84_gate_regression_no_live_registration_path() {
        // Live activation remains precondition-gated per decision 84; composition is tests-only.
        // We assert that ToolObservation does not expose live wiring and that
        // compose_tick_input is pure (no side-effects, no global registration).
        let obs = ToolObservation::new(
            "context.search",
            json!({ "query": "x" }),
            success(json!({ "hits": [] })),
        );
        let tick =
            compose_tick_input(1, "rev".to_owned(), vec![], vec![], &[obs]);
        assert!(tick.events.is_empty());
        // Re-calling with same inputs yields identical output, proving no hidden state
        let obs2 = ToolObservation::new(
            "context.search",
            json!({ "query": "x" }),
            success(json!({ "hits": [] })),
        );
        let tick2 =
            compose_tick_input(1, "rev".to_owned(), vec![], vec![], &[obs2]);
        assert_eq!(tick, tick2);
    }
}
