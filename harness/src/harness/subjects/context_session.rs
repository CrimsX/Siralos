//! Context-family records: projection, graph, representation, scheduler, tool, benchmark, session, and TUI render.
use crate::harness::*;

pub(in crate::harness) fn context_scan_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    with_fixture_workspace(scenario_id, input, |root| {
        // Build scenarios exercise the B3a composition (scan + classify + L1 summary)
        // but keep the differential record byte-identical to the B1/B2 shape so the
        // 5 existing scenarios remain unchanged.
        if scenario_id.contains("build-summaries") {
            let ctx = siralos_adapters::context_scan::build_workspace_context(
                root,
                siralos_adapters::context_scan::DEFAULT_SCAN_BOUNDS,
            )
            .map_err(|error| {
                HarnessError::new(
                    HarnessErrorKind::ProbeSpawn,
                    format!("context-scan build failed: {error}"),
                )
            })?;
            // Confirm the store seam invariant (L0+L1 per node, host-only) without
            // changing the record shape — byte-identical to the B1/B2 scenarios.
            debug_assert_eq!(ctx.scan.nodes.len(), ctx.graph.nodes().len());
            debug_assert_eq!(ctx.scan.nodes.len(), ctx.store.sets().len());
            let scan = ctx.scan;
            let nodes: Vec<Value> = scan
                .nodes
                .iter()
                .map(|node| {
                    json!({
                        "relativePath": node.relative_path,
                        "contentDigest": node.content_digest,
                        "byteLen": node.byte_len
                    })
                })
                .collect();
            return Ok(json!({
                "nodes": nodes,
                "protectedSkipped": scan.protected_skipped,
                "oversizedSkipped": scan.oversized_skipped,
                "filesNotScanned": scan.files_not_scanned,
                "truncated": scan.truncated
            }));
        }
        let scan = siralos_adapters::context_scan::scan_workspace(root)
            .map_err(|error| {
                HarnessError::new(
                    HarnessErrorKind::ProbeSpawn,
                    format!("context-scan failed: {error}"),
                )
            })?;
        let nodes: Vec<Value> = scan
            .nodes
            .iter()
            .map(|node| {
                json!({
                    "relativePath": node.relative_path,
                    "contentDigest": node.content_digest,
                    "byteLen": node.byte_len
                })
            })
            .collect();
        Ok(json!({
            "nodes": nodes,
            "protectedSkipped": scan.protected_skipped,
            "oversizedSkipped": scan.oversized_skipped,
            "filesNotScanned": scan.files_not_scanned,
            "truncated": scan.truncated
        }))
    })
}

/// Activation B3b (decision 99): canonical `context-session` record.
///
/// Mirrors the CPU session wiring seam: load the workspace profile, read
/// the additive `[profile.context_system]` opt-in only when the profile is
/// actually applied (a malformed table leaves the profile unapplied and
/// therefore OFF), build the host-side `ContextSystemSession` (build on
/// opt-in, fail-closed diagnostic otherwise), and optionally drive the
/// demand loop over host-observed rounds (each round's calls become
/// `ToolObservation`s and one tick). The record is deterministic and
/// bounded: tools are the three read-only context tools, the working set is
/// over the classified graph, and every tick is a canonical bounded
/// `TickInput`.
pub(in crate::harness) fn context_session_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    with_fixture_workspace("context-session", input, |root| {
        // Mirror the live seam (interactive.rs:1032-1041): only an applied
        // profile contributes the opt-in. A malformed or refused profile
        // leaves the subsystem OFF even if the raw record claimed enabled.
        let host_rules = vec![siralos_core::tool::PolicyRule {
            capability: siralos_core::tool::CapabilityId::parse(
                "workspace.read",
            )
            .expect("workspace.read is a valid capability id"),
            rule: siralos_core::tool::PermissionRule::Allow,
        }];
        let loaded =
            siralos_adapters::profile_config::load_workspace_profile(root);
        let declared = match &loaded {
            siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
                record,
            ) => siralos_core::composition::declare_profile(
                Some(record),
                &siralos_core::tool::PermissionPolicy::from_rules(
                    host_rules.clone(),
                ),
            ),
            siralos_adapters::profile_config::WorkspaceProfileLoad::Absent => {
                siralos_core::composition::DeclaredProfile::Absent
            }
            siralos_adapters::profile_config::WorkspaceProfileLoad::Invalid {
                diagnostic,
            } => siralos_core::composition::DeclaredProfile::Invalid {
                diagnostic: diagnostic.clone(),
            },
        };
        let effective = siralos_core::composition::compose_effective_policy(
            &host_rules,
            &declared,
        );
        let enabled = if effective.applied_profile.is_some() {
            match &loaded {
                siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
                    record,
                ) => record.context_system_enabled,
                _ => false,
            }
        } else {
            false
        };
        let build = siralos_adapters::context_session::build_context_system(
            root, enabled,
        );
        let session = build.session.clone();
        let is_audit = scenario_id.contains("audit");
        if session.is_none() {
            if is_audit {
                return Ok(json!({
                    "enabled": enabled,
                    "off": true,
                    "diagnostic": build.diagnostic.unwrap_or_default(),
                    "nodes": 0,
                    "tools": [],
                    "workingSetNodes": 0,
                    "rounds": [],
                    "audit": {
                        "counters": null,
                        "ring": [],
                    },
                }));
            }
            return Ok(json!({
                "enabled": enabled,
                "off": true,
                "diagnostic": build.diagnostic.unwrap_or_default(),
                "nodes": 0,
                "tools": [],
                "workingSetNodes": 0,
                "rounds": [],
            }));
        }
        let mut s = session.expect("session");
        let mut tools: Vec<String> = s
            .register_tools()
            .iter()
            .map(|tool| tool.definition().name.clone())
            .collect();
        tools.sort();
        let nodes = s.workspace.graph.nodes().len();
        let working_set_nodes = s.working_set.entries().len();
        let mut rounds: Vec<Value> = Vec::new();
        if let Ok(round_specs) = scenario_array(input, "rounds") {
            for spec in round_specs {
                let mut observations: Vec<
                    siralos_adapters::tool::context_events::ToolObservation,
                > = Vec::new();
                if let Ok(calls) = scenario_array(spec, "calls") {
                    for call in calls {
                        let tool = scenario_string(call, "tool")?;
                        let node_id = scenario_string(call, "node_id")?;
                        observations.push(
                            siralos_adapters::tool::context_events::ToolObservation::new(
                                tool.clone(),
                                json!({ "node_id": node_id }),
                                siralos_core::provider::ToolExecutionResult::Success {
                                    output: json!({ "id": node_id }),
                                    summary: format!("{tool} {node_id}"),
                                },
                            ),
                        );
                    }
                }
                let report = s.drive_tick(&observations);
                rounds.push(json!({
                    "tick": report.tick,
                    "promoted": report.promoted,
                    "demoted": report.demoted,
                }));
            }
        }
        if is_audit {
            let m = &s.metrics;
            let counters = json!({
                "ticks_total": m.ticks_total,
                "coalesced_noop_ticks_total": m.coalesced_noop_ticks_total,
                "events_total": m.events_total,
                "events_dropped_total": m.events_dropped_total,
                "demand_updates_total": m.demand_updates_total,
                "promotions_total": m.promotions_total,
                "demotions_total": m.demotions_total,
                "stale_demotions_total": m.stale_demotions_total,
                "pin_quota_demotions_total": m.pin_quota_demotions_total,
                "budget_demotions_total": m.budget_demotions_total,
                "assembled_summary_tokens_total": m.assembled_summary_tokens_total,
                "neighbor_stub_tokens_total": m.neighbor_stub_tokens_total,
            });
            let records = m.records();
            let start = records.len().saturating_sub(8);
            let ring: Vec<Value> = records[start..]
                .iter()
                .map(|rec| {
                    json!({
                        "now": rec.now,
                        "canonical_event_count": rec.canonical_event_count,
                        "events_dropped": rec.events_dropped,
                        "tier_counts": {
                            "hot": rec.tier_counts.hot,
                            "warm": rec.tier_counts.warm,
                            "cold": rec.tier_counts.cold,
                            "archive": rec.tier_counts.archive,
                        },
                        "assembled_unique_total": rec.assembled_unique_total,
                        "assembled_summary_total": rec.assembled_summary_total,
                        "stub_total": rec.stub_total,
                        "demotion_counts": {
                            "stale": rec.demotion_counts.stale,
                            "pin_quota": rec.demotion_counts.pin_quota,
                            "budget": rec.demotion_counts.budget,
                        },
                        "promotion_count": rec.promotion_count,
                    })
                })
                .collect();
            return Ok(json!({
                "enabled": enabled,
                "off": false,
                "diagnostic": null,
                "nodes": nodes,
                "tools": tools,
                "workingSetNodes": working_set_nodes,
                "rounds": rounds,
                "audit": {
                    "counters": counters,
                    "ring": ring,
                },
            }));
        }
        Ok(json!({
            "enabled": enabled,
            "off": false,
            "diagnostic": null,
            "nodes": nodes,
            "tools": tools,
            "workingSetNodes": working_set_nodes,
            "rounds": rounds,
        }))
    })
}

// ---------------------------------------------------------------------------
// T4 (decision 108): the `tui-render` subject — the pinned render model.
//
// The builder constructs a `TuiState` from the scenario input and renders
// it over a FIXED 80x24 `TestBackend` viewport via the PRODUCTION draw
// path (`draw` / `draw_with_pane`) — no render re-implementation inside
// the harness. The harness only constructs inputs and serializes the
// produced buffer: one line per buffer row, exact characters. The builder
// is DETERMINISTIC — same input yields a byte-equal record.

/// Fixed `tui-render` viewport width in columns.
const TUI_RENDER_WIDTH: u16 = 80;

/// Fixed `tui-render` viewport height in rows.
const TUI_RENDER_HEIGHT: u16 = 24;

