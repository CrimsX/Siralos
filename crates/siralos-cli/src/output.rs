//! Deterministic CLI rendering for the R7.5 observability commands.
//!
//! This module owns presentation only. Projection values, Tool visibility,
//! and permission decisions are calculated by the existing core services;
//! the formatter never creates authority or changes an approved surface.

use siralos_adapters::domain::PluginRecord;
use siralos_core::projection::LastProjection;
use siralos_core::tool::{
    PermissionDecision, PermissionPolicy, RegisteredToolInfo,
    evaluate_permission,
};

/// Render the current detached projection using the frozen `/context`
/// vocabulary.
pub fn format_context_status(last: Option<&LastProjection>) -> String {
    let Some(last) = last else {
        return "Context projection: not yet computed (send a prompt first)\n"
            .to_owned();
    };
    let context = &last.request.context_projection;
    let stable_bytes = context
        .stable_segments
        .iter()
        .map(|segment| segment.bytes)
        .sum::<usize>();
    let contextual_bytes = context
        .contextual_segments
        .iter()
        .map(|segment| segment.bytes)
        .sum::<usize>();
    let volatile_bytes = context
        .volatile_segments
        .iter()
        .map(|segment| segment.bytes)
        .sum::<usize>();
    let pressure = &last.request.pressure;
    let tool = &last.request.tool_projection;
    let pressure_percent = (pressure.ratio * 100.0).round() as i64;
    format!(
        "Context projection (mode {})\n  Stable: {} B (fingerprint {})\n  Contextual: {} B\n  Volatile: {} B\n  Estimated: {} tokens / {} working\n  Pressure: {} ({}%)\n  Tool ABI: {} ({} available, {} gated, {} hidden)\n\n",
        last.request.mode.as_str(),
        stable_bytes,
        fingerprint_prefix(&context.stable_fingerprint),
        contextual_bytes,
        volatile_bytes,
        last.request.estimated_tokens,
        pressure.working_maximum,
        pressure.state.as_str(),
        pressure_percent,
        fingerprint_prefix(&tool.fingerprint),
        tool.counts.available,
        tool.counts.gated,
        tool.counts.hidden,
    )
}

/// Render the compact current Tool projection used by `/tools`.
pub fn format_tool_projection(last: Option<&LastProjection>) -> String {
    let Some(last) = last else {
        return "Tool projection: not yet computed\n".to_owned();
    };
    let tool = &last.request.tool_projection;
    format!(
        "Tool projection: {} available, {} gated, {} hidden (ABI {})\n",
        tool.counts.available,
        tool.counts.gated,
        tool.counts.hidden,
        fingerprint_prefix(&tool.fingerprint),
    )
}

/// Render registered Tools with their existing Host permission decisions.
pub fn format_tools(
    tools: &[RegisteredToolInfo],
    policy: &PermissionPolicy,
) -> String {
    if tools.is_empty() {
        return "Available tools:\n  (none)\n".to_owned();
    }
    let lines = tools
        .iter()
        .map(|info| {
            let kind = match info.capability.as_str() {
                "workspace.write" => "write",
                "godot.probe_project" => "reviewable",
                _ => "read-only",
            };
            let status = match evaluate_permission(&info.capability, policy) {
                PermissionDecision::Deny { .. } => "denied",
                PermissionDecision::Ask { .. } => "approval required",
                PermissionDecision::Allow => "allowed",
            };
            format!(
                "  {} - {} ({}, {})",
                info.definition.name,
                info.definition.description,
                kind,
                status,
            )
        })
        .collect::<Vec<_>>();
    format!("Available tools:\n{}\n", lines.join("\n"))
}

fn fingerprint_prefix(fingerprint: &str) -> &str {
    &fingerprint[..fingerprint.len().min(8)]
}

