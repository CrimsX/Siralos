//! Domain-neutral Tool-loop permission evaluation.
//!
//! The TypeScript reference evaluates `CapabilityPolicy` rules through
//! `evaluatePermission` and may add profile constraints for concrete
//! capabilities. The R7.2 slice owns only the generic decision layer:
//! a Host-supplied rule per opaque [`CapabilityId`] and the exact
//! allow/ask/deny decision reasons. It ports no TypeScript
//! policy/profile object graph and creates no second authority system;
//! optional-domain capabilities remain opaque identifiers and can be
//! gated by the same rule path with zero Core changes.

use std::collections::BTreeMap;

use crate::tool::capability::CapabilityId;

/// One Host permission rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRule {
    /// Execution is permitted without approval.
    Allow,
    /// Execution requires a reviewable preparation protocol.
    Ask,
    /// Execution is denied.
    Deny,
}

impl PermissionRule {
    /// The stable wire vocabulary.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }

    /// Parse the stable wire vocabulary.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "allow" => Some(Self::Allow),
            "ask" => Some(Self::Ask),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// One capability-to-rule mapping for policy construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRule {
    /// The opaque capability identifier this rule targets.
    pub capability: CapabilityId,
    /// The Host decision for that capability.
    pub rule: PermissionRule,
}

/// Immutable ordered Host permission policy.
///
/// Rules are stored in a `BTreeMap` (deterministic capability order);
/// no map iteration is observable and capability equality/order is
/// explicit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionPolicy {
    rules: BTreeMap<CapabilityId, PermissionRule>,
}

impl PermissionPolicy {
    /// Build a policy from ordered rules.
    ///
    /// A later rule for the same capability replaces the earlier rule;
    /// this mirrors the single-slot record semantics of the reference
    /// policy and keeps construction total for harness fixtures.
    pub fn from_rules(rules: impl IntoIterator<Item = PolicyRule>) -> Self {
        let mut table = BTreeMap::new();
        for PolicyRule { capability, rule } in rules {
            table.insert(capability, rule);
        }
        Self { rules: table }
    }

    /// The rule for a capability, when one is defined.
    pub fn rule(&self, capability: &CapabilityId) -> Option<PermissionRule> {
        self.rules.get(capability).copied()
    }

    /// The number of rules in the policy.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the policy has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Ordered iteration over the rules (capability id order).
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = (&CapabilityId, PermissionRule)> {
        self.rules.iter().map(|(capability, rule)| (capability, *rule))
    }
}

/// One deterministic permission decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// The call may proceed to execution.
    Allow,
    /// Approval would be required; plain R7.2 Tools have no reviewable
    /// preparation protocol and are denied without execution by the
    /// application gate.
    Ask {
        /// The exact reference ask reason.
        reason: String,
    },
    /// The call is denied by policy.
    Deny {
        /// The exact reference denial reason.
        reason: String,
    },
}

impl PermissionDecision {
    /// The stable decision vocabulary.
    pub fn decision(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask { .. } => "ask",
            Self::Deny { .. } => "deny",
        }
    }

    /// The decision reason for non-allow outcomes.
    pub fn reason(&self) -> &str {
        match self {
            Self::Allow => "",
            Self::Ask { reason } | Self::Deny { reason } => reason,
        }
    }
}

/// Evaluate one capability against Host policy immediately before
/// execution.
///
/// Precedence and reasons match the reference:
/// missing rule → deny/fail-closed; explicit deny → deny; otherwise the
/// declared rule (`ask` carries the approval reason, `allow` permits).
pub fn evaluate_permission(
    capability: &CapabilityId,
    policy: &PermissionPolicy,
) -> PermissionDecision {
    let Some(rule) = policy.rule(capability) else {
        return PermissionDecision::Deny {
            reason: format!(
                "No permission rule is defined for {capability}; failing closed."
            ),
        };
    };
    match rule {
        PermissionRule::Deny => PermissionDecision::Deny {
            reason: format!("Policy denies {capability}."),
        },
        PermissionRule::Ask => PermissionDecision::Ask {
            reason: format!("Policy requires approval for {capability}."),
        },
        PermissionRule::Allow => PermissionDecision::Allow,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PermissionDecision, PermissionPolicy, PermissionRule, PolicyRule,
        evaluate_permission,
    };
    use crate::tool::capability::CapabilityId;

    fn id(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    #[test]
    fn evaluates_allow_ask_deny_with_exact_reasons() {
        let capability = id("workspace.read");
        let allow = PermissionPolicy::from_rules([PolicyRule {
            capability: capability.clone(),
            rule: PermissionRule::Allow,
        }]);
        assert_eq!(
            evaluate_permission(&capability, &allow),
            PermissionDecision::Allow
        );

        let ask = PermissionPolicy::from_rules([PolicyRule {
            capability: capability.clone(),
            rule: PermissionRule::Ask,
        }]);
        assert_eq!(
            evaluate_permission(&capability, &ask),
            PermissionDecision::Ask {
                reason: "Policy requires approval for workspace.read."
                    .to_owned(),
            }
        );

        let deny = PermissionPolicy::from_rules([PolicyRule {
            capability: capability.clone(),
            rule: PermissionRule::Deny,
        }]);
        assert_eq!(
            evaluate_permission(&capability, &deny),
            PermissionDecision::Deny {
                reason: "Policy denies workspace.read.".to_owned(),
            }
        );
    }

    #[test]
    fn missing_rules_fail_closed_with_the_reference_reason() {
        assert_eq!(
            evaluate_permission(&id("future.domain"), &PermissionPolicy::default()),
            PermissionDecision::Deny {
                reason: "No permission rule is defined for future.domain; failing closed."
                    .to_owned(),
            }
        );
    }

    #[test]
    fn later_rules_replace_earlier_rules_for_one_capability() {
        let capability = id("a.b");
        let policy = PermissionPolicy::from_rules([
            PolicyRule {
                capability: capability.clone(),
                rule: PermissionRule::Allow,
            },
            PolicyRule {
                capability: capability.clone(),
                rule: PermissionRule::Deny,
            },
        ]);
        assert_eq!(
            evaluate_permission(&capability, &policy),
            PermissionDecision::Deny {
                reason: "Policy denies a.b.".to_owned(),
            }
        );
    }
}