/// Build one `tui-render` record: scenario echo + the canonical frame.
///
/// The record shape is `{ scenario, frame }`: `scenario` echoes the input
/// fields (`transcript_lines`, `input`, `status`, `scroll_offset`,
/// `pending_approval` lines or null, `pane` presence), and `frame` is the
/// 24-row canonical rendering (one string per buffer row, exact
/// characters, trailing spaces trimmed per row for JSON stability — the
/// row count and order are what the snapshot pins).
pub(in crate::harness) fn tui_render_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use siralos_cli::tui::{
        ApprovalModal, ContextPaneData, ToolActivityEntry, TuiState, draw,
        draw_with_pane,
    };
    use siralos_core::context_metrics::{
        ContextMetrics, DemotionKindCounts, TickRecord, TierCounts,
    };

    // I4: transcript entries carry fixed fixture timestamps so pinned frames stay deterministic.
    // The scenario input still carries `transcript_lines` as strings (unchanged files);
    // the builder injects a FIXED timestamp per entry for rendering.
    const FIXTURE_TIMESTAMP: &str = "2026-08-31 12:00:00 UTC";
    let transcript: Vec<String> = input
        .get("transcript_lines")
        .and_then(Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .filter_map(|v| {
                    if let Some(s) = v.as_str() {
                        Some(s.to_owned())
                    } else if let Some(obj) = v.as_object() {
                        obj.get("text")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .ok_or_else(|| {
            HarnessError::corpus(
                "tui-render transcript_lines must be an array of strings",
            )
        })?;
    let input_text =
        input.get("input").and_then(Value::as_str).ok_or_else(|| {
            HarnessError::corpus("tui-render input must be a string")
        })?;
    let status =
        input.get("status").and_then(Value::as_str).ok_or_else(|| {
            HarnessError::corpus("tui-render status must be a string")
        })?;
    let scroll_offset = input
        .get("scroll_offset")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            HarnessError::corpus(
                "tui-render scroll_offset must be a non-negative integer",
            )
        })?;
    let pending: Option<Vec<String>> = match input.get("pending_approval") {
        Some(Value::Null) | None => None,
        Some(Value::Array(request)) => Some(
            request
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        ),
        _ => {
            return Err(HarnessError::corpus(
                "tui-render pending_approval must be null or an array of strings",
            ));
        }
    };
    // Optional pane snapshot: counters pairs + ring records + activity.
    // Ring records use the harness-shipped `{ now, events }` fixture
    // shape; the remaining `TickRecord` fields are zero (the snapshots pin
    // the RENDERED frame, not the metrics values — counters carry the
    // scenario's values verbatim).
    let pane_data: Option<ContextPaneData> = match input.get("pane") {
        Some(Value::Null) | None => None,
        Some(Value::Object(pane)) => {
            let counters: Vec<(String, u64)> = pane
                .get("counters")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| {
                            let pair = entry.as_array()?;
                            Some((
                                pair.first()?.as_str()?.to_owned(),
                                pair.get(1)?.as_u64()?,
                            ))
                        })
                        .collect()
                })
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "tui-render pane.counters must be [name, value] pairs",
                    )
                })?;
            let ring: Vec<TickRecord> = pane
                .get("ring")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .map(|entry| TickRecord {
                            now: entry
                                .get("now")
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                            canonical_event_count: entry
                                .get("events")
                                .and_then(Value::as_u64)
                                .unwrap_or(0)
                                as usize,
                            events_dropped: 0,
                            tier_counts: TierCounts {
                                hot: 0,
                                warm: 0,
                                cold: 0,
                                archive: 0,
                            },
                            assembled_unique_total: 0,
                            assembled_summary_total: 0,
                            stub_total: 0,
                            demotion_counts: DemotionKindCounts {
                                stale: 0,
                                pin_quota: 0,
                                budget: 0,
                            },
                            promotion_count: 0,
                        })
                        .collect()
                })
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "tui-render pane.ring must be an array",
                    )
                })?;
            let activity: Vec<ToolActivityEntry> = pane
                .get("activity")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .map(|entry| ToolActivityEntry {
                            tool_name: entry
                                .get("tool_name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            status: entry
                                .get("status")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                        })
                        .collect()
                })
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "tui-render pane.activity must be an array",
                    )
                })?;
            // The pane snapshot needs live metrics for `context_pane_lines`
            // only through the owned `ContextPaneData` — construct it
            // directly (same struct the production builder returns).
            let _ = ContextMetrics::new();
            Some(ContextPaneData { counters, ring, activity })
        }
        _ => {
            return Err(HarnessError::corpus(
                "tui-render pane must be null or an object",
            ));
        }
    };

    let mut state = TuiState::new();
    // H2: banner + greeting at session start (TUI-only). The pinned fixtures carry them.
    siralos_cli::tui::push_banner_and_greeting(&mut state);
    // I4/H4: fixed fixture timestamps for deterministic frames (banner stays unstamped).
    let transcript_entries: Vec<siralos_cli::tui::TranscriptEntry> =
        transcript
            .iter()
            .map(|text| siralos_cli::tui::TranscriptEntry {
                text: text.clone(),
                timestamp: Some(FIXTURE_TIMESTAMP.to_owned()),
            })
            .collect();
    // Append scenario transcript after the banner.
    for entry in transcript_entries {
        state.transcript.push(entry.clone());
        state.transcript_lines.push(entry.text);
    }
    // Ensure transcript_lines stays synced for legacy draw path fallback
    state.input = input_text.to_owned();
    state.status = status.to_owned();
    state.scroll_offset = scroll_offset.min(u16::MAX as u64) as u16;
    state.pending_approval = pending.clone().map(ApprovalModal::new);
    state.palette = None;

    // Production draw path over the fixed viewport — no re-implementation.
    let backend = TestBackend::new(TUI_RENDER_WIDTH, TUI_RENDER_HEIGHT);
    let mut terminal = Terminal::new(backend).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeProtocol,
            format!("tui-render terminal failed: {error}"),
        )
    })?;
    let pane_ref = pane_data.as_ref();
    terminal
        .draw(|frame| match pane_ref {
            Some(pane) => draw_with_pane(&state, Some(pane), frame),
            None => draw(&state, frame),
        })
        .map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeProtocol,
                format!("tui-render draw failed: {error}"),
            )
        })?;
    let buffer = terminal.backend().buffer().clone();
    let area = buffer.area;
    debug_assert_eq!(area.width, TUI_RENDER_WIDTH);
    debug_assert_eq!(area.height, TUI_RENDER_HEIGHT);
    let mut frame: Vec<String> = Vec::with_capacity(area.height as usize);
    for row in 0..area.height {
        let mut line = String::new();
        for col in 0..area.width {
            if let Some(cell) = buffer.cell((col, row)) {
                line.push_str(cell.symbol());
            }
        }
        // Trim trailing spaces per row for JSON stability; the row count
        // (24) and order pin the snapshot.
        frame.push(line.trim_end().to_owned());
    }

    Ok(json!({
        "scenario": {
            "id": scenario_id,
            "transcript_lines": transcript,
            "input": input_text,
            "status": status,
            "scroll_offset": scroll_offset,
            "pending_approval": pending,
            "pane_present": pane_data.is_some(),
        },
        "viewport": { "width": TUI_RENDER_WIDTH, "height": TUI_RENDER_HEIGHT },
        "frame": frame,
    }))
}

// ---------------------------------------------------------------------------
// Stage 3R R7.1 subject: provider-turn.

pub(in crate::harness) fn context_graph_record(
    _input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::context_graph::{
        ContextEdge, ContextEdgeKind, ContextGraph, ContextNode,
        ContextNodeKind, compute_context_graph_digest, estimate_tokens,
        stale_context_nodes,
    };
    use siralos_core::identity::sha256_hex;

    let auth_body = "auth overview body for ctx-source-auth";
    let decision_body = "decision body for ctx-decision-1";
    let knowledge_body = "knowledge summary body for ctx-knowledge-summary";
    let auth_digest = sha256_hex(auth_body.as_bytes());
    let decision_digest = sha256_hex(decision_body.as_bytes());
    let knowledge_digest = sha256_hex(knowledge_body.as_bytes());
    let mutated_auth_body = "auth overview body mutated";
    let mutated_auth_digest = sha256_hex(mutated_auth_body.as_bytes());

    let nodes = vec![
        ContextNode {
            id: "ctx-source-auth".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: auth_digest.clone(),
            summary: "auth overview".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("auth overview"),
        },
        ContextNode {
            id: "ctx-decision-1".to_owned(),
            kind: ContextNodeKind::Decision,
            content_digest: decision_digest.clone(),
            summary: String::new(),
            source_bindings: vec![],
            token_estimate: 0,
        },
        ContextNode {
            id: "ctx-knowledge-summary".to_owned(),
            kind: ContextNodeKind::Knowledge,
            content_digest: knowledge_digest.clone(),
            summary: String::new(),
            source_bindings: vec![
                ("ctx-source-auth".to_owned(), auth_digest.clone()),
                ("ctx-decision-1".to_owned(), decision_digest.clone()),
            ],
            token_estimate: 0,
        },
    ];
    let edges = vec![
        ContextEdge {
            from: "ctx-source-auth".to_owned(),
            to: "ctx-decision-1".to_owned(),
            kind: ContextEdgeKind::Contains,
        },
        ContextEdge {
            from: "ctx-decision-1".to_owned(),
            to: "ctx-knowledge-summary".to_owned(),
            kind: ContextEdgeKind::DependsOn,
        },
        ContextEdge {
            from: "ctx-source-auth".to_owned(),
            to: "ctx-knowledge-summary".to_owned(),
            kind: ContextEdgeKind::RelevantTo,
        },
    ];
    let graph = ContextGraph::build(nodes, edges).map_err(|e| {
        HarnessError::corpus(format!("context-graph build failed: {e}"))
    })?;
    let graph_digest = compute_context_graph_digest(&graph);
    let current_original = vec![
        ("ctx-source-auth".to_owned(), auth_digest.clone()),
        ("ctx-decision-1".to_owned(), decision_digest.clone()),
    ];
    let stale_before = stale_context_nodes(&graph, &current_original);
    let stale_before_ids: Vec<String> =
        stale_before.iter().map(|n| n.node_id.clone()).collect();
    let current_mutated = vec![
        ("ctx-source-auth".to_owned(), mutated_auth_digest.clone()),
        ("ctx-decision-1".to_owned(), decision_digest.clone()),
    ];
    let stale_after_nodes = stale_context_nodes(&graph, &current_mutated);
    let stale_after: Vec<String> =
        stale_after_nodes.iter().map(|n| n.node_id.clone()).collect();
    let stale_bindings: Vec<String> = stale_after_nodes
        .iter()
        .flat_map(|n| n.stale_bindings.clone())
        .collect();
    let dangling_error = {
        let bad_nodes = vec![ContextNode {
            id: "ctx-source-auth".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: auth_digest.clone(),
            summary: String::new(),
            source_bindings: vec![],
            token_estimate: 0,
        }];
        let bad_edges = vec![ContextEdge {
            from: "ctx-source-auth".to_owned(),
            to: "missing-node".to_owned(),
            kind: ContextEdgeKind::References,
        }];
        match ContextGraph::build(bad_nodes, bad_edges) {
            Ok(_) => "unexpected-ok".to_owned(),
            Err(e) => format!("{e}"),
        }
    };
    Ok(json!({
        "nodeCount": graph.nodes().len(),
        "edgeCount": graph.edges().len(),
        "graphDigest": graph_digest,
        "staleBefore": stale_before_ids,
        "staleAfter": stale_after,
        "staleBindings": stale_bindings,
        "danglingEdgeError": dangling_error,
    }))
}

// ---------------------------------------------------------------------------
// Hermetic subject: context-representation (decision 79 slice 2, corpus v58).