/// Activation B4 (decision 100): bounded trailing audit segment for opted-in
/// sessions — counters in pinned declaration order + the last 8 tick records
/// oldest-first. Pure function of the in-memory ring + counters; identical
/// state -> identical bytes. Empty ring -> counters only, no ring lines.
/// The caller must gate on opt-in + successful build; OFF returns empty
/// (byte-transparent). Bounded by construction (12 counters, 8 records,
/// fixed per-record line shape).
#[must_use]
pub fn format_context_audit(
    session: Option<&siralos_adapters::context_session::ContextSystemSession>,
) -> String {
    let Some(session) = session else {
        return String::new();
    };
    let m = &session.metrics;
    let mut out = String::new();
    out.push_str("Context audit:\n");
    out.push_str("  counters:\n");
    out.push_str(&format!("    ticks_total: {}\n", m.ticks_total));
    out.push_str(&format!(
        "    coalesced_noop_ticks_total: {}\n",
        m.coalesced_noop_ticks_total
    ));
    out.push_str(&format!("    events_total: {}\n", m.events_total));
    out.push_str(&format!(
        "    events_dropped_total: {}\n",
        m.events_dropped_total
    ));
    out.push_str(&format!(
        "    demand_updates_total: {}\n",
        m.demand_updates_total
    ));
    out.push_str(&format!("    promotions_total: {}\n", m.promotions_total));
    out.push_str(&format!("    demotions_total: {}\n", m.demotions_total));
    out.push_str(&format!(
        "    stale_demotions_total: {}\n",
        m.stale_demotions_total
    ));
    out.push_str(&format!(
        "    pin_quota_demotions_total: {}\n",
        m.pin_quota_demotions_total
    ));
    out.push_str(&format!(
        "    budget_demotions_total: {}\n",
        m.budget_demotions_total
    ));
    out.push_str(&format!(
        "    assembled_summary_tokens_total: {}\n",
        m.assembled_summary_tokens_total
    ));
    out.push_str(&format!(
        "    neighbor_stub_tokens_total: {}\n",
        m.neighbor_stub_tokens_total
    ));
    let records = m.records();
    if records.is_empty() {
        return out;
    }
    out.push_str("  ring (last 8):\n");
    let start = records.len().saturating_sub(8);
    for rec in &records[start..] {
        out.push_str(&format!(
            "    tick {}: now={} canonical_event_count={} events_dropped={} tier_counts hot={} warm={} cold={} archive={} assembled_unique_total={} assembled_summary_total={} stub_total={} demotion_counts stale={} pin_quota={} budget={} promotion_count={}\n",
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
        ));
    }
    out
}

/// Render the installed domains view (`/domains`): the deterministic
/// empty state, or the recorded plugin list sorted by id.
pub fn format_domains(records: &[PluginRecord]) -> String {
    if records.is_empty() {
        return "No domains installed.\n[Add Plugin] /domains-add <folder>\n"
            .to_owned();
    }
    let lines = records
        .iter()
        .map(|record| {
            let short_digest = record
                .digest
                .strip_prefix("sha256:")
                .unwrap_or(&record.digest)
                .chars()
                .take(8)
                .collect::<String>();
            format!(
                "  {} (digest {}, path {})",
                record.id, short_digest, record.path,
            )
        })
        .collect::<Vec<_>>();
    format!("Domains installed:\n{}\n", lines.join("\n"))
}

/// Render the outcome of one `/domains-add` flow.
pub fn format_plugin_added(record: &PluginRecord) -> String {
    let digest =
        record.digest.strip_prefix("sha256:").unwrap_or(&record.digest);
    format!("Installed {} (digest sha256:{digest}).\n", record.id)
}

#[cfg(test)]
mod tests {
    use super::{format_context_status, format_tool_projection, format_tools};
    use siralos_adapters::tool::{
        WorkspaceListTool, WorkspaceReadTool, WorkspaceSearchTool,
    };
    use siralos_core::projection::{
        ProjectionInput, ProjectionService,
        capacity::ContextCapacity,
        evidence::EvidenceProjectorOptions,
        segments::{SegmentInput, Stability},
    };
    use siralos_core::tool::{
        PermissionPolicy, PermissionRule, PolicyRule, ToolRegistry,
    };

    fn registry() -> ToolRegistry {
        let root = std::env::temp_dir();
        let tools: Vec<Box<dyn siralos_core::tool::Tool>> = vec![
            Box::new(WorkspaceListTool::new(&root).expect("list tool")),
            Box::new(WorkspaceReadTool::new(&root).expect("read tool")),
            Box::new(WorkspaceSearchTool::new(&root).expect("search tool")),
        ];
        ToolRegistry::new(tools).expect("unique tool names")
    }

