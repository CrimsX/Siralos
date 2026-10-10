//! The context pane: decision 100 counters, the tick ring, host tool
//! activity, and the right-hand pane row composition (T3).

use super::truncate_to_width;

/// Fixed width of the T3 context pane in columns (right-hand pane).
pub const CONTEXT_PANE_WIDTH: u16 = 40;

/// Maximum tick records shown in the context pane ring block (same bound as
/// the decision 100 audit segment: the LAST 8, oldest-first).
pub const CONTEXT_PANE_RING_LIMIT: usize = 8;

/// Maximum tool rounds shown in the context pane activity block (the LAST 8,
/// oldest-first).
pub const CONTEXT_PANE_ACTIVITY_LIMIT: usize = 8;

/// Pinned counter order for the context pane — the SAME 12 counters in the
/// SAME declaration order as the decision 100 audit segment (`output.rs`
/// `format_context_audit`, decision 100 R1a). The pane reads the same
/// `ContextMetrics` state; the order is pinned here, not re-derived.
pub const CONTEXT_COUNTER_ORDER: [&str; 12] = [
    "ticks_total",
    "coalesced_noop_ticks_total",
    "events_total",
    "events_dropped_total",
    "demand_updates_total",
    "promotions_total",
    "demotions_total",
    "stale_demotions_total",
    "pin_quota_demotions_total",
    "budget_demotions_total",
    "assembled_summary_tokens_total",
    "neighbor_stub_tokens_total",
];

/// One host-observed tool round for the pane activity block: the tool name
/// plus the typed result status (the `status_str` vocabulary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActivityEntry {
    /// Tool name from host-observed history (sanitized at extraction).
    pub tool_name: String,
    /// Typed result status (`success`, `failed`, …).
    pub status: String,
}

/// Read-only snapshot behind the T3 context pane, derived from the SAME
/// host-side sources the `/context` audit segment uses: the live
/// `ContextMetrics` (counters + tick ring) and the host-observed
/// conversation history (tool activity). No persistence, no mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPaneData {
    /// The 12 counters in [`CONTEXT_COUNTER_ORDER`] pinned order.
    pub counters: Vec<(String, u64)>,
    /// The LAST 8 tick records, oldest-first (same records the audit
    /// segment renders).
    pub ring: Vec<siralos_core::context_metrics::TickRecord>,
    /// The LAST 8 host-observed tool rounds, oldest-first.
    pub activity: Vec<ToolActivityEntry>,
}

/// Read the 12 counters from `metrics` in pinned order.
///
/// Single source (P3): this reads the SAME `ContextMetrics` the
/// `format_context_audit` segment reads — no parallel extraction, no
/// re-derived field logic. Values are host-side numbers (sanitizer-clean
/// by construction).
#[must_use]
pub fn context_counters(
    metrics: &siralos_core::context_metrics::ContextMetrics,
) -> Vec<(String, u64)> {
    let values = [
        metrics.ticks_total,
        metrics.coalesced_noop_ticks_total,
        metrics.events_total,
        metrics.events_dropped_total,
        metrics.demand_updates_total,
        metrics.promotions_total,
        metrics.demotions_total,
        metrics.stale_demotions_total,
        metrics.pin_quota_demotions_total,
        metrics.budget_demotions_total,
        metrics.assembled_summary_tokens_total,
        metrics.neighbor_stub_tokens_total,
    ];
    CONTEXT_COUNTER_ORDER
        .iter()
        .zip(values)
        .map(|(name, value)| ((*name).to_owned(), value))
        .collect()
}

/// Slice the LAST 8 tick records oldest-first — the same window the decision
/// 100 audit segment renders (`records.len().saturating_sub(8)`).
#[must_use]
pub fn context_ring_tail(
    metrics: &siralos_core::context_metrics::ContextMetrics,
) -> Vec<siralos_core::context_metrics::TickRecord> {
    let records = metrics.records();
    let start = records.len().saturating_sub(CONTEXT_PANE_RING_LIMIT);
    records[start..].to_vec()
}

