//! Adversarial/security tests for R7.3 projection invariants.

#[cfg(test)]
mod tests {
    use crate::projection::evidence::{
        EvidenceProjectorOptions, project_for_model, strip_ansi_and_control,
    };
    use crate::projection::trim::trim_conversation_preserving_pairs;
    use crate::provider::{
        AssistantToolCallInput, ConversationItem, ToolExecutionResult,
    };
    use serde_json::json;

    #[test]
    fn raw_history_unchanged_by_trim() {
        let items = vec![
            ConversationItem::UserMessage { content: "u1".to_owned() },
            ConversationItem::AssistantMessage { content: "a1".to_owned() },
            ConversationItem::AssistantToolCall {
                call_id: "c1".to_owned(),
                tool_name: "t".to_owned(),
                input: AssistantToolCallInput::Present(json!({})),
            },
            ConversationItem::ToolResult {
                call_id: "c1".to_owned(),
                tool_name: "t".to_owned(),
                result: ToolExecutionResult::Success {
                    output: json!({}),
                    summary: "r1".to_owned(),
                },
            },
            ConversationItem::UserMessage { content: "u2".to_owned() },
        ];
        let original = items.clone();
        let _ = trim_conversation_preserving_pairs(&items, 1);
        assert_eq!(items, original);
    }

    #[test]
    fn raw_tool_result_unchanged_by_evidence_projection() {
        let result = ToolExecutionResult::Success {
            output: json!({"data": 1}),
            summary: "secret-token hello".to_owned(),
        };
        let before = result.clone();
        let _ = project_for_model(
            None,
            None,
            "secret-token hello",
            &EvidenceProjectorOptions {
                secrets: vec!["secret-token".to_owned()],
                max_total_bytes: 32768,
                max_line_bytes: 1024,
            },
        );
        assert_eq!(result, before);
    }

    #[test]
    fn secret_removed_before_collapse_cannot_reappear() {
        let view = project_for_model(
            None,
            None,
            "s\ns\ns",
            &EvidenceProjectorOptions {
                secrets: vec!["s".to_owned()],
                max_total_bytes: 1000,
                max_line_bytes: 20,
            },
        );
        let has_secret =
            view.text.contains('s') && !view.text.contains("[REDACTED]");
        assert!(!has_secret);
        assert!(view.transformations.contains(&"redact-secrets".to_owned()));
        assert!(
            !view
                .transformations
                .contains(&"collapse-repeated-lines".to_owned())
        );
    }

    #[test]
    fn stripped_controls_cannot_reappear() {
        let raw = "\u{001b}[31mred\u{001b}[0m line\u{0007}\u{0001}more";
        let view = project_for_model(
            None,
            None,
            raw,
            &EvidenceProjectorOptions::default(),
        );
        assert!(!view.text.contains('\u{001b}'));
        assert!(!view.text.contains('\u{0007}'));
        assert!(!view.text.contains('\u{0001}'));
    }

    #[test]
    fn unicode_never_split_into_invalid_utf8() {
        let raws: Vec<String> = vec![
            "\u{1F600}\u{1F600}".to_owned(),
            "a".repeat(1023) + "\u{1F600}",
            "\u{1F600}hello".to_owned(),
            "e\u{0301}".to_owned(),
        ];
        for raw in raws {
            let view = project_for_model(
                None,
                None,
                &raw,
                &EvidenceProjectorOptions {
                    secrets: Vec::new(),
                    max_total_bytes: 10,
                    max_line_bytes: 4,
                },
            );
            assert!(std::str::from_utf8(view.text.as_bytes()).is_ok());
            assert!(!view.text.contains('\u{FFFD}'));
        }
    }

    #[test]
    fn terminal_marker_precedence_exact() {
        let view = project_for_model(
            None,
            None,
            &"x".repeat(5000),
            &EvidenceProjectorOptions {
                secrets: Vec::new(),
                max_total_bytes: 32,
                max_line_bytes: 4,
            },
        );
        assert!(view.truncated);
        assert!(view.text.contains("[truncated]"));
        assert!(view.text.ends_with("[truncated]"));
    }

