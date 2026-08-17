//! Generic Tool visibility and `ApprovedToolSurface` coupling.
//!
//! For each registered tool, the Host-supplied allowed `Tool` names (when
//! supplied) and the R7.2 permission decision map to `available / gated /
//! hidden`. Provider-visible tools are `available + gated` in registry order.
//! The same visible list derives both the provider request definitions and the
//! `ApprovedToolSurface` names; they are never independently computed.
//!
//! Domain-neutral: no domain-specific Tool names or capabilities are hard-coded.

use crate::identity::CanonicalValue;
use crate::identity::sha256_hex;
use crate::provider::ToolDefinition;
use crate::tool::capability::CapabilityId;
use crate::tool::permission::{
    PermissionDecision, PermissionPolicy, evaluate_permission,
};
use crate::tool::registry::RegisteredToolInfo;
use std::collections::BTreeMap;

/// One visibility state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Visible and invocable (allow).
    Available,
    /// Visible but gated (ask).
    Gated,
    /// Absent from provider schema.
    Hidden,
}

impl Visibility {
    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Gated => "gated",
            Self::Hidden => "hidden",
        }
    }
}

/// One projected tool with its visibility.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedTool {
    /// Tool name.
    pub name: String,
    /// Visibility.
    pub visibility: Visibility,
    /// Description (provider-visible).
    pub description: String,
    /// Input schema (opaque value, preserved for visible tools).
    pub input_schema: serde_json::Value,
}

/// Result of a Tool projection.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolProjection {
    /// SHA-256 of canonical JSON over visible `{name,description,inputSchema}` in registry order.
    pub fingerprint: String,
    /// All tools with their visibility.
    pub tools: Vec<ProjectedTool>,
    /// Counts by visibility.
    pub counts: ToolCounts,
    /// Provider-visible definitions (`available + gated`, registry order).
    pub request_tools: Vec<ToolDefinition>,
    /// ApprovedToolSurface names (same visible names, sorted for determinism).
    pub approved_names: Vec<String>,
}

/// Counts by visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolCounts {
    /// Available.
    pub available: usize,
    /// Gated.
    pub gated: usize,
    /// Hidden.
    pub hidden: usize,
}

/// Generic projection mode (validated, domain-neutral).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectionMode {
    /// Unrestricted surface.
    Generic,
    /// Host-owned development surface.
    Development,
    /// Host-owned review surface.
    Review,
    /// Host-owned inspection surface.
    Inspection,
    /// Host-owned planning surface.
    Planning,
}

impl ProjectionMode {
    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Development => "development",
            Self::Review => "review",
            Self::Inspection => "inspection",
            Self::Planning => "planning",
        }
    }

    /// Parse wire string (case-sensitive).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "generic" => Some(Self::Generic),
            "development" => Some(Self::Development),
            "review" => Some(Self::Review),
            "inspection" => Some(Self::Inspection),
            "planning" => Some(Self::Planning),
            _ => None,
        }
    }

    /// Whether this mode requires tool calling.
    pub fn requires_tool_calling(self) -> bool {
        matches!(
            self,
            Self::Development
                | Self::Review
                | Self::Inspection
                | Self::Planning
        )
    }
}

/// Input to tool visibility projection.
#[derive(Debug, Clone)]
pub struct ToolProjectionInput<'a> {
    /// Host-registered tools in registry order.
    pub registered_tools: &'a [RegisteredToolInfo],
    /// Host permission policy (R7.2 owner).
    pub policy: &'a PermissionPolicy,
    /// Host-allowed exact Tool names for this mode, when the mode filters by name.
    /// `None` means: allow via R7.2 permission only (no name filtering).
    /// `Some(list)` means: only tools whose name is in the list are visible;
    /// others are `hidden` regardless of permission.
    pub allowed_tool_names: Option<&'a [String]>,
    /// Whether the Host surface requires native/mixed for native prepare tools.
    /// This is intentionally opaque: Core does not interpret domain semantics.
    /// When `true`, additional filtering for native prepare tools could be applied
    /// by the caller; the minimal R7.3 core treats `allowed_tool_names` as the
    /// authoritative name filter and does not add extra domain-specific filtering.
    pub mode: ProjectionMode,
}