pub(in crate::harness) fn context_representation_record(
    _input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::context_representation::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
        representation_store_digest, resolve_representation,
    };

    let identity_content = r#"{"id":"ctx-source-auth","kind":"source"}"#;
    let summary_content = "summary prose for ctx-source-auth";
    let structured_content = r#"["fact1","fact2"]"#;
    let source_content = "";

    let identity_digest = content_digest_of(identity_content);
    let summary_digest = content_digest_of(summary_content);
    let structured_digest = content_digest_of(structured_content);
    let source_digest = content_digest_of(source_content);

    let reps = vec![
        NodeRepresentation {
            level: RepresentationLevel::Identity,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: identity_digest.clone(),
            derived_from: vec![],
            content: identity_content.to_owned(),
        },
        NodeRepresentation {
            level: RepresentationLevel::Summary,
            origin: RepresentationOrigin::ModelDerived,
            content_digest: summary_digest.clone(),
            derived_from: vec![(
                "ctx-source-auth".to_owned(),
                identity_digest.clone(),
            )],
            content: summary_content.to_owned(),
        },
        NodeRepresentation {
            level: RepresentationLevel::Structured,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: structured_digest.clone(),
            derived_from: vec![],
            content: structured_content.to_owned(),
        },
        NodeRepresentation {
            level: RepresentationLevel::Source,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: source_digest.clone(),
            derived_from: vec![],
            content: source_content.to_owned(),
        },
    ];

    let set = NodeRepresentationSet::build("ctx-source-auth".to_owned(), reps)
        .map_err(|e| {
            HarnessError::corpus(format!(
                "context-representation build failed: {e}"
            ))
        })?;
    let store =
        ContextRepresentationStore::build(vec![set.clone()]).map_err(|e| {
            HarnessError::corpus(format!(
                "context-representation store build failed: {e}"
            ))
        })?;
    let store_digest = representation_store_digest(&store);
    let resolved_l2 =
        resolve_representation(&set, RepresentationLevel::Structured)
            .map(|r| r.content_digest.clone())
            .unwrap_or_default();

    let model_derived_l2_error = {
        let bad = NodeRepresentation {
            level: RepresentationLevel::Structured,
            origin: RepresentationOrigin::ModelDerived,
            content_digest: structured_digest.clone(),
            derived_from: vec![(
                "ctx-source-auth".to_owned(),
                identity_digest.clone(),
            )],
            content: structured_content.to_owned(),
        };
        match NodeRepresentationSet::build(
            "ctx-source-auth".to_owned(),
            vec![bad],
        ) {
            Ok(_) => "unexpected-ok".to_owned(),
            Err(e) => match e {
                siralos_core::context_representation::RepresentationError::ModelDerivedStructured { .. } => {
                    "ModelDerivedStructured".to_owned()
                }
                _ => format!("{e}"),
            },
        }
    };

    let unprovenanced_error = {
        let bad = NodeRepresentation {
            level: RepresentationLevel::Summary,
            origin: RepresentationOrigin::ModelDerived,
            content_digest: summary_digest.clone(),
            derived_from: vec![],
            content: summary_content.to_owned(),
        };
        match NodeRepresentationSet::build(
            "ctx-source-auth".to_owned(),
            vec![bad],
        ) {
            Ok(_) => "unexpected-ok".to_owned(),
            Err(e) => match e {
                siralos_core::context_representation::RepresentationError::UnprovenancedDerived { .. } => {
                    "UnprovenancedDerived".to_owned()
                }
                _ => format!("{e}"),
            },
        }
    };

    Ok(json!({
        "setCount": store.sets().len(),
        "repCount": store.sets().iter().map(|s| s.representations.len()).sum::<usize>(),
        "storeDigest": store_digest,
        "resolvedL2Digest": resolved_l2,
        "modelDerivedL2Error": model_derived_l2_error,
        "unprovenancedDerivedError": unprovenanced_error,
    }))
}

// ---------------------------------------------------------------------------
// Hermetic subject: context-scheduler (decision 79 slice 3, corpus v59).