    fn allow_policy() -> PermissionPolicy {
        PermissionPolicy::from_rules([PolicyRule {
            capability: siralos_core::tool::CapabilityId::parse(
                "workspace.read",
            )
            .expect("capability"),
            rule: PermissionRule::Allow,
        }])
    }

    fn last_projection(
        segments: Vec<SegmentInput>,
        capacity: ContextCapacity,
        registered_tools: &[siralos_core::tool::RegisteredToolInfo],
        policy: &PermissionPolicy,
        allowed_tool_names: Option<Vec<String>>,
    ) -> siralos_core::projection::LastProjection {
        let mut service = ProjectionService::new();
        service.project(ProjectionInput {
            mode:
                siralos_core::projection::visibility::ProjectionMode::Generic,
            messages: &[],
            registered_tools,
            provider_tool_calling: true,
            capacity,
            pressure_limits: Default::default(),
            segments,
            evidence_options: EvidenceProjectorOptions::default(),
            allowed_tool_names,
            policy,
            task_revision: None,
        });
        service.last_projection().expect("projection").clone()
    }

    #[test]
    fn empty_and_uncomputed_renderings_are_stable() {
        assert_eq!(
            format_context_status(None),
            "Context projection: not yet computed (send a prompt first)\n"
        );
        assert_eq!(
            format_tool_projection(None),
            "Tool projection: not yet computed\n"
        );
        assert_eq!(
            format_tools(&[], &PermissionPolicy::default()),
            "Available tools:\n  (none)\n"
        );
    }

    #[test]
    fn context_rendering_uses_projected_values_and_pressure() {
        let tools = registry();
        let policy = allow_policy();
        let registered = tools.definitions();
        let last = last_projection(
            vec![
                SegmentInput {
                    id: "stable".to_owned(),
                    stability: Stability::Stable,
                    title: "Stable".to_owned(),
                    content: "abc".to_owned(),
                },
                SegmentInput {
                    id: "contextual".to_owned(),
                    stability: Stability::Contextual,
                    title: "Context".to_owned(),
                    content: "de".to_owned(),
                },
                SegmentInput {
                    id: "volatile".to_owned(),
                    stability: Stability::Volatile,
                    title: "Volatile".to_owned(),
                    content: "f".to_owned(),
                },
            ],
            ContextCapacity::with_working_maximum(1),
            &registered,
            &policy,
            Some(vec!["workspace.list".to_owned()]),
        );
        let context = format_context_status(Some(&last));
        assert!(context.starts_with("Context projection (mode generic)\n"));
        assert!(context.contains("  Stable: 3 B (fingerprint "));
        assert!(context.contains("  Contextual: 2 B\n  Volatile: 1 B\n"));
        assert!(context.contains("  Estimated: "));
        assert!(context.contains("  Pressure: hard ("));
        assert!(context.contains("  Tool ABI: "));
        assert!(context.contains("(1 available, 0 gated, 2 hidden)\n\n"));
        assert_eq!(
            format_tool_projection(Some(&last)),
            format!(
                "Tool projection: 1 available, 0 gated, 2 hidden (ABI {})\n",
                &last.request.tool_projection.fingerprint[..8]
            )
        );
    }

    #[test]
    fn tools_render_in_registration_order_and_use_the_same_policy() {
        let tools = registry();
        let definitions = tools.definitions();
        let policy = allow_policy();
        let rendered = format_tools(&definitions, &policy);
        let list = rendered.find("workspace.list").expect("list");
        let read = rendered.find("workspace.read").expect("read");
        let search = rendered.find("workspace.search").expect("search");
        assert!(list < read && read < search);
        assert!(rendered.contains("(read-only, allowed)"));

        let denied = PermissionPolicy::from_rules([PolicyRule {
            capability: siralos_core::tool::CapabilityId::parse(
                "workspace.read",
            )
            .expect("capability"),
            rule: PermissionRule::Deny,
        }]);
        assert!(
            format_tools(&definitions, &denied)
                .contains("(read-only, denied)")
        );
        let ask = PermissionPolicy::from_rules([PolicyRule {
            capability: siralos_core::tool::CapabilityId::parse(
                "workspace.read",
            )
            .expect("capability"),
            rule: PermissionRule::Ask,
        }]);
        assert!(
            format_tools(&definitions, &ask)
                .contains("(read-only, approval required)")
        );
        let gated_last = last_projection(
            Vec::new(),
            ContextCapacity::default(),
            &definitions,
            &ask,
            None,
        );
        assert_eq!(
            format_tool_projection(Some(&gated_last)),
            format!(
                "Tool projection: 0 available, 3 gated, 0 hidden (ABI {})\n",
                &gated_last.request.tool_projection.fingerprint[..8]
            )
        );
    }