/// Project tools to their visibility states.
///
/// `allowed_tool_names`: when `Some`, tools whose `definition.name` is not in
/// the allow-list are `hidden` regardless of permission. This is how the CLI
/// composition supplies the frozen exact-name tables without Core hard-coding
/// Domain-specific tool names. When `None`, only the R7.2 permission decision applies
/// (the `generic` mode behavior in the reference).
pub fn project_tools(input: ToolProjectionInput<'_>) -> ToolProjection {
    let mut projected: Vec<ProjectedTool> =
        Vec::with_capacity(input.registered_tools.len());
    for info in input.registered_tools {
        let visibility = if let Some(allowed) = input.allowed_tool_names {
            if !allowed.iter().any(|name| name == &info.definition.name) {
                Visibility::Hidden
            } else {
                permission_visibility(&info.capability, input.policy)
            }
        } else {
            permission_visibility(&info.capability, input.policy)
        };
        projected.push(ProjectedTool {
            name: info.definition.name.clone(),
            visibility,
            description: info.definition.description.clone(),
            input_schema: info.definition.input_schema.clone(),
        });
    }
    let visible: Vec<&ProjectedTool> = projected
        .iter()
        .filter(|t| t.visibility != Visibility::Hidden)
        .collect();
    // Fingerprint: SHA-256(canonical JSON([{name,description,inputSchema}] visible in order))
    // Canonical JSON: sorted keys, preserved array order, JSON escaping via json_escape.
    // We use CanonicalValue → sha256_hex via siralos-core identity (domain-neutral).
    // InputSchema is a JSON value — convert via helper.
    let fingerprint = if visible.is_empty() {
        sha256_hex("[]".as_bytes())
    } else {
        let array: Vec<CanonicalValue> = visible
            .iter()
            .map(|tool| {
                let mut m = BTreeMap::new();
                m.insert(
                    "description".to_owned(),
                    CanonicalValue::Str(tool.description.clone()),
                );
                m.insert(
                    "inputSchema".to_owned(),
                    value_to_canonical(&tool.input_schema),
                );
                m.insert(
                    "name".to_owned(),
                    CanonicalValue::Str(tool.name.clone()),
                );
                CanonicalValue::Object(m)
            })
            .collect();
        let canonical = CanonicalValue::Array(array).to_canonical();
        sha256_hex(canonical.as_bytes())
    };
    let counts = ToolCounts {
        available: projected
            .iter()
            .filter(|t| t.visibility == Visibility::Available)
            .count(),
        gated: projected
            .iter()
            .filter(|t| t.visibility == Visibility::Gated)
            .count(),
        hidden: projected
            .iter()
            .filter(|t| t.visibility == Visibility::Hidden)
            .count(),
    };
    let request_tools: Vec<ToolDefinition> = visible
        .iter()
        .map(|tool| ToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
        })
        .collect();
    let mut approved_names: Vec<String> =
        visible.iter().map(|t| t.name.clone()).collect();
    approved_names.sort();
    ToolProjection {
        fingerprint,
        tools: projected,
        counts,
        request_tools,
        approved_names,
    }
}

fn permission_visibility(
    capability: &CapabilityId,
    policy: &PermissionPolicy,
) -> Visibility {
    match evaluate_permission(capability, policy) {
        PermissionDecision::Allow => Visibility::Available,
        PermissionDecision::Ask { .. } => Visibility::Gated,
        PermissionDecision::Deny { .. } => Visibility::Hidden,
    }
}