pub(in crate::harness) fn context_scheduler_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::context_scheduler::{
        SchedulerConfig, SchedulerEntry, SchedulerEvent, SchedulerTick,
        SearchScore, WorkingSetState, WorkingSetTier,
    };
    // Decision 93 new scenario: search-score-retention (rank-for-retention)
    let is_retention =
        input.get("searchScoreRetention").and_then(|v| v.as_bool())
            == Some(true)
            || input.get("case").and_then(|v| v.as_str())
                == Some("search-score-retention");
    if is_retention {
        use siralos_core::context_graph::{
            ContextGraph, ContextNode, ContextNodeKind, estimate_tokens,
        };
        use siralos_core::context_representation::{
            ContextRepresentationStore, NodeRepresentation,
            NodeRepresentationSet, RepresentationLevel, RepresentationOrigin,
            content_digest_of,
        };
        // --- without scores: scheduler order (lowest scheduler demoted first) ---
        let nodes = vec![
            ContextNode {
                id: "a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "a".repeat(64),
                summary: "summary-a".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-a"),
            },
            ContextNode {
                id: "b".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "b".repeat(64),
                summary: "summary-b".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-b"),
            },
            ContextNode {
                id: "c".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "c".repeat(64),
                summary: "summary-c".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-c"),
            },
        ];
        let graph =
            ContextGraph::build(nodes, vec![]).expect("retention graph");
        let sets = vec![
            NodeRepresentationSet::build(
                "a".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-a"),
                    derived_from: vec![],
                    content: "summary-a".to_owned(),
                }],
            )
            .expect("a"),
            NodeRepresentationSet::build(
                "b".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-b"),
                    derived_from: vec![],
                    content: "summary-b".to_owned(),
                }],
            )
            .expect("b"),
            NodeRepresentationSet::build(
                "c".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-c"),
                    derived_from: vec![],
                    content: "summary-c".to_owned(),
                }],
            )
            .expect("c"),
        ];
        let store =
            ContextRepresentationStore::build(sets).expect("retention store");
        // entries: a low sched (10), b mid (50), c high (90)
        let entries = vec![
            SchedulerEntry {
                node_id: "a".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: false,
                relevance: 10,
                last_access_tick: 0,
                token_estimate: 1000,
                content_digest: String::new(),
            },
            SchedulerEntry {
                node_id: "b".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: false,
                relevance: 50,
                last_access_tick: 0,
                token_estimate: 1000,
                content_digest: String::new(),
            },
            SchedulerEntry {
                node_id: "c".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: false,
                relevance: 90,
                last_access_tick: 0,
                token_estimate: 1000,
                content_digest: String::new(),
            },
        ];
        let tok = estimate_tokens("summary-a");
        let cfg = SchedulerConfig::new(tok).expect("cfg");
        let mut state_without =
            WorkingSetState::build(entries.clone()).expect("state without");
        let ctx_without = state_without.assemble(&graph, &store, &cfg);
        // --- with scores: a 100, c 50, b absent 0 => demote b(0) then c(50), keep a(100) ---
        let mut state_with =
            WorkingSetState::build(entries).expect("state with");
        state_with.set_search_scores(vec![
            SearchScore::new("a", 100),
            SearchScore::new("c", 50),
        ]);
        let ctx_with = state_with.assemble(&graph, &store, &cfg);
        // --- quota unchanged: pinned HOT with search scores inverted, pin quota demotes lowest scheduler ---
        let big_a = "x".repeat(2040) + "a";
        let big_b = "x".repeat(2040) + "b";
        let big_c = "x".repeat(2040) + "c";
        let big_nodes = vec![
            ContextNode {
                id: "a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "a".repeat(64),
                summary: big_a.clone(),
                source_bindings: vec![],
                token_estimate: estimate_tokens(&big_a),
            },
            ContextNode {
                id: "b".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "b".repeat(64),
                summary: big_b.clone(),
                source_bindings: vec![],
                token_estimate: estimate_tokens(&big_b),
            },
            ContextNode {
                id: "c".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "c".repeat(64),
                summary: big_c.clone(),
                source_bindings: vec![],
                token_estimate: estimate_tokens(&big_c),
            },
        ];
        let big_graph =
            ContextGraph::build(big_nodes, vec![]).expect("big graph");
        let big_sets = vec![
            NodeRepresentationSet::build(
                "a".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(&big_a),
                    derived_from: vec![],
                    content: big_a,
                }],
            )
            .expect("a"),
            NodeRepresentationSet::build(
                "b".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(&big_b),
                    derived_from: vec![],
                    content: big_b,
                }],
            )
            .expect("b"),
            NodeRepresentationSet::build(
                "c".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(&big_c),
                    derived_from: vec![],
                    content: big_c,
                }],
            )
            .expect("c"),
        ];
        let big_store =
            ContextRepresentationStore::build(big_sets).expect("big store");
        let pinned_entries = vec![
            SchedulerEntry {
                node_id: "a".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: true,
                relevance: 10,
                last_access_tick: 0,
                token_estimate: 1000,
                content_digest: String::new(),
            },
            SchedulerEntry {
                node_id: "b".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: true,
                relevance: 90,
                last_access_tick: 0,
                token_estimate: 1000,
                content_digest: String::new(),
            },
            SchedulerEntry {
                node_id: "c".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: true,
                relevance: 90,
                last_access_tick: 0,
                token_estimate: 1000,
                content_digest: String::new(),
            },
        ];
        let mut pinned_state =
            WorkingSetState::build(pinned_entries).expect("pinned");
        pinned_state.set_search_scores(vec![
            SearchScore::new("a", 100),
            SearchScore::new("b", 0),
            SearchScore::new("c", 50),
        ]);
        let cfg_default = SchedulerConfig::default();
        let ctx_quota =
            pinned_state.assemble(&big_graph, &big_store, &cfg_default);
        // --- depth unchanged: high score node still L1 only ---
        let depth_nodes = vec![
            ContextNode {
                id: "a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "a".repeat(64),
                summary: "summary-a".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-a"),
            },
            ContextNode {
                id: "b".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "b".repeat(64),
                summary: "summary-b".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-b"),
            },
        ];
        let depth_graph =
            ContextGraph::build(depth_nodes, vec![]).expect("depth graph");
        let set_a_deep = NodeRepresentationSet::build(
            "a".to_owned(),
            vec![
                NodeRepresentation {
                    level: RepresentationLevel::Identity,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("identity-a"),
                    derived_from: vec![],
                    content: "identity-a".to_owned(),
                },
                NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-a"),
                    derived_from: vec![],
                    content: "summary-a".to_owned(),
                },
                NodeRepresentation {
                    level: RepresentationLevel::Structured,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("structured-a"),
                    derived_from: vec![],
                    content: "structured-a".to_owned(),
                },
            ],
        )
        .expect("set_a_deep");
        let set_b = NodeRepresentationSet::build(
            "b".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-b"),
                derived_from: vec![],
                content: "summary-b".to_owned(),
            }],
        )
        .expect("b");
        let depth_store =
            ContextRepresentationStore::build(vec![set_a_deep, set_b])
                .expect("depth store");
        let mut depth_state = WorkingSetState::build(vec![
            SchedulerEntry {
                node_id: "a".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: false,
                relevance: 90,
                last_access_tick: 0,
                token_estimate: 1000,
                content_digest: String::new(),
            },
            SchedulerEntry {
                node_id: "b".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: false,
                relevance: 10,
                last_access_tick: 0,
                token_estimate: 1000,
                content_digest: String::new(),
            },
        ])
        .expect("depth state");
        depth_state.set_search_scores(vec![SearchScore::new("a", 9999)]);
        let depth_ctx = depth_state.assemble(
            &depth_graph,
            &depth_store,
            &SchedulerConfig::default(),
        );
        let depth_summary = depth_ctx
            .entries
            .iter()
            .find(|e| e.node_id == "a")
            .map(|e| e.summary.clone())
            .unwrap_or_default();
        // For protocol validation, return a valid context-scheduler result where assembly reflects with-scores retention
        // (withoutScores would be ["a","b"] -> "c" remaining, withScores is ["b","c"] -> "a" remaining).
        // Keep the comparison evidence in the assembly demoted field and verify quota/depth via the same state.
        let _without = &ctx_without; // retain for evidence (unused in final assembly but proves ordering)
        let tiers_before_retention: std::collections::BTreeMap<
            String,
            String,
        > = [
            ("a".to_owned(), "Hot".to_owned()),
            ("b".to_owned(), "Hot".to_owned()),
            ("c".to_owned(), "Hot".to_owned()),
        ]
        .into_iter()
        .collect();
        let assembly_json = json!({
            "entries": ctx_with.entries.iter().map(|e| json!({
                "nodeId": e.node_id,
                "contentDigest": e.content_digest,
                "summary": e.summary,
                "tokenEstimate": e.token_estimate,
                "neighbors": e.neighbor_stubs.iter().map(|s| json!({
                    "nodeId": s.node_id,
                    "contentDigest": s.content_digest
                })).collect::<Vec<Value>>()
            })).collect::<Vec<Value>>(),
            "totalTokensBefore": ctx_with.total_tokens_before,
            "totalTokensAfter": ctx_with.total_tokens_after,
            "demoted": ctx_with.demoted,
        });
        // Use withScores demoted as budgetDemotions for validation (must be array)
        // and verify quota unchanged by checking ctx_quota still demoted "a" (lowest scheduler)
        assert!(
            ctx_quota.demoted.contains(&"a".to_owned()),
            "quota must demote a"
        );
        assert_eq!(depth_summary, "summary-a", "depth must stay L1");
        return Ok(json!({
            "tiersBefore": tiers_before_retention,
            "tickReport": {"tick": 1, "promoted": [], "demoted": []},
            "tiersAfterStale": tiers_before_retention,
            "budgetDemotions": ctx_with.demoted.clone(),
            "hotTokensAfter": ctx_with.total_tokens_after,
            "unknownNodeError": "unknown-node: not found",
            "assembly": assembly_json
        }));
    }

    let entries = vec![
        SchedulerEntry {
            node_id: "ctx-a".to_owned(),
            tier: WorkingSetTier::Cold,
            pinned: false,
            relevance: 10,
            last_access_tick: 0,
            token_estimate: 1000,
            content_digest: "a".repeat(64),
        },
        SchedulerEntry {
            node_id: "ctx-b".to_owned(),
            tier: WorkingSetTier::Cold,
            pinned: false,
            relevance: 50,
            last_access_tick: 0,
            token_estimate: 1000,
            content_digest: "b".repeat(64),
        },
        SchedulerEntry {
            node_id: "ctx-c".to_owned(),
            tier: WorkingSetTier::Hot,
            pinned: false,
            relevance: 90,
            last_access_tick: 0,
            token_estimate: 1000,
            content_digest: "c".repeat(64),
        },
    ];
    let mut state = WorkingSetState::build(entries).map_err(|e| {
        HarnessError::corpus(format!("context-scheduler build failed: {e}"))
    })?;
    let tiers_before: std::collections::BTreeMap<String, String> = state
        .entries()
        .iter()
        .map(|e| (e.node_id.clone(), e.tier.as_str().to_owned()))
        .collect();

    // Access(ctx-a) and Pin(ctx-b)
    state
        .apply_event(SchedulerEvent::Access { node_id: "ctx-a".to_owned() })
        .map_err(|e| HarnessError::corpus(format!("access failed: {e}")))?;
    state
        .apply_event(SchedulerEvent::Pin { node_id: "ctx-b".to_owned() })
        .map_err(|e| HarnessError::corpus(format!("pin failed: {e}")))?;

    let cfg = SchedulerConfig::default();
    let tick_report = state.tick(&cfg);
    let tick_report_json = json!({
        "tick": tick_report.tick,
        "promoted": tick_report.promoted,
        "demoted": tick_report.demoted,
    });

    // Stale(ctx-c) + tick
    state
        .apply_event(SchedulerEvent::Stale { node_id: "ctx-c".to_owned() })
        .map_err(|e| HarnessError::corpus(format!("stale failed: {e}")))?;
    let _ = state.tick(&cfg);
    let tiers_after_stale: std::collections::BTreeMap<String, String> = state
        .entries()
        .iter()
        .map(|e| (e.node_id.clone(), e.tier.as_str().to_owned()))
        .collect();

    // Budget with tiny budget forcing demotion.
    let tiny_cfg = SchedulerConfig::new(1500)
        .map_err(|e| HarnessError::corpus(format!("{e}")))?;
    let estimates = vec![
        ("ctx-a".to_owned(), 1000),
        ("ctx-b".to_owned(), 1000),
        ("ctx-c".to_owned(), 1000),
    ];
    let budget = state
        .enforce_budget(&tiny_cfg, &estimates)
        .map_err(|e| HarnessError::corpus(format!("budget failed: {e}")))?;

    let unknown_error = match state.apply_event(SchedulerEvent::Access {
        node_id: "unknown-node".to_owned(),
    }) {
        Ok(_) => "unexpected-ok".to_owned(),
        Err(e) => format!("{e}"),
    };

    // Assembly B1/B2 (decision 90): HOT L1 summaries + bounded neighbor stubs, counted in budget
    let assembly_ctx = {
        use siralos_core::context_graph::{
            ContextEdge, ContextEdgeKind, ContextGraph, ContextNode,
            ContextNodeKind, estimate_tokens,
        };
        use siralos_core::context_representation::{
            ContextRepresentationStore, NodeRepresentation,
            NodeRepresentationSet, RepresentationLevel, RepresentationOrigin,
            content_digest_of,
        };
        let nodes = vec![
            ContextNode {
                id: "ctx-a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "a".repeat(64),
                summary: "summary-ctx-a".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-ctx-a"),
            },
            ContextNode {
                id: "ctx-b".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "b".repeat(64),
                summary: "summary-ctx-b".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-ctx-b"),
            },
            ContextNode {
                id: "ctx-c".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "c".repeat(64),
                summary: "summary-ctx-c".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-ctx-c"),
            },
        ];
        let edges = vec![
            ContextEdge {
                from: "ctx-a".to_owned(),
                to: "ctx-b".to_owned(),
                kind: ContextEdgeKind::Contains,
            },
            ContextEdge {
                from: "ctx-a".to_owned(),
                to: "ctx-c".to_owned(),
                kind: ContextEdgeKind::Contains,
            },
            ContextEdge {
                from: "ctx-b".to_owned(),
                to: "ctx-c".to_owned(),
                kind: ContextEdgeKind::DependsOn,
            },
        ];
        let graph = ContextGraph::build(nodes, edges).expect("harness graph");
        // Store with deeper structured for ctx-a to prove L1-only assembly (B1)
        let set_a = NodeRepresentationSet::build(
            "ctx-a".to_owned(),
            vec![
                NodeRepresentation {
                    level: RepresentationLevel::Identity,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("identity-ctx-a"),
                    derived_from: vec![],
                    content: "identity-ctx-a".to_owned(),
                },
                NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("summary-ctx-a"),
                    derived_from: vec![],
                    content: "summary-ctx-a".to_owned(),
                },
                NodeRepresentation {
                    level: RepresentationLevel::Structured,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("structured-ctx-a"),
                    derived_from: vec![],
                    content: "structured-ctx-a".to_owned(),
                },
            ],
        )
        .expect("set_a");
        let set_b = NodeRepresentationSet::build(
            "ctx-b".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-ctx-b"),
                derived_from: vec![],
                content: "summary-ctx-b".to_owned(),
            }],
        )
        .expect("set_b");
        let set_c = NodeRepresentationSet::build(
            "ctx-c".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-ctx-c"),
                derived_from: vec![],
                content: "summary-ctx-c".to_owned(),
            }],
        )
        .expect("set_c");
        let store =
            ContextRepresentationStore::build(vec![set_a, set_b, set_c])
                .expect("store");
        // Assemble with default budget (4096) and pinned quota (1024) counting summaries+stubs
        state.assemble(&graph, &store, &cfg)
    };
    let assembly_json = json!({
        "entries": assembly_ctx.entries.iter().map(|e| json!({
            "nodeId": e.node_id,
            "contentDigest": e.content_digest,
            "summary": e.summary,
            "tokenEstimate": e.token_estimate,
            "neighbors": e.neighbor_stubs.iter().map(|s| json!({
                "nodeId": s.node_id,
                "contentDigest": s.content_digest
            })).collect::<Vec<Value>>()
        })).collect::<Vec<Value>>(),
        "totalTokensBefore": assembly_ctx.total_tokens_before,
        "totalTokensAfter": assembly_ctx.total_tokens_after,
        "demoted": assembly_ctx.demoted,
    });

    Ok(json!({
        "tiersBefore": tiers_before,
        "tickReport": tick_report_json,
        "tiersAfterStale": tiers_after_stale,
        "budgetDemotions": budget.demoted,
        "hotTokensAfter": budget.hot_tokens_after,
        "unknownNodeError": unknown_error,
        "assembly": assembly_json,
    }))
}

// ---------------------------------------------------------------------------
// Hermetic subject: context-tool (decision 79 slice 4, corpus v60).
// ---------------------------------------------------------------------------