    #[test]
    fn hidden_tools_cannot_enter_provider_schema() {
        use crate::projection::visibility::{
            ProjectionMode, ToolProjectionInput, Visibility, project_tools,
        };
        use crate::provider::ToolDefinition;
        use crate::tool::capability::CapabilityId;
        use crate::tool::permission::{
            PermissionPolicy, PermissionRule, PolicyRule,
        };
        use crate::tool::registry::RegisteredToolInfo;
        let tools = vec![
            RegisteredToolInfo {
                definition: ToolDefinition {
                    name: "workspace.read".to_owned(),
                    description: "r".to_owned(),
                    input_schema: json!({"type":"object"}),
                },
                capability: CapabilityId::parse("workspace.read").unwrap(),
            },
            RegisteredToolInfo {
                definition: ToolDefinition {
                    name: "secret.tool".to_owned(),
                    description: "s".to_owned(),
                    input_schema: json!({"type":"object"}),
                },
                capability: CapabilityId::parse("secret.tool").unwrap(),
            },
        ];
        let policy = PermissionPolicy::from_rules([
            PolicyRule {
                capability: CapabilityId::parse("workspace.read").unwrap(),
                rule: PermissionRule::Allow,
            },
            PolicyRule {
                capability: CapabilityId::parse("secret.tool").unwrap(),
                rule: PermissionRule::Deny,
            },
        ]);
        let proj = project_tools(ToolProjectionInput {
            registered_tools: &tools,
            policy: &policy,
            allowed_tool_names: None,
            mode: ProjectionMode::Generic,
        });
        assert!(proj.request_tools.iter().all(|t| t.name != "secret.tool"));
        assert!(!proj.approved_names.contains(&"secret.tool".to_owned()));
        assert!(
            proj.tools
                .iter()
                .find(|t| t.name == "secret.tool")
                .unwrap()
                .visibility
                == Visibility::Hidden
        );
    }

    #[test]
    fn approved_surface_derives_from_same_visible_list() {
        use crate::projection::visibility::{
            ProjectionMode, ToolProjectionInput, project_tools,
        };
        use crate::provider::ToolDefinition;
        use crate::tool::capability::CapabilityId;
        use crate::tool::permission::{
            PermissionPolicy, PermissionRule, PolicyRule,
        };
        use crate::tool::registry::RegisteredToolInfo;
        let tools = vec![
            RegisteredToolInfo {
                definition: ToolDefinition {
                    name: "b.tool".to_owned(),
                    description: "b".to_owned(),
                    input_schema: json!({"type":"object"}),
                },
                capability: CapabilityId::parse("b.tool").unwrap(),
            },
            RegisteredToolInfo {
                definition: ToolDefinition {
                    name: "a.tool".to_owned(),
                    description: "a".to_owned(),
                    input_schema: json!({"type":"object"}),
                },
                capability: CapabilityId::parse("a.tool").unwrap(),
            },
        ];
        let policy = PermissionPolicy::from_rules([
            PolicyRule {
                capability: CapabilityId::parse("b.tool").unwrap(),
                rule: PermissionRule::Allow,
            },
            PolicyRule {
                capability: CapabilityId::parse("a.tool").unwrap(),
                rule: PermissionRule::Allow,
            },
        ]);
        let proj = project_tools(ToolProjectionInput {
            registered_tools: &tools,
            policy: &policy,
            allowed_tool_names: Some(&[
                "b.tool".to_owned(),
                "a.tool".to_owned(),
            ]),
            mode: ProjectionMode::Generic,
        });
        assert_eq!(proj.request_tools[0].name, "b.tool");
        assert_eq!(proj.request_tools[1].name, "a.tool");
        assert_eq!(proj.approved_names, vec!["a.tool", "b.tool"]);
        assert_eq!(proj.approved_names.len(), proj.request_tools.len());
    }

    #[test]
    fn strip_ansi_malformed_csi() {
        let raw = "\u{001b}[12\u{0001}mhello";
        let out = strip_ansi_and_control(raw);
        assert!(!out.contains('\u{001b}'));
        assert!(!out.contains('\u{0001}'));
        assert!(out.contains("hello"));
    }
}