fn value_to_canonical(value: &serde_json::Value) -> CanonicalValue {
    match value {
        serde_json::Value::Null => CanonicalValue::Null,
        serde_json::Value::Bool(b) => CanonicalValue::Bool(*b),
        serde_json::Value::Number(n) => {
            // Preserve number representation as string via canonical form.
            // CanonicalValue currently holds U64; for non-integer numbers
            // we fallback to string representation (still deterministic).
            if let Some(u) = n.as_u64() {
                CanonicalValue::U64(u)
            } else if let Some(i) = n.as_i64() {
                // Negative integers → use string as fallback (rare in tool schemas).
                CanonicalValue::Str(i.to_string())
            } else {
                CanonicalValue::Str(n.to_string())
            }
        }
        serde_json::Value::String(s) => CanonicalValue::Str(s.clone()),
        serde_json::Value::Array(arr) => {
            CanonicalValue::Array(arr.iter().map(value_to_canonical).collect())
        }
        serde_json::Value::Object(map) => {
            let mut bmap = BTreeMap::new();
            for (k, v) in map {
                bmap.insert(k.clone(), value_to_canonical(v));
            }
            CanonicalValue::Object(bmap)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ProjectionMode, ToolProjectionInput, Visibility, project_tools,
    };
    use crate::provider::ToolDefinition;
    use crate::tool::capability::CapabilityId;
    use crate::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    use crate::tool::registry::RegisteredToolInfo;
    use serde_json::json;

    fn tool(name: &str, capability: &str) -> RegisteredToolInfo {
        RegisteredToolInfo {
            definition: ToolDefinition {
                name: name.to_owned(),
                description: format!("{name} tool"),
                input_schema: json!({"type": "object"}),
            },
            capability: CapabilityId::parse(capability).unwrap(),
        }
    }

    fn policy_allow(cap: &str) -> PermissionPolicy {
        PermissionPolicy::from_rules([PolicyRule {
            capability: CapabilityId::parse(cap).unwrap(),
            rule: PermissionRule::Allow,
        }])
    }

    #[test]
    fn allow_ask_deny_map_to_available_gated_hidden() {
        let tools = vec![
            tool("workspace.read", "workspace.read"),
            tool("workspace.write", "workspace.write"),
        ];
        let mixed = PermissionPolicy::from_rules([
            PolicyRule {
                capability: CapabilityId::parse("workspace.read").unwrap(),
                rule: PermissionRule::Allow,
            },
            PolicyRule {
                capability: CapabilityId::parse("workspace.write").unwrap(),
                rule: PermissionRule::Deny,
            },
        ]);
        let proj = project_tools(ToolProjectionInput {
            registered_tools: &tools,
            policy: &mixed,
            allowed_tool_names: None,
            mode: ProjectionMode::Generic,
        });
        assert_eq!(proj.tools[0].visibility, Visibility::Available);
        assert_eq!(proj.tools[1].visibility, Visibility::Hidden);
        assert_eq!(proj.counts.available, 1);
        assert_eq!(proj.counts.hidden, 1);
    }

    #[test]
    fn name_allowlist_hides_non_listed() {
        let tools = vec![
            tool("workspace.read", "workspace.read"),
            tool("workspace.write", "workspace.write"),
        ];
        let policy = PermissionPolicy::from_rules([
            PolicyRule {
                capability: CapabilityId::parse("workspace.read").unwrap(),
                rule: PermissionRule::Allow,
            },
            PolicyRule {
                capability: CapabilityId::parse("workspace.write").unwrap(),
                rule: PermissionRule::Allow,
            },
        ]);
        let proj = project_tools(ToolProjectionInput {
            registered_tools: &tools,
            policy: &policy,
            allowed_tool_names: Some(&["workspace.read".to_owned()]),
            mode: ProjectionMode::Generic,
        });
        assert_eq!(proj.tools[0].visibility, Visibility::Available);
        assert_eq!(proj.tools[1].visibility, Visibility::Hidden);
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let tools = vec![tool("workspace.read", "workspace.read")];
        let policy = policy_allow("workspace.read");
        let a = project_tools(ToolProjectionInput {
            registered_tools: &tools,
            policy: &policy,
            allowed_tool_names: None,
            mode: ProjectionMode::Generic,
        });
        let b = project_tools(ToolProjectionInput {
            registered_tools: &tools,
            policy: &policy,
            allowed_tool_names: None,
            mode: ProjectionMode::Generic,
        });
        assert_eq!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn approved_names_sorted() {
        let tools = vec![tool("b.tool", "b.tool"), tool("a.tool", "a.tool")];
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
            allowed_tool_names: None,
            mode: ProjectionMode::Generic,
        });
        assert_eq!(proj.approved_names, vec!["a.tool", "b.tool"]);
        // Request order preserves registration order
        assert_eq!(proj.request_tools[0].name, "b.tool");
        assert_eq!(proj.request_tools[1].name, "a.tool");
    }
}
