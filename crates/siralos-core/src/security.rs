//! Concrete authority policy surface (Stage 3R R13.1).
//!
//! Mirrors the TypeScript reference's concrete policy object graph:
//! the capability vocabulary, the built-in default policies per sandbox
//! profile, and profile-constrained permission evaluation with exact
//! reason strings. The generic decision layer remains
//! [`crate::tool::permission`]; this module owns no second authority
//! system and grants nothing by itself.

use std::collections::BTreeMap;

use crate::tool::permission::PermissionRule;
use crate::workspace::path::is_protected_behavioral_config_path;

/// Every capability id, in canonical reference order.
pub const CAPABILITY_IDS: [&str; 14] = [
    "workspace.read",
    "workspace.write",
    "git.inspect",
    "godot.inspect",
    "godot.probe_project",
    "godot.api",
    "godot.diagnose",
    "godot.lsp",
    "godot.development",
    "process.execute",
    "network.outbound",
    "reference.inspect",
    "research.fetch",
    "self.inspect",
];

/// One capability policy: a rule per capability id.
#[derive(Debug, Clone)]
pub struct CapabilityPolicy {
    rules: BTreeMap<&'static str, PermissionRule>,
}

impl CapabilityPolicy {
    /// Build a policy from explicit entries (test/policy-derivation seam).
    pub fn from_entries(
        entries: Vec<(&'static str, PermissionRule)>,
    ) -> CapabilityPolicy {
        let mut rules = BTreeMap::new();
        for (capability, rule) in entries {
            rules.insert(capability, rule);
        }
        CapabilityPolicy { rules }
    }

    /// Look up the rule for one capability id.
    pub fn rule(&self, capability: &str) -> Option<PermissionRule> {
        self.rules.get(capability).copied()
    }

    /// Ordered `(capability, rule)` pairs in canonical capability order.
    pub fn ordered_rules(&self) -> Vec<(&'static str, PermissionRule)> {
        CAPABILITY_IDS
            .iter()
            .map(|capability| (*capability, self.rules[capability]))
            .collect()
    }
}

/// Build a total policy from per-capability overrides over deny-by-default.
fn policy_from(
    entries: [(&'static str, PermissionRule); 14],
) -> CapabilityPolicy {
    let mut rules = BTreeMap::new();
    for (capability, rule) in entries {
        rules.insert(capability, rule);
    }
    CapabilityPolicy { rules }
}

use PermissionRule::{Allow, Ask, Deny};

/// The minimal profile facts permission evaluation can observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxProfile {
    /// The profile id.
    pub id: &'static str,
    /// Whether workspace writes are constrained read-only.
    pub workspace_access_read_only: bool,
    /// Whether process execution is enabled.
    pub process_enabled: bool,
}

/// Every built-in profile id, in canonical reference order.
pub const SANDBOX_PROFILE_IDS: [&str; 7] = [
    "inspect",
    "develop-offline",
    "validation-offline",
    "godot-probe-offline",
    "godot-recovery-probe-offline",
    "godot-diagnostics-offline",
    "godot-lsp-local",
];

/// The built-in profile facts observable to permission evaluation.
///
/// Internal execution profiles mirror `validation-offline`: never
/// user-selectable and never used for tool permission evaluation beyond
/// making evaluation total.
pub fn get_built_in_profile(profile_id: &str) -> Option<SandboxProfile> {
    match profile_id {
        "inspect" => Some(SandboxProfile {
            id: "inspect",
            workspace_access_read_only: true,
            process_enabled: false,
        }),
        "develop-offline" => Some(SandboxProfile {
            id: "develop-offline",
            workspace_access_read_only: false,
            process_enabled: true,
        }),
        "validation-offline"
        | "godot-probe-offline"
        | "godot-recovery-probe-offline"
        | "godot-diagnostics-offline"
        | "godot-lsp-local" => Some(SandboxProfile {
            id: match profile_id {
                "validation-offline" => "validation-offline",
                "godot-probe-offline" => "godot-probe-offline",
                "godot-recovery-probe-offline" => {
                    "godot-recovery-probe-offline"
                }
                "godot-diagnostics-offline" => "godot-diagnostics-offline",
                _ => "godot-lsp-local",
            },
            workspace_access_read_only: true,
            process_enabled: true,
        }),
        _ => None,
    }
}

/// The user-facing `inspect` default policy.
const fn inspect_policy_entries() -> [(&'static str, PermissionRule); 14] {
    [
        ("workspace.read", Allow),
        ("workspace.write", Deny),
        ("git.inspect", Allow),
        ("godot.inspect", Allow),
        ("godot.probe_project", Ask),
        ("godot.api", Allow),
        ("godot.diagnose", Ask),
        ("godot.lsp", Ask),
        ("godot.development", Allow),
        ("process.execute", Deny),
        ("network.outbound", Deny),
        ("reference.inspect", Allow),
        ("research.fetch", Deny),
        ("self.inspect", Allow),
    ]
}

/// The user-facing `develop-offline` default policy.
const fn develop_offline_policy_entries()
-> [(&'static str, PermissionRule); 14] {
    [
        ("workspace.read", Allow),
        ("workspace.write", Ask),
        ("git.inspect", Allow),
        ("godot.inspect", Allow),
        ("godot.probe_project", Ask),
        ("godot.api", Allow),
        ("godot.diagnose", Ask),
        ("godot.lsp", Ask),
        ("godot.development", Allow),
        ("process.execute", Ask),
        ("network.outbound", Deny),
        ("reference.inspect", Allow),
        ("research.fetch", Deny),
        ("self.inspect", Allow),
    ]
}

/// The internal execution profiles' default policies (never
/// user-selectable): unconditional Godot workflow allows are denied and
/// only inspection stays permitted.
const fn internal_policy_entries() -> [(&'static str, PermissionRule); 14] {
    [
        ("workspace.read", Allow),
        ("workspace.write", Deny),
        ("git.inspect", Allow),
        ("godot.inspect", Allow),
        ("godot.probe_project", Deny),
        ("godot.api", Deny),
        ("godot.diagnose", Deny),
        ("godot.lsp", Deny),
        ("godot.development", Deny),
        ("process.execute", Ask),
        ("network.outbound", Deny),
        ("reference.inspect", Allow),
        ("research.fetch", Deny),
        ("self.inspect", Allow),
    ]
}

/// The built-in default policy for a profile id.
pub fn create_default_policy(profile_id: &str) -> Option<CapabilityPolicy> {
    let entries = match profile_id {
        "inspect" => inspect_policy_entries(),
        "develop-offline" => develop_offline_policy_entries(),
        "validation-offline"
        | "godot-probe-offline"
        | "godot-recovery-probe-offline"
        | "godot-diagnostics-offline"
        | "godot-lsp-local" => internal_policy_entries(),
        _ => return None,
    };
    Some(policy_from(entries))
}

/// The outcome of one permission evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Execution is permitted without approval.
    Allow,
    /// Execution requires reviewable approval, with the exact reason.
    Ask {
        /// The exact reference reason string.
        reason: String,
    },
    /// Execution is denied, with the exact reason.
    Deny {
        /// The exact reference reason string.
        reason: String,
    },
}

/// Evaluate one capability under a policy and profile, mirroring the
/// reference evaluation order exactly: missing rule fails closed, an
/// explicit deny wins, then profile constraints, then ask, then allow.
pub fn evaluate_permission(
    capability: &str,
    policy: &CapabilityPolicy,
    profile: &SandboxProfile,
) -> PermissionDecision {
    let Some(rule) = policy.rule(capability) else {
        return PermissionDecision::Deny {
            reason: format!(
                "No permission rule is defined for {capability}; failing closed."
            ),
        };
    };
    if rule == Deny {
        return PermissionDecision::Deny {
            reason: format!("Policy denies {capability}."),
        };
    }
    if let Some(reason) = profile_constraint_issue(capability, profile) {
        return PermissionDecision::Deny { reason };
    }
    if rule == Ask {
        return PermissionDecision::Ask {
            reason: format!("Policy requires approval for {capability}."),
        };
    }
    PermissionDecision::Allow
}

fn profile_constraint_issue(
    capability: &str,
    profile: &SandboxProfile,
) -> Option<String> {
    match capability {
        "process.execute" if !profile.process_enabled => Some(format!(
            "Profile {} does not enable process execution.",
            profile.id
        )),
        "network.outbound" => Some(
            "No built-in sandbox profile enables outbound network access."
                .to_string(),
        ),
        "workspace.write" if profile.workspace_access_read_only => {
            Some(format!(
                "Profile {} provides read-only workspace access.",
                profile.id
            ))
        }
        _ => None,
    }
}

/// Protected behavioral-configuration classification, re-exported through
/// the authority surface so every consumer classifies identically.
pub fn is_protected_behavioral_config(workspace_relative_path: &str) -> bool {
    is_protected_behavioral_config_path(workspace_relative_path)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policies_are_total_over_the_capability_vocabulary() {
        for id in SANDBOX_PROFILE_IDS {
            let policy = create_default_policy(id).expect("built-in profile");
            assert_eq!(policy.ordered_rules().len(), CAPABILITY_IDS.len());
        }
        assert!(create_default_policy("not-a-profile").is_none());
    }

    #[test]
    fn evaluation_matches_the_reference_reasons_exactly() {
        let develop =
            get_built_in_profile("develop-offline").expect("built-in");
        let inspect = get_built_in_profile("inspect").expect("built-in");
        assert_eq!(
            evaluate_permission(
                "workspace.read",
                &create_default_policy("develop-offline").expect("b"),
                &develop
            ),
            PermissionDecision::Allow
        );
        assert_eq!(
            evaluate_permission(
                "research.fetch",
                &create_default_policy("inspect").expect("b"),
                &inspect
            ),
            PermissionDecision::Deny {
                reason: "Policy denies research.fetch.".to_string()
            }
        );
        assert_eq!(
            evaluate_permission(
                "godot.diagnose",
                &create_default_policy("develop-offline").expect("b"),
                &develop
            ),
            PermissionDecision::Ask {
                reason: "Policy requires approval for godot.diagnose."
                    .to_string()
            }
        );
        // Permitting rule plus a process-disabled profile: constraint denies.
        assert_eq!(
            evaluate_permission(
                "process.execute",
                &create_default_policy("develop-offline").expect("b"),
                &inspect
            ),
            PermissionDecision::Deny {
                reason: "Profile inspect does not enable process execution."
                    .to_string()
            }
        );
        assert_eq!(
            evaluate_permission(
                "workspace.write",
                &create_default_policy("develop-offline").expect("b"),
                &inspect
            ),
            PermissionDecision::Deny {
                reason: "Profile inspect provides read-only workspace access."
                    .to_string()
            }
        );
    }

    #[test]
    fn a_missing_rule_fails_closed() {
        let rebuilt: Vec<(&'static str, PermissionRule)> =
            create_default_policy("inspect")
                .expect("built-in")
                .ordered_rules()
                .into_iter()
                .filter(|(capability, _)| *capability != "self.inspect")
                .collect();
        let policy = CapabilityPolicy::from_entries(rebuilt);
        assert_eq!(
            evaluate_permission("self.inspect", &policy, &get_built_in_profile("inspect").expect("b")),
            PermissionDecision::Deny {
                reason: "No permission rule is defined for self.inspect; failing closed.".to_string()
            }
        );
    }

    #[test]
    fn behavioral_config_classification_is_case_insensitive_and_depth_free() {
        assert!(is_protected_behavioral_config("AGENTS.md"));
        assert!(is_protected_behavioral_config("agents.md"));
        assert!(is_protected_behavioral_config("deep/nested/dir/AGENTS.md"));
        assert!(is_protected_behavioral_config(".siralos/config.json"));
        assert!(is_protected_behavioral_config("a\\b\\AGENTS.md"));
        assert!(!is_protected_behavioral_config("src/main.ts"));
        assert!(!is_protected_behavioral_config("AGENTS.md.bak"));
        assert!(!is_protected_behavioral_config(""));
    }
}