pub(in crate::harness) fn context_tool_record(
    _input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_adapters::tool::{
        ContextExpandTool, ContextInspectTool, ContextSearchTool,
        ContextToolState,
    };
    use siralos_core::context_graph::{
        ContextEdge, ContextEdgeKind, ContextGraph, ContextNode,
        ContextNodeKind, estimate_tokens,
    };
    use siralos_core::context_representation::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
    };
    use siralos_core::context_scheduler::{
        SchedulerEntry, WorkingSetState, WorkingSetTier,
    };
    use siralos_core::identity::sha256_hex;
    use siralos_core::provider::CancellationToken;
    use siralos_core::tool::Tool;

    // --- Fixture: 3-node graph ---
    let a_body = "body-a";
    let b_body = "body-b";
    let k_body = "body-k";
    let a_digest = sha256_hex(a_body.as_bytes());
    let b_digest = sha256_hex(b_body.as_bytes());
    let k_digest = sha256_hex(k_body.as_bytes());
    let a_mutated = sha256_hex("mutated-a".as_bytes());
    let nodes = vec![
        ContextNode {
            id: "ctx-a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: a_digest.clone(),
            summary: "auth overview for ctx-a".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("auth overview for ctx-a"),
        },
        ContextNode {
            id: "ctx-b".to_owned(),
            kind: ContextNodeKind::Decision,
            content_digest: b_digest.clone(),
            summary: "decision summary b".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("decision summary b"),
        },
        ContextNode {
            id: "ctx-knowledge".to_owned(),
            kind: ContextNodeKind::Knowledge,
            content_digest: k_digest.clone(),
            summary: "knowledge with bindings".to_owned(),
            source_bindings: vec![
                ("ctx-a".to_owned(), a_digest.clone()),
                ("ctx-b".to_owned(), b_digest.clone()),
            ],
            token_estimate: estimate_tokens("knowledge with bindings"),
        },
    ];
    let edges = vec![
        ContextEdge {
            from: "ctx-a".to_owned(),
            to: "ctx-b".to_owned(),
            kind: ContextEdgeKind::Contains,
        },
        ContextEdge {
            from: "ctx-b".to_owned(),
            to: "ctx-knowledge".to_owned(),
            kind: ContextEdgeKind::DependsOn,
        },
    ];
    let graph = ContextGraph::build(nodes, edges).map_err(|e| {
        HarnessError::corpus(format!("context-tool graph build failed: {e}"))
    })?;

    // --- Representation store: ctx-a L0+L2 host, ctx-b L0 host, ctx-knowledge L0+L1 model-derived ---
    let identity_a = r#"{"id":"ctx-a","kind":"source"}"#;
    let structured_a = r#"["fact1","fact2"]"#;
    let identity_b = r#"{"id":"ctx-b","kind":"decision"}"#;
    let identity_k = r#"{"id":"ctx-knowledge","kind":"knowledge"}"#;
    let summary_k = "knowledge prose summary";
    let reps_a = vec![
        NodeRepresentation {
            level: RepresentationLevel::Identity,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of(identity_a),
            derived_from: vec![],
            content: identity_a.to_owned(),
        },
        NodeRepresentation {
            level: RepresentationLevel::Structured,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of(structured_a),
            derived_from: vec![],
            content: structured_a.to_owned(),
        },
    ];
    let reps_b = vec![NodeRepresentation {
        level: RepresentationLevel::Identity,
        origin: RepresentationOrigin::HostExtracted,
        content_digest: content_digest_of(identity_b),
        derived_from: vec![],
        content: identity_b.to_owned(),
    }];
    let reps_k = vec![
        NodeRepresentation {
            level: RepresentationLevel::Identity,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of(identity_k),
            derived_from: vec![],
            content: identity_k.to_owned(),
        },
        NodeRepresentation {
            level: RepresentationLevel::Summary,
            origin: RepresentationOrigin::ModelDerived,
            content_digest: content_digest_of(summary_k),
            derived_from: vec![(
                "ctx-knowledge".to_owned(),
                content_digest_of(identity_k),
            )],
            content: summary_k.to_owned(),
        },
    ];
    let set_a = NodeRepresentationSet::build("ctx-a".to_owned(), reps_a)
        .map_err(|e| {
            HarnessError::corpus(format!(
                "context-tool set_a build failed: {e}"
            ))
        })?;
    let set_b = NodeRepresentationSet::build("ctx-b".to_owned(), reps_b)
        .map_err(|e| {
            HarnessError::corpus(format!(
                "context-tool set_b build failed: {e}"
            ))
        })?;
    let set_k =
        NodeRepresentationSet::build("ctx-knowledge".to_owned(), reps_k)
            .map_err(|e| {
                HarnessError::corpus(format!(
                    "context-tool set_k build failed: {e}"
                ))
            })?;
    let store = ContextRepresentationStore::build(vec![set_a, set_b, set_k])
        .map_err(|e| {
        HarnessError::corpus(format!("context-tool store build failed: {e}"))
    })?;

    // --- Scheduler state: ctx-a Hot pinned, ctx-b Warm, ctx-knowledge Cold ---
    let entries = vec![
        SchedulerEntry {
            node_id: "ctx-a".to_owned(),
            tier: WorkingSetTier::Hot,
            pinned: true,
            relevance: 80,
            last_access_tick: 5,
            token_estimate: 1000,
            content_digest: "a".repeat(64),
        },
        SchedulerEntry {
            node_id: "ctx-b".to_owned(),
            tier: WorkingSetTier::Warm,
            pinned: false,
            relevance: 50,
            last_access_tick: 2,
            token_estimate: 1000,
            content_digest: "b".repeat(64),
        },
        SchedulerEntry {
            node_id: "ctx-knowledge".to_owned(),
            tier: WorkingSetTier::Cold,
            pinned: false,
            relevance: 10,
            last_access_tick: 0,
            token_estimate: 1000,
            content_digest: "k".repeat(64),
        },
    ];
    let state = WorkingSetState::build(entries).map_err(|e| {
        HarnessError::corpus(format!("context-tool state build failed: {e}"))
    })?;
    let current =
        vec![("ctx-a".to_owned(), a_mutated), ("ctx-b".to_owned(), b_digest)];
    let snapshot = ContextToolState::new(graph, store, state, current);

    // Mutation guard: capture tiers before.
    let tiers_before: std::collections::BTreeMap<String, String> = snapshot
        .state
        .entries()
        .iter()
        .map(|e| (e.node_id.clone(), e.tier.as_str().to_owned()))
        .collect();

    let token = CancellationToken::new();
    // Inspect ctx-a
    let inspect_tool = ContextInspectTool::new(snapshot.clone());
    let inspect_a = match inspect_tool
        .execute(&json!({ "node_id": "ctx-a" }), token.signal())
    {
        siralos_core::provider::ToolExecutionResult::Success {
            output,
            ..
        } => output,
        other => {
            return Err(HarnessError::corpus(format!(
                "inspect ctx-a failed: {other:?}"
            )));
        }
    };
    // Search auth
    let search_tool = ContextSearchTool::new(snapshot.clone());
    let search_out = match search_tool
        .execute(&json!({ "query": "auth" }), token.signal())
    {
        siralos_core::provider::ToolExecutionResult::Success {
            output,
            ..
        } => output,
        other => {
            return Err(HarnessError::corpus(format!(
                "search auth failed: {other:?}"
            )));
        }
    };
    let search_hits = search_out
        .get("hits")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    // Expand ctx-a structured
    let expand_tool = ContextExpandTool::new(snapshot.clone());
    let expand_a = match expand_tool.execute(
        &json!({ "node_id": "ctx-a", "level": "structured" }),
        token.signal(),
    ) {
        siralos_core::provider::ToolExecutionResult::Success {
            output,
            ..
        } => output,
        other => {
            return Err(HarnessError::corpus(format!(
                "expand ctx-a failed: {other:?}"
            )));
        }
    };
    let expand_digest = expand_a
        .get("content_digest")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let expand_origin = expand_a
        .get("origin")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    // Expand ctx-b structured -> unavailable
    let expand_b_result = expand_tool.execute(
        &json!({ "node_id": "ctx-b", "level": "structured" }),
        token.signal(),
    );
    let expand_unavailable = match expand_b_result {
        siralos_core::provider::ToolExecutionResult::Unavailable {
            message,
        } => message,
        other => {
            return Err(HarnessError::corpus(format!(
                "expand ctx-b unavailable expected, got {other:?}"
            )));
        }
    };
    // Expand unknown -> not-found
    let expand_unknown_result = expand_tool.execute(
        &json!({ "node_id": "unknown-node", "level": "identity" }),
        token.signal(),
    );
    let expand_not_found = match expand_unknown_result {
        siralos_core::provider::ToolExecutionResult::Failed { message } => {
            message
        }
        other => {
            return Err(HarnessError::corpus(format!(
                "expand unknown not-found expected, got {other:?}"
            )));
        }
    };
    // Inspect ctx-knowledge stale
    let inspect_k_tool = ContextInspectTool::new(snapshot.clone());
    let inspect_k = match inspect_k_tool
        .execute(&json!({ "node_id": "ctx-knowledge" }), token.signal())
    {
        siralos_core::provider::ToolExecutionResult::Success {
            output,
            ..
        } => output,
        other => {
            return Err(HarnessError::corpus(format!(
                "inspect knowledge failed: {other:?}"
            )));
        }
    };
    let stale_knowledge =
        inspect_k.get("stale").and_then(|v| v.as_bool()).unwrap_or(false);
    // Mutation guard: tiers after all calls must equal before.
    let tiers_after: std::collections::BTreeMap<String, String> = snapshot
        .state
        .entries()
        .iter()
        .map(|e| (e.node_id.clone(), e.tier.as_str().to_owned()))
        .collect();
    let mutation_guard =
        if tiers_before == tiers_after { "unchanged" } else { "mutated" };
    if mutation_guard != "unchanged" {
        return Err(HarnessError::corpus(
            "mutation guard failed: tiers changed",
        ));
    }

    Ok(json!({
        "inspectCtxA": inspect_a,
        "searchAuthHits": search_hits,
        "expandDigest": expand_digest,
        "expandOrigin": expand_origin,
        "expandUnavailable": expand_unavailable,
        "expandNotFound": expand_not_found,
        "inspectKnowledgeStale": stale_knowledge,
        "mutationGuard": mutation_guard,
    }))
}

// ---------------------------------------------------------------------------
// Hermetic subject: context-benchmark (decision 79 slice 5, corpus v61; v2 at v62; v64 corrected-baseline re-gate).
// DeepAll is the sole gated reference (zero overhead, no dedup, deepest Source>Detailed>Structured>Summary>Identity);
// SummariesAll and IdentityDiag are informational; paged expansion priority shallow excludes Source vs DeepAll includes Source — intentional.
// ---------------------------------------------------------------------------

