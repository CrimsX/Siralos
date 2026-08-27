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
}