    // B4 (decision 100) — audit surface tests (~7)
    #[test]
    fn audit_off_transparency_is_byte_identical() {
        use crate::output::format_context_audit;
        assert_eq!(format_context_audit(None), "");
        let base =
            "Context projection: not yet computed (send a prompt first)\n";
        let audit = format_context_audit(None);
        let combined = if audit.is_empty() {
            base.to_owned()
        } else {
            format!("{base}{audit}")
        };
        assert_eq!(combined, base);
    }

    #[test]
    fn audit_on_renders_counters_in_pinned_order() {
        use crate::output::format_context_audit;
        use siralos_adapters::context_session::build_context_system;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir()
            .join(format!("siralos-audit-counters-{nonce}"));
        std::fs::create_dir_all(&root).expect("temp dir");
        std::fs::write(root.join("a.txt"), b"alpha").expect("write");
        let build = build_context_system(&root, true);
        let mut session = build.session.expect("session");
        let obs = siralos_adapters::tool::context_events::ToolObservation::new(
            "context.inspect",
            serde_json::json!({ "node_id": "a.txt" }),
            siralos_core::provider::ToolExecutionResult::Success {
                output: serde_json::json!({ "id": "a.txt" }),
                summary: "inspect a.txt".to_owned(),
            },
        );
        let _ = session.drive_tick(std::slice::from_ref(&obs));
        let audit = format_context_audit(Some(&session));
        assert!(audit.contains("Context audit:\n"));
        assert!(audit.contains("  counters:\n"));
        let order = [
            "    ticks_total:",
            "    coalesced_noop_ticks_total:",
            "    events_total:",
            "    events_dropped_total:",
            "    demand_updates_total:",
            "    promotions_total:",
            "    demotions_total:",
            "    stale_demotions_total:",
            "    pin_quota_demotions_total:",
            "    budget_demotions_total:",
            "    assembled_summary_tokens_total:",
            "    neighbor_stub_tokens_total:",
        ];
        let mut last_pos = 0usize;
        for needle in order {
            let pos = audit[last_pos..].find(needle).expect(needle);
            last_pos += pos + needle.len();
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn audit_on_renders_last_8_records_oldest_first_bounded() {
        use crate::output::format_context_audit;
        use siralos_adapters::context_session::build_context_system;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("siralos-audit-ring-{nonce}"));
        std::fs::create_dir_all(&root).expect("temp dir");
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(root.join(name), format!("body {name}").as_bytes())
                .expect("write");
        }
        let build = build_context_system(&root, true);
        let mut session = build.session.expect("session");
        for i in 0..10 {
            let node = match i % 3 {
                0 => "a.txt",
                1 => "b.txt",
                _ => "c.txt",
            };
            let obs =
                siralos_adapters::tool::context_events::ToolObservation::new(
                    "context.inspect",
                    serde_json::json!({ "node_id": node }),
                    siralos_core::provider::ToolExecutionResult::Success {
                        output: serde_json::json!({ "id": node }),
                        summary: format!("inspect {node}"),
                    },
                );
            let _ = session.drive_tick(std::slice::from_ref(&obs));
        }
        let audit = format_context_audit(Some(&session));
        let ring_lines: Vec<&str> = audit
            .lines()
            .filter(|l| l.trim_start().starts_with("tick "))
            .collect();
        assert_eq!(ring_lines.len(), 8, "expected last 8 records");
        assert!(
            ring_lines[0].contains("now=3"),
            "first ring line should be oldest of last 8, got {}",
            ring_lines[0]
        );
        assert!(
            ring_lines[7].contains("now=10"),
            "last ring line should be newest, got {}",
            ring_lines[7]
        );
        for line in &ring_lines {
            assert!(line.contains("canonical_event_count="));
            assert!(line.contains("events_dropped="));
            assert!(line.contains("tier_counts"));
            assert!(line.contains("assembled_unique_total="));
            assert!(line.contains("demotion_counts"));
            assert!(line.contains("promotion_count="));
            assert!(line.len() < 400, "ring line should be bounded");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn audit_determinism_is_byte_equal() {
        use crate::output::format_context_audit;
        use siralos_adapters::context_session::build_context_system;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir()
            .join(format!("siralos-audit-determ-twice-{nonce}"));
        std::fs::create_dir_all(&root).expect("temp");
        std::fs::write(root.join("a.txt"), b"alpha").expect("write");
        let build = build_context_system(&root, true);
        let mut session = build.session.expect("session");
        let obs = siralos_adapters::tool::context_events::ToolObservation::new(
            "context.inspect",
            serde_json::json!({ "node_id": "a.txt" }),
            siralos_core::provider::ToolExecutionResult::Success {
                output: serde_json::json!({ "id": "a.txt" }),
                summary: "inspect a.txt".to_owned(),
            },
        );
        let _ = session.drive_tick(std::slice::from_ref(&obs));
        let once = format_context_audit(Some(&session));
        let twice = format_context_audit(Some(&session));
        assert_eq!(once, twice, "identical state -> identical bytes");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn audit_segment_flows_through_sanitizer() {
        use crate::output::format_context_audit;
        use crate::sanitize::sanitize_for_display;
        use siralos_adapters::context_session::build_context_system;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir()
            .join(format!("siralos-audit-sanitize-{nonce}"));
        std::fs::create_dir_all(&root).expect("temp");
        std::fs::write(root.join("a.txt"), b"alpha").expect("write");
        let build = build_context_system(&root, true);
        let mut session = build.session.expect("session");
        let obs = siralos_adapters::tool::context_events::ToolObservation::new(
            "context.inspect",
            serde_json::json!({ "node_id": "a.txt" }),
            siralos_core::provider::ToolExecutionResult::Success {
                output: serde_json::json!({ "id": "a.txt" }),
                summary: "inspect a.txt".to_owned(),
            },
        );
        let _ = session.drive_tick(std::slice::from_ref(&obs));
        let audit = format_context_audit(Some(&session));
        assert_eq!(sanitize_for_display(&audit), audit);
        assert_eq!(sanitize_for_display("\u{1b}[31mhello"), "hello");
        assert_eq!(
            sanitize_for_display("\u{1b}]8;;https://example.com\u{07}ok"),
            "ok"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn audit_render_does_not_drain_or_clear_ring() {
        use crate::output::format_context_audit;
        use siralos_adapters::context_session::build_context_system;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("siralos-audit-nomut-{nonce}"));
        std::fs::create_dir_all(&root).expect("temp");
        std::fs::write(root.join("a.txt"), b"alpha").expect("write");
        let build = build_context_system(&root, true);
        let mut session = build.session.expect("session");
        let obs = siralos_adapters::tool::context_events::ToolObservation::new(
            "context.inspect",
            serde_json::json!({ "node_id": "a.txt" }),
            siralos_core::provider::ToolExecutionResult::Success {
                output: serde_json::json!({ "id": "a.txt" }),
                summary: "inspect a.txt".to_owned(),
            },
        );
        let _ = session.drive_tick(std::slice::from_ref(&obs));
        let len_before = session.metrics.records().len();
        let audit_before = format_context_audit(Some(&session));
        let len_after = session.metrics.records().len();
        let audit_after = format_context_audit(Some(&session));
        assert_eq!(len_before, len_after);
        assert_eq!(audit_before, audit_after);
        assert_eq!(session.metrics.ticks_total, 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn audit_empty_ring_renders_counters_only() {
        use crate::output::format_context_audit;
        use siralos_adapters::context_session::build_context_system;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("siralos-audit-empty-{nonce}"));
        std::fs::create_dir_all(&root).expect("temp");
        std::fs::write(root.join("a.txt"), b"alpha").expect("write");
        let build = build_context_system(&root, true);
        let session = build.session.expect("session");
        assert_eq!(session.metrics.records().len(), 0);
        let audit = format_context_audit(Some(&session));
        assert!(audit.contains("  counters:\n"));
        assert!(
            !audit.contains("ring (last 8)"),
            "empty ring should not emit ring lines, got {audit}"
        );
        assert!(!audit.lines().any(|l| l.trim_start().starts_with("tick ")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