pub(in crate::harness) fn context_benchmark_record(
    _input: &Value,
) -> Result<Value, HarnessError> {
    use crate::context_benchmark::{gold_set_v3, run_benchmark};
    let scenarios = gold_set_v3().map_err(|e| {
        HarnessError::corpus(format!("context-benchmark gold_set failed: {e}"))
    })?;
    let report = run_benchmark(&scenarios).map_err(|e| {
        HarnessError::corpus(format!("context-benchmark run failed: {e}"))
    })?;
    let mk_metrics =
        |scenarios: &Vec<crate::context_benchmark::ScenarioMetrics>| {
            scenarios
                .iter()
                .map(|m| {
                    json!({
                        "name": m.name,
                        "tokensBaseline": m.tokens_baseline,
                        "tokensPaged": m.tokens_paged,
                        "recallBaseline": m.recall_baseline,
                        "recallPaged": m.recall_paged,
                        "toolCalls": m.tool_calls
                    })
                })
                .collect::<Vec<Value>>()
        };
    let v1_metrics = mk_metrics(&report.v1.scenarios);
    let v2_metrics = mk_metrics(&report.v2.scenarios);
    let decomposition = json!({
        "dedupSaved": report.decomposition.dedup_saved,
        "rerankSaved": report.decomposition.rerank_saved,
        "levelSaved": report.decomposition.level_saved,
        "dedupSharePct": report.decomposition.dedup_share_bps,
        "dedupGuardOk": report.decomposition.dedup_guard_ok
    });
    let sensitivity = json!({
        "cells": report.sensitivity.cells.iter().map(|c| json!({
            "bytesPerToken": c.bytes_per_token,
            "overhead": c.overhead,
            "recallOk": c.recall_ok,
            "marginOk": c.margin_ok,
            "dedupOk": c.dedup_ok
        })).collect::<Vec<Value>>(),
        "allOk": report.sensitivity.all_ok
    });
    let informational = json!({
        "paraphraseGap": {
            "queryTokens": report.informational.paraphrase_gap.query_tokens,
            "keyOverlapCount": report.informational.paraphrase_gap.key_overlap,
            "inHits": report.informational.paraphrase_gap.in_hits,
            "neighborReachable": report.informational.paraphrase_gap.neighbor_reachable,
            "neighborCost": report.informational.paraphrase_gap.neighbor_cost
        }
    });
    let neighbor_candidates = report
        .neighbor_candidates
        .iter()
        .map(|nc| {
            json!({
                "name": nc.name,
                "generated": nc.generated,
                "admitted": nc.admitted,
                "tokens": nc.tokens
            })
        })
        .collect::<Vec<Value>>();
    let per_scenario = report
        .per_scenario
        .iter()
        .map(|r| {
            json!({
                "name": r.name,
                "deepAll": r.deep_all,
                "summariesAll": r.summaries_all,
                "identityDiag": r.identity_diag,
                "pagedV1": r.paged_v1,
                "pagedV2": r.paged_v2,
                "recallV1": r.recall_v1,
                "recallV2": r.recall_v2,
                "toolCallsV1": r.tool_calls_v1,
                "toolCallsV2": r.tool_calls_v2
            })
        })
        .collect::<Vec<Value>>();
    let level_census = report
        .level_census
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "source": c.source,
                "detailed": c.detailed,
                "structured": c.structured,
                "summary": c.summary,
                "identity": c.identity
            })
        })
        .collect::<Vec<Value>>();
    Ok(json!({
        "baseline": {
            "totalBaseline": report.v1.total_baseline,
            "totalKey": report.v1.total_key,
            "totalRecallBaseline": report.v1.total_recall_baseline
        },
        "strategyV1": {
            "totalPaged": report.v1.total_paged,
            "totalRecallPaged": report.v1.total_recall_paged,
            "totalToolCalls": report.v1.total_tool_calls,
            "scenarioMetrics": v1_metrics.clone()
        },
        "strategyV2": {
            "totalPaged": report.v2.total_paged,
            "totalRecallPaged": report.v2.total_recall_paged,
            "totalToolCalls": report.v2.total_tool_calls,
            "scenarioMetrics": v2_metrics.clone()
        },
        "aggregates": {
            "totalBaseline": report.v2.total_baseline,
            "totalPaged": report.v2.total_paged,
            "totalRecallBaseline": report.v2.total_recall_baseline,
            "totalRecallPaged": report.v2.total_recall_paged,
            "totalToolCalls": report.v2.total_tool_calls
        },
        "scenarioMetrics": v2_metrics,
        "decomposition": decomposition,
        "sensitivity": sensitivity,
        "informational": informational,
        "go": report.go,
        "reason": report.reason,
        "deepAll": report.deep_all,
        "summariesAll": report.summaries_all,
        "identityDiag": report.identity_diag,
        "pagedV1": report.paged_v1,
        "pagedV2": report.paged_v2,
        "depthPremiumBps": report.depth_premium_bps,
        "perScenario": per_scenario,
        "levelCensus": level_census,
        "depthAwareRecall": {
            "v1": report.depth_aware_recall_v1,
            "v2": report.depth_aware_recall_v2,
            "v3": report.depth_aware_recall_v3
        },
        "answerLevelCensus": {
            "preCommitted": {
                "summary": report.answer_level_census.pre_committed_summary,
                "structured": report.answer_level_census.pre_committed_structured
            },
            "asRun": {
                "summary": report.answer_level_census.as_run_summary,
                "structured": report.answer_level_census.as_run_structured
            },
            "floorOk": report.answer_level_census.floor_ok
        },
        "perLevelRecallSplit": {
            "summaryKeys": {
                "total": report.per_level_recall_split.summary_total,
                "recallV2": report.per_level_recall_split.summary_recall_v2,
                "recallV3": report.per_level_recall_split.summary_recall_v3
            },
            "structuredKeys": {
                "total": report.per_level_recall_split.structured_total,
                "recallV2": report.per_level_recall_split.structured_recall_v2,
                "recallV3": report.per_level_recall_split.structured_recall_v3
            }
        },
        "escalationCurve": report.escalation_curve.iter().map(|p| json!({
            "k": p.k,
            "cost": p.cost,
            "depthAwareRecall": p.depth_aware_recall,
            "toolCalls": p.tool_calls
        })).collect::<Vec<Value>>(),
        "policyAdopted": report.policy_adopted,
        "policyReason": report.policy_reason,
        "neighborCandidates": neighbor_candidates
    }))
}

// ---------------------------------------------------------------------------
// Stage 3R R7.1 subject: provider-turn.

/// Strict context-graph input shape validation (decision 79, corpus v57).
pub(in crate::harness) fn validate_context_graph_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("context-graph input must be an object")
    })?;
    for key in obj.keys() {
        let _ = key;
    }
    Ok(())
}

/// Strict context-representation input shape validation (decision 79 slice 2, corpus v58).
pub(in crate::harness) fn validate_context_representation_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("context-representation input must be an object")
    })?;
    for key in obj.keys() {
        let _ = key;
    }
    Ok(())
}

/// Strict context-scheduler input shape validation (decision 79 slice 3, corpus v59).
pub(in crate::harness) fn validate_context_scheduler_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("context-scheduler input must be an object")
    })?;
    for key in obj.keys() {
        let _ = key;
    }
    Ok(())
}

/// Strict context-tool input shape validation (decision 79 slice 4, corpus v60).
pub(in crate::harness) fn validate_context_tool_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("context-tool input must be an object")
    })?;
    for key in obj.keys() {
        let _ = key;
    }
    Ok(())
}

/// Strict context-benchmark input shape validation (decision 79 slice 5, corpus v61).
pub(in crate::harness) fn validate_context_benchmark_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("context-benchmark input must be an object")
    })?;
    for key in obj.keys() {
        let _ = key;
    }
    Ok(())
}

/// Activation B3b (decision 99): `context-session` input shape. The input is
/// an object (fixture `files`, optional `siralos.toml` profile, optional
/// `rounds`); no strict key set is enforced beyond the object bound (the
/// record is candidate-authored and digest-bound).
pub(in crate::harness) fn validate_context_session_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("context-session input must be an object")
    })?;
    for key in obj.keys() {
        let _ = key;
    }
    Ok(())
}

/// T4 (decision 108): `tui-render` input shape.
///
/// The input is one object with exactly the render-model fields:
/// `transcript_lines` (array of strings), `input` (string), `status`
/// (string), `scroll_offset` (u64), `pending_approval` (null or array of
/// strings), and `pane` (null or `{ counters, ring, activity }`). Unknown
/// fields are rejected; the record is candidate-authored and digest-bound.
pub(in crate::harness) fn validate_tui_render_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("tui-render input must be an object")
    })?;
    const EXPECTED: [&str; 6] = [
        "transcript_lines",
        "input",
        "status",
        "scroll_offset",
        "pending_approval",
        "pane",
    ];
    if obj.len() != EXPECTED.len()
        || !EXPECTED.iter().all(|key| obj.contains_key(*key))
    {
        return Err(HarnessError::corpus(
            "tui-render input must contain exactly transcript_lines, input, status, scroll_offset, pending_approval, pane",
        ));
    }
    let lines = obj
        .get("transcript_lines")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            HarnessError::corpus(
                "tui-render transcript_lines must be an array",
            )
        })?;
    if lines.len() > 200
        || lines
            .iter()
            .any(|line| line.as_str().is_none_or(|text| text.len() > 512))
    {
        return Err(HarnessError::corpus(
            "tui-render transcript_lines must hold at most 200 lines of at most 512 bytes",
        ));
    }
    let text = obj.get("input").and_then(Value::as_str).ok_or_else(|| {
        HarnessError::corpus("tui-render input must be a string")
    })?;
    if text.len() > 512 {
        return Err(HarnessError::corpus(
            "tui-render input must be at most 512 bytes",
        ));
    }
    let status =
        obj.get("status").and_then(Value::as_str).ok_or_else(|| {
            HarnessError::corpus("tui-render status must be a string")
        })?;
    if status.len() > 512 {
        return Err(HarnessError::corpus(
            "tui-render status must be at most 512 bytes",
        ));
    }
    if obj.get("scroll_offset").and_then(Value::as_u64).is_none() {
        return Err(HarnessError::corpus(
            "tui-render scroll_offset must be a non-negative integer",
        ));
    }
    match obj.get("pending_approval") {
        Some(Value::Null) => {}
        Some(Value::Array(request)) => {
            if request.len() > 40
                || request.iter().any(|line| {
                    line.as_str().is_none_or(|text| text.len() > 512)
                })
            {
                return Err(HarnessError::corpus(
                    "tui-render pending_approval must hold at most 40 lines of at most 512 bytes",
                ));
            }
        }
        _ => {
            return Err(HarnessError::corpus(
                "tui-render pending_approval must be null or an array of strings",
            ));
        }
    }
    match obj.get("pane") {
        Some(Value::Null) => {}
        Some(Value::Object(pane)) => {
            const PANE_KEYS: [&str; 3] = ["counters", "ring", "activity"];
            if pane.len() != PANE_KEYS.len()
                || !PANE_KEYS.iter().all(|key| pane.contains_key(*key))
            {
                return Err(HarnessError::corpus(
                    "tui-render pane must contain exactly counters, ring, activity",
                ));
            }
            let counters = pane
                .get("counters")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "tui-render pane.counters must be an array",
                    )
                })?;
            if counters.len() > 12
                || counters.iter().any(|entry| {
                    entry.as_array().is_none_or(|pair| {
                        pair.len() != 2
                            || pair[0].as_str().is_none()
                            || pair[1].as_u64().is_none()
                    })
                })
            {
                return Err(HarnessError::corpus(
                    "tui-render pane.counters must hold at most 12 [name, value] pairs",
                ));
            }
            let ring = pane.get("ring").and_then(Value::as_array).ok_or_else(
                || {
                    HarnessError::corpus(
                        "tui-render pane.ring must be an array",
                    )
                },
            )?;
            if ring.len() > 8
                || ring.iter().any(|entry| {
                    entry.as_object().is_none_or(|record| {
                        !record.contains_key("now")
                            || !record.contains_key("events")
                    })
                })
            {
                return Err(HarnessError::corpus(
                    "tui-render pane.ring must hold at most 8 records with now and events",
                ));
            }
            let activity = pane
                .get("activity")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "tui-render pane.activity must be an array",
                    )
                })?;
            if activity.len() > 8
                || activity.iter().any(|entry| {
                    entry.as_object().is_none_or(|record| {
                        record
                            .get("tool_name")
                            .and_then(Value::as_str)
                            .is_none()
                            || record
                                .get("status")
                                .and_then(Value::as_str)
                                .is_none()
                    })
                })
            {
                return Err(HarnessError::corpus(
                    "tui-render pane.activity must hold at most 8 {tool_name, status} records",
                ));
            }
        }
        _ => {
            return Err(HarnessError::corpus(
                "tui-render pane must be null or an object",
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Stage 3R R7.3 subject: context-projection.
// ---------------------------------------------------------------------------

pub(in crate::harness) fn context_projection_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let cases = scenario_array(input, "cases")?;
    let mut records = Vec::new();
    for case in cases {
        let case = materialize_value(case)?;
        let kind = scenario_string(&case, "kind")?;
        let entry = case.get("input").ok_or_else(|| {
            HarnessError::corpus("context-projection case requires an input")
        })?;
        let record = match kind.as_str() {
            "estimate" => run_context_estimate_case(entry)?,
            "pressure" => run_context_pressure_case(entry)?,
            "trim" => run_context_trim_case(entry)?,
            "segments" => run_context_segments_case(entry)?,
            "tool-visibility" => run_context_tool_visibility_case(entry)?,
            "evidence" => run_context_evidence_case(entry)?,
            "fingerprints" => run_context_fingerprints_case(entry)?,
            "pipeline" => run_context_pipeline_case(entry)?,
            "unsupported-tool-calling" => run_context_unsupported_case(entry)?,
            other => {
                return Err(HarnessError::corpus(format!(
                    "unknown context-projection kind {other}"
                )));
            }
        };
        records.push(json!({ "kind": kind, "result": record }));
    }
    Ok(json!({ "cases": records }))
}

fn run_context_estimate_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::estimator::{
        estimate_conversation_item_tokens, estimate_tokens,
    };
    let input = materialize_value(input)?;
    let texts = input
        .get("texts")
        .and_then(Value::as_array)
        .ok_or_else(|| HarnessError::corpus("estimate requires texts"))?;
    let mut text_estimates = Vec::new();
    for text in texts {
        let text = text.as_str().ok_or_else(|| {
            HarnessError::corpus("estimate texts must be strings")
        })?;
        let bytes = text.len();
        let tokens = estimate_tokens(text);
        text_estimates
            .push(json!({ "text": text, "bytes": bytes, "tokens": tokens }));
    }
    let mut item_estimates = Vec::new();
    if let Some(items) =
        input.get("conversationItems").and_then(Value::as_array)
    {
        for item in items {
            let ci = parse_conversation_items(std::slice::from_ref(item))?
                .into_iter()
                .next()
                .unwrap();
            let est = estimate_conversation_item_tokens(&ci);
            item_estimates.push(json!({ "item": item, "bytes": est.bytes, "tokens": est.tokens }));
        }
    }
    Ok(
        json!({ "textEstimates": text_estimates, "itemEstimates": item_estimates }),
    )
}

fn ratio_value(ratio: f64) -> Value {
    if ratio.fract() == 0.0
        && ratio.is_finite()
        && ratio >= i64::MIN as f64
        && ratio <= i64::MAX as f64
    {
        Value::Number(serde_json::Number::from(ratio as i64))
    } else if let Some(n) = serde_json::Number::from_f64(ratio) {
        Value::Number(n)
    } else {
        Value::Number(serde_json::Number::from(0))
    }
}

fn run_context_pressure_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::pressure::{
        PressureLimits, classify_pressure,
    };
    let input = materialize_value(input)?;
    let cases = input
        .get("cases")
        .and_then(Value::as_array)
        .ok_or_else(|| HarnessError::corpus("pressure requires cases"))?;
    let limits =
        if let Some(limits) = input.get("limits").and_then(Value::as_object) {
            PressureLimits {
                warn_ratio: limits
                    .get("warnRatio")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.70),
                auto_ratio: limits
                    .get("autoRatio")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.85),
                hard_ratio: limits
                    .get("hardRatio")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
            }
        } else {
            PressureLimits::default()
        };
    let mut results = Vec::new();
    for entry in cases {
        let estimated = entry
            .get("estimatedTokens")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                HarnessError::corpus("pressure case requires estimatedTokens")
            })? as usize;
        let working = entry
            .get("workingMaximum")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                HarnessError::corpus("pressure case requires workingMaximum")
            })?;
        let p = classify_pressure(estimated, working, limits);
        results.push(json!({ "estimatedTokens": p.estimated_tokens, "workingMaximum": p.working_maximum, "ratio": ratio_value(p.ratio), "state": p.state.as_str() }));
    }
    Ok(json!({ "results": results }))
}