/// Format one tick record with the SAME field names as the decision 100
/// audit segment (decision 100 R1b). The pane truncates this line to the
/// pane width at render; the full form here is what the single-source test
/// compares against the `/context` segment.
#[must_use]
pub fn format_tick_record_line(
    rec: &siralos_core::context_metrics::TickRecord,
) -> String {
    format!(
        "    tick {}: now={} canonical_event_count={} events_dropped={} tier_counts hot={} warm={} cold={} archive={} assembled_unique_total={} assembled_summary_total={} stub_total={} demotion_counts stale={} pin_quota={} budget={} promotion_count={}",
        rec.now,
        rec.now,
        rec.canonical_event_count,
        rec.events_dropped,
        rec.tier_counts.hot,
        rec.tier_counts.warm,
        rec.tier_counts.cold,
        rec.tier_counts.archive,
        rec.assembled_unique_total,
        rec.assembled_summary_total,
        rec.stub_total,
        rec.demotion_counts.stale,
        rec.demotion_counts.pin_quota,
        rec.demotion_counts.budget,
        rec.promotion_count,
    )
}

/// Extract the LAST 8 host-observed tool rounds (oldest-first) from the
/// authoritative conversation history: every `ToolResult` in order with its
/// tool name and typed result status.
///
/// Sanitizer discipline (P4): the tool name passes through the same
/// `sanitize_for_display` boundary the `/context` segment applies; the
/// status is the static typed vocabulary (`status_str`). The pane
/// introduces no new unsanitized source.
#[must_use]
pub fn tool_activity_from_history(
    history: &[siralos_core::provider::ConversationItem],
) -> Vec<ToolActivityEntry> {
    use siralos_core::provider::ConversationItem;
    let mut entries: Vec<ToolActivityEntry> = Vec::new();
    for item in history {
        if let ConversationItem::ToolResult { tool_name, result, .. } = item {
            entries.push(ToolActivityEntry {
                tool_name: crate::sanitize::sanitize_for_display(tool_name),
                status: result.status_str().to_owned(),
            });
        }
    }
    let start = entries.len().saturating_sub(CONTEXT_PANE_ACTIVITY_LIMIT);
    entries[start..].to_vec()
}

/// Build the pane snapshot when the subsystem is opted in AND built — the
/// SAME gating as the decision 100 `/context` segment
/// (`context_system_enabled && session.is_some()`).
///
/// OFF (`!enabled` or `metrics` absent) returns `None`: the frame renders
/// byte-identical to T2's (no pane, no placeholder). Read-only over
/// in-memory state (P7): counters, ring, and history are borrowed, never
/// drained or cleared.
#[must_use]
pub fn build_context_pane(
    enabled: bool,
    metrics: Option<&siralos_core::context_metrics::ContextMetrics>,
    history: &[siralos_core::provider::ConversationItem],
) -> Option<ContextPaneData> {
    if !enabled {
        return None;
    }
    let metrics = metrics?;
    Some(ContextPaneData {
        counters: context_counters(metrics),
        ring: context_ring_tail(metrics),
        activity: tool_activity_from_history(history),
    })
}

/// Render the full pane line list: counters block, ring block, tool
/// activity block, top to bottom. Empty ring/activity renders the block
/// header with no lines. Every line is bounded to `inner_width`
/// (deterministic truncation).
#[must_use]
pub fn context_pane_lines(
    pane: &ContextPaneData,
    inner_width: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(truncate_to_width("counters:", inner_width));
    for (name, value) in &pane.counters {
        lines.push(truncate_to_width(
            &format!("  {name}: {value}"),
            inner_width,
        ));
    }
    lines.push(truncate_to_width("ring (last 8):", inner_width));
    for rec in &pane.ring {
        lines.push(truncate_to_width(
            &format_tick_record_line(rec),
            inner_width,
        ));
    }
    lines.push(truncate_to_width("tools (last 8):", inner_width));
    for entry in &pane.activity {
        lines.push(truncate_to_width(
            &format!("  {} {}", entry.tool_name, entry.status),
            inner_width,
        ));
    }
    lines
}