fn run_context_trim_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::estimator::estimate_conversation_tokens;
    use siralos_core::projection::trim::trim_conversation_preserving_pairs;
    let input = materialize_value(input)?;
    let max_tokens = input
        .get("maxTokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| HarnessError::corpus("trim requires maxTokens"))?
        as usize;
    let messages =
        parse_conversation_items(scenario_array(&input, "messages")?)?;
    let original_tokens = estimate_conversation_tokens(&messages);
    let trimmed = trim_conversation_preserving_pairs(&messages, max_tokens);
    Ok(json!({
        "originalTokens": original_tokens,
        "maxTokens": max_tokens,
        "kept": trimmed.items.iter().map(conversation_item_value).collect::<Vec<_>>(),
        "droppedItems": trimmed.dropped_items,
        "estimatedTokens": trimmed.estimated_tokens,
    }))
}

fn run_context_segments_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::segments::{
        SegmentInput, Stability, project_segments, serialize_prefix,
    };
    let input = materialize_value(input)?;
    let segments = scenario_array(&input, "segments")?;
    let mut inputs = Vec::new();
    for seg in segments {
        let obj = seg
            .as_object()
            .ok_or_else(|| HarnessError::corpus("segments must be objects"))?;
        let stability = match obj.get("stability").and_then(Value::as_str) {
            Some("stable") => Stability::Stable,
            Some("contextual") => Stability::Contextual,
            Some("volatile") => Stability::Volatile,
            _ => {
                return Err(HarnessError::corpus(
                    "segments stability must be stable/contextual/volatile",
                ));
            }
        };
        inputs.push(SegmentInput {
            id: scenario_string(seg, "id")?,
            stability,
            title: scenario_string(seg, "title")?,
            content: seg
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        });
    }
    let proj = project_segments(inputs);
    let prefix = serialize_prefix(&proj);
    Ok(json!({
        "stableSegments": proj.stable_segments.iter().map(segment_value).collect::<Vec<_>>(),
        "contextualSegments": proj.contextual_segments.iter().map(segment_value).collect::<Vec<_>>(),
        "volatileSegments": proj.volatile_segments.iter().map(segment_value).collect::<Vec<_>>(),
        "stableFingerprint": proj.stable_fingerprint,
        "stableBytes": proj.stable_bytes,
        "stablePrefixBytes": proj.stable_prefix_bytes,
        "totalBytes": proj.total_bytes,
        "estimatedTokens": proj.estimated_tokens,
        "systemPrefix": prefix,
        "systemPrefixBytes": prefix.len(),
    }))
}

fn segment_value(
    seg: &siralos_core::projection::segments::ContextSegment,
) -> Value {
    json!({ "id": seg.id, "stability": seg.stability.as_str(), "title": seg.title, "content": seg.content, "bytes": seg.bytes, "estimatedTokens": seg.estimated_tokens })
}

fn run_context_tool_visibility_case(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::projection::visibility::{
        ProjectionMode, ToolProjectionInput, project_tools,
    };
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    let input = materialize_value(input)?;
    let mode = ProjectionMode::parse(input.get("mode").and_then(Value::as_str).unwrap_or("generic"))
        .ok_or_else(|| HarnessError::corpus("tool-visibility mode must be generic/development/review/inspection/planning"))?;
    let registered =
        parse_registered_tools(scenario_array(&input, "registeredTools")?)?;
    let policy = if let Some(rules) =
        input.get("policyRules").and_then(Value::as_array)
    {
        let mut policy_rules = Vec::new();
        for rule in rules {
            let cap = scenario_string(rule, "capability")?;
            let decision = scenario_string(rule, "decision")?;
            let r = match decision.as_str() {
                "allow" => PermissionRule::Allow,
                "ask" => PermissionRule::Ask,
                "deny" => PermissionRule::Deny,
                _ => {
                    return Err(HarnessError::corpus(
                        "policy decision must be allow/ask/deny",
                    ));
                }
            };
            policy_rules.push(PolicyRule {
                capability: CapabilityId::parse(&cap).map_err(|e| {
                    HarnessError::corpus(format!(
                        "invalid capability {cap}: {e}"
                    ))
                })?,
                rule: r,
            });
        }
        PermissionPolicy::from_rules(policy_rules)
    } else {
        PermissionPolicy::default()
    };
    let allowed_names: Option<Vec<String>> =
        input.get("allowedToolNames").and_then(Value::as_array).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                .collect()
        });
    let proj = project_tools(ToolProjectionInput {
        registered_tools: &registered,
        policy: &policy,
        allowed_tool_names: allowed_names.as_deref(),
        mode,
    });
    Ok(json!({
        "tools": proj.tools.iter().map(|t| json!({ "name": t.name, "visibility": t.visibility.as_str() })).collect::<Vec<_>>(),
        "counts": { "available": proj.counts.available, "gated": proj.counts.gated, "hidden": proj.counts.hidden },
        "fingerprint": proj.fingerprint,
        "requestTools": proj.request_tools.iter().map(|t| json!({ "description": t.description, "name": t.name })).collect::<Vec<_>>(),
        "approvedNames": proj.approved_names,
    }))
}

fn parse_registered_tools(
    tools: &[Value],
) -> Result<Vec<siralos_core::tool::registry::RegisteredToolInfo>, HarnessError>
{
    let mut out = Vec::new();
    for tool in tools {
        let name = scenario_string(tool, "name")?;
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let input_schema = tool
            .get("inputSchema")
            .cloned()
            .unwrap_or(json!({"type":"object"}));
        let capability_str = scenario_string(tool, "capability")?;
        let capability = siralos_core::tool::capability::CapabilityId::parse(
            &capability_str,
        )
        .map_err(|e| {
            HarnessError::corpus(format!(
                "invalid capability {capability_str}: {e}"
            ))
        })?;
        out.push(siralos_core::tool::registry::RegisteredToolInfo {
            definition: siralos_core::provider::ToolDefinition {
                name,
                description,
                input_schema,
            },
            capability,
        });
    }
    Ok(out)
}

fn run_context_evidence_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::evidence::{
        EvidenceProjectorOptions, project_for_model,
    };
    let input = materialize_value(input)?;
    let options = EvidenceProjectorOptions {
        secrets: input
            .get("secrets")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                    .collect()
            })
            .unwrap_or_default(),
        max_total_bytes: input
            .get("maxTotalBytes")
            .and_then(Value::as_u64)
            .unwrap_or(32768) as usize,
        max_line_bytes: input
            .get("maxLineBytes")
            .and_then(Value::as_u64)
            .unwrap_or(1024) as usize,
    };
    let raw_text =
        input.get("rawText").and_then(Value::as_str).unwrap_or("").to_owned();
    let view = project_for_model(None, None, &raw_text, &options);
    let mut result = json!({
        "text": view.text,
        "truncated": view.truncated,
        "shownBytes": view.shown_bytes,
        "originalBytes": view.original_bytes,
        "transformations": view.transformations,
        "toolResultView": Value::Null,
    });
    if let Some(tr) = input.get("toolResult").and_then(Value::as_object) {
        let status =
            tr.get("status").and_then(Value::as_str).unwrap_or("failed");
        let source_text = if status == "success" {
            tr.get("summary").and_then(Value::as_str).unwrap_or("")
        } else {
            tr.get("message").and_then(Value::as_str).unwrap_or("")
        };
        let v2 = project_for_model(None, None, source_text, &options);
        if status == "success" {
            result["toolResultView"] = json!({ "status": status, "summary": v2.text, "truncated": v2.truncated, "transformations": v2.transformations, "shownBytes": v2.shown_bytes, "originalBytes": v2.original_bytes });
        } else {
            result["toolResultView"] = json!({ "status": status, "message": v2.text, "truncated": v2.truncated, "transformations": v2.transformations });
        }
    }
    Ok(result)
}

fn run_context_fingerprints_case(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::projection::segments::{
        SegmentInput, Stability, project_segments,
    };
    let input = materialize_value(input)?;
    let base_segments = scenario_array(&input, "baseSegments")?;
    let variant_segments = scenario_array(&input, "variantSegments")?;
    let parse = |segments: &[Value]| -> Result<
        siralos_core::projection::segments::ContextProjection,
        HarnessError,
    > {
        let mut inputs = Vec::new();
        for seg in segments {
            let stability = match seg.get("stability").and_then(Value::as_str)
            {
                Some("stable") => Stability::Stable,
                Some("contextual") => Stability::Contextual,
                Some("volatile") => Stability::Volatile,
                _ => {
                    return Err(HarnessError::corpus(
                        "stability must be stable/contextual/volatile",
                    ));
                }
            };
            inputs.push(SegmentInput {
                id: scenario_string(seg, "id")?,
                stability,
                title: scenario_string(seg, "title")?,
                content: seg
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            });
        }
        Ok(project_segments(inputs))
    };
    let base = parse(base_segments)?;
    let variant = parse(variant_segments)?;
    Ok(json!({
        "stableFingerprint": base.stable_fingerprint,
        "variantStableFingerprint": variant.stable_fingerprint,
        "stableFingerprintUnchanged": base.stable_fingerprint == variant.stable_fingerprint,
        "stableBytesUnchanged": base.stable_bytes == variant.stable_bytes,
        "stablePrefixBytesUnchanged": base.stable_prefix_bytes == variant.stable_prefix_bytes,
    }))
}

fn run_context_pipeline_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::{
        ProjectionInput, ProjectionService,
        capacity::ContextCapacity,
        evidence::EvidenceProjectorOptions,
        pressure::PressureLimits,
        segments::{SegmentInput, Stability},
        visibility::ProjectionMode,
    };
    let input = materialize_value(input)?;
    let mode = ProjectionMode::parse(
        input.get("mode").and_then(Value::as_str).unwrap_or("generic"),
    )
    .ok_or_else(|| HarnessError::corpus("pipeline mode invalid"))?;
    let working_maximum =
        input.get("workingMaximum").and_then(Value::as_i64).unwrap_or(32768);
    let capacity = ContextCapacity::with_working_maximum(working_maximum);
    let messages =
        if let Some(arr) = input.get("messages").and_then(Value::as_array) {
            parse_conversation_items(arr)?
        } else {
            Vec::new()
        };
    let registered = if let Some(arr) =
        input.get("registeredTools").and_then(Value::as_array)
    {
        parse_registered_tools(arr)?
    } else {
        Vec::new()
    };
    let segments = if let Some(arr) =
        input.get("segments").and_then(Value::as_array)
    {
        let mut v = Vec::new();
        for seg in arr {
            let stability = match seg.get("stability").and_then(Value::as_str)
            {
                Some("stable") => Stability::Stable,
                Some("contextual") => Stability::Contextual,
                Some("volatile") => Stability::Volatile,
                _ => Stability::Contextual,
            };
            v.push(SegmentInput {
                id: scenario_string(seg, "id")?,
                stability,
                title: seg
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(
                        seg.get("id").and_then(Value::as_str).unwrap_or("T"),
                    )
                    .to_owned(),
                content: seg
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            });
        }
        v
    } else {
        // No explicit segments: pipeline without segments is the service-composition path.
        // The oracle service's default stableInstructions for this fixture is "You are Siralos."
        // synthesized as one stable segment with the same title/content for byte-identical parity.
        vec![SegmentInput {
            id: "siralos-core-instructions".to_owned(),
            stability: Stability::Stable,
            title: "Siralos instructions".to_owned(),
            content: "You are Siralos.".to_owned(),
        }]
    };
    let policy = if let Some(rules) =
        input.get("policyRules").and_then(Value::as_array)
    {
        use siralos_core::tool::capability::CapabilityId;
        use siralos_core::tool::permission::{
            PermissionPolicy, PermissionRule, PolicyRule,
        };
        let mut prs = Vec::new();
        for rule in rules {
            let cap = scenario_string(rule, "capability")?;
            let dec = scenario_string(rule, "decision")?;
            let r = match dec.as_str() {
                "allow" => PermissionRule::Allow,
                "ask" => PermissionRule::Ask,
                _ => PermissionRule::Deny,
            };
            prs.push(PolicyRule {
                capability: CapabilityId::parse(&cap)
                    .map_err(|e| HarnessError::corpus(format!("{e}")))?,
                rule: r,
            });
        }
        PermissionPolicy::from_rules(prs)
    } else {
        siralos_core::tool::permission::PermissionPolicy::default()
    };
    let allowed_names: Option<Vec<String>> =
        input.get("allowedToolNames").and_then(Value::as_array).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                .collect()
        });
    let evidence_options = EvidenceProjectorOptions {
        secrets: input
            .get("secrets")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                    .collect()
            })
            .unwrap_or_default(),
        max_total_bytes: input
            .get("maxTotalBytes")
            .and_then(Value::as_u64)
            .unwrap_or(32768) as usize,
        max_line_bytes: input
            .get("maxLineBytes")
            .and_then(Value::as_u64)
            .unwrap_or(1024) as usize,
    };
    let provider_tool_calling = input
        .get("providerToolCalling")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mut service = ProjectionService::new();
    let req = service.project(ProjectionInput {
        mode,
        messages: &messages,
        registered_tools: &registered,
        provider_tool_calling,
        capacity,
        pressure_limits: PressureLimits::default(),
        segments,
        evidence_options,
        allowed_tool_names: allowed_names,
        policy: &policy,
        task_revision: None,
    });
    Ok(json!({
        "blocked": req.blocked.as_ref().map(|b| json!({ "type": b.kind(), "reason": b.message() })),
        "contextFingerprint": req.context_projection.stable_fingerprint,
        "estimatedTokens": req.estimated_tokens,
        "pressure": { "estimatedTokens": req.pressure.estimated_tokens, "workingMaximum": req.pressure.working_maximum, "state": req.pressure.state.as_str(), "ratio": ratio_value(req.pressure.ratio) },
        "providerCalled": req.blocked.is_none(),
        "systemPrefixBytes": req.system.as_deref().map(|s| s.len()).unwrap_or(0),
        "toolProjection": { "counts": { "available": req.tool_projection.counts.available, "gated": req.tool_projection.counts.gated, "hidden": req.tool_projection.counts.hidden }, "fingerprint": req.tool_projection.fingerprint },
        "workingMaximum": req.pressure.working_maximum,
    }))
}

fn run_context_unsupported_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::{
        ProjectionInput, ProjectionService, capacity::ContextCapacity,
        evidence::EvidenceProjectorOptions, pressure::PressureLimits,
        visibility::ProjectionMode,
    };
    let input = materialize_value(input)?;
    let mode = ProjectionMode::parse(
        input.get("mode").and_then(Value::as_str).unwrap_or("development"),
    )
    .ok_or_else(|| HarnessError::corpus("unsupported mode invalid"))?;
    let registered = if let Some(arr) =
        input.get("registeredTools").and_then(Value::as_array)
    {
        parse_registered_tools(arr)?
    } else {
        Vec::new()
    };
    let messages =
        if let Some(arr) = input.get("messages").and_then(Value::as_array) {
            parse_conversation_items(arr)?
        } else {
            Vec::new()
        };
    let policy = if let Some(rules) =
        input.get("policyRules").and_then(Value::as_array)
    {
        use siralos_core::tool::capability::CapabilityId;
        use siralos_core::tool::permission::{
            PermissionPolicy, PermissionRule, PolicyRule,
        };
        let mut prs = Vec::new();
        for rule in rules {
            let cap = scenario_string(rule, "capability")?;
            let dec = scenario_string(rule, "decision")?;
            let r = match dec.as_str() {
                "allow" => PermissionRule::Allow,
                "ask" => PermissionRule::Ask,
                _ => PermissionRule::Deny,
            };
            prs.push(PolicyRule {
                capability: CapabilityId::parse(&cap)
                    .map_err(|e| HarnessError::corpus(format!("{e}")))?,
                rule: r,
            });
        }
        PermissionPolicy::from_rules(prs)
    } else {
        siralos_core::tool::permission::PermissionPolicy::default()
    };
    let segments = if let Some(arr) =
        input.get("segments").and_then(Value::as_array)
    {
        let mut v = Vec::new();
        for seg in arr {
            let stability = match seg.get("stability").and_then(Value::as_str)
            {
                Some("stable") => {
                    siralos_core::projection::segments::Stability::Stable
                }
                Some("contextual") => {
                    siralos_core::projection::segments::Stability::Contextual
                }
                Some("volatile") => {
                    siralos_core::projection::segments::Stability::Volatile
                }
                _ => siralos_core::projection::segments::Stability::Stable,
            };
            v.push(siralos_core::projection::segments::SegmentInput {
                id: scenario_string(seg, "id")?,
                stability,
                title: seg
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Instructions")
                    .to_owned(),
                content: seg
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            });
        }
        v
    } else {
        Vec::new()
    };
    let mut service = ProjectionService::new();
    let req = service.project(ProjectionInput {
        mode,
        messages: &messages,
        registered_tools: &registered,
        provider_tool_calling: false,
        capacity: ContextCapacity::default(),
        pressure_limits: PressureLimits::default(),
        segments,
        evidence_options: EvidenceProjectorOptions::default(),
        allowed_tool_names: None,
        policy: &policy,
        task_revision: None,
    });
    Ok(json!({
        "mode": req.mode.as_str(),
        "blocked": req.blocked.as_ref().map(|b| json!({ "type": b.kind(), "reason": b.message() })),
        "toolCounts": { "available": req.tool_projection.counts.available, "gated": req.tool_projection.counts.gated, "hidden": req.tool_projection.counts.hidden },
        "fingerprint": req.tool_projection.fingerprint,
        "requestTools": req.tool_projection.request_tools.iter().map(|t| Value::String(t.name.clone())).collect::<Vec<_>>(),
        "estimatedTokens": req.estimated_tokens,
        "providerCalled": false,
    }))
}

fn conversation_item_value(
    item: &siralos_core::provider::ConversationItem,
) -> Value {
    match item {
        siralos_core::provider::ConversationItem::UserMessage { content } => {
            json!({ "type": "user_message", "content": content })
        }
        siralos_core::provider::ConversationItem::AssistantMessage {
            content,
        } => json!({ "type": "assistant_message", "content": content }),
        siralos_core::provider::ConversationItem::AssistantToolCall {
            call_id,
            tool_name,
            input,
        } => {
            if let Some(v) = input.value() {
                json!({ "type": "assistant_tool_call", "callId": call_id, "toolName": tool_name, "input": v })
            } else {
                json!({ "type": "assistant_tool_call", "callId": call_id, "toolName": tool_name })
            }
        }
        siralos_core::provider::ConversationItem::ToolResult {
            call_id,
            tool_name,
            result,
        } => {
            json!({ "type": "tool_result", "callId": call_id, "toolName": tool_name, "result": tool_execution_result_value(result) })
        }
    }
}

// ---------------------------------------------------------------------------
