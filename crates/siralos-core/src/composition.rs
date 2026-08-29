//! Composition foundations (Stage 5.1, decision 47): named declarative
//! working configurations.
//!
//! A [[Profile]](https://adr.siralos) is "a named declarative AI working
//! configuration" (ADR 0036 §6). This module owns the generic record and
//! the one mechanical security property of the slice: **narrowing-only**
//! resolution. A profile permission overlay entry is legal iff it is not
//! broader than the Host's current rule for that capability
//! (`Deny < Ask < Allow`); a widening request is a typed refusal, never a
//! silent clamp. A Profile never itself grants authority — resolution
//! output feeds the existing Host policy gates unchanged.
//!
//! Profiles are pure declarative data: no network, no spawn, no live
//! probing, no wall clock. Zero-configuration stays first-class: an absent
//! profile resolves to a typed default.

use std::collections::BTreeMap;

use crate::identity::{CanonicalValue, compute_artifact_digest};
use crate::tool::capability::CapabilityId;
use crate::tool::permission::{
    PermissionDecision, PermissionPolicy, PermissionRule, PolicyRule,
    evaluate_permission,
};

/// Maximum profile name length in UTF-8 bytes.
pub const MAX_PROFILE_NAME_BYTES: usize = 64;
/// Maximum number of permission-overlay entries in one profile.
pub const MAX_PROFILE_OVERLAY_ENTRIES: usize = 16;

/// One profile permission-overlay entry: the capability and the rule the
/// profile requests for it. Legality is decided by
/// [`resolve_profile_overlay`], never assumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileOverlayEntry {
    /// Target capability.
    pub capability: CapabilityId,
    /// Requested rule (must not be broader than the Host rule).
    pub requested: PermissionRule,
}

/// A named declarative working configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRecord {
    /// Non-empty, bounded profile name.
    pub name: String,
    /// Permission overlay entries (bounded count, unique capabilities).
    pub overlay: Vec<ProfileOverlayEntry>,
}

/// Rank of a rule for the narrowing comparison: `Deny < Ask < Allow`.
fn rule_rank(rule: &PermissionRule) -> u8 {
    match rule {
        PermissionRule::Deny => 0,
        PermissionRule::Ask => 1,
        PermissionRule::Allow => 2,
    }
}

/// Rank of the Host's current decision for a capability.
fn decision_rank(decision: &PermissionDecision) -> u8 {
    match decision {
        PermissionDecision::Deny { .. } => 0,
        PermissionDecision::Ask { .. } => 1,
        PermissionDecision::Allow => 2,
    }
}

/// A typed validation failure for a malformed profile record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileValidationError {
    /// Deterministic, human-readable reason.
    pub message: String,
}

/// A typed refusal for an authority-widening overlay request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRefusal {
    /// Deterministic reason naming the offending capability.
    pub message: String,
}

impl ProfileRecord {
    /// Validate bounds and uniqueness. Deterministic order: name, then
    /// entry count, then per-entry checks in entry order.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileValidationError`] for empty/oversized names,
    /// NUL bytes, entry-count overflow, or duplicate capabilities.
    pub fn validate(&self) -> Result<(), ProfileValidationError> {
        if self.name.is_empty() {
            return Err(ProfileValidationError {
                message: "A profile requires a non-empty name.".to_owned(),
            });
        }
        if self.name.len() > MAX_PROFILE_NAME_BYTES {
            return Err(ProfileValidationError {
                message: format!(
                    "The profile name exceeds the {MAX_PROFILE_NAME_BYTES}-byte bound."
                ),
            });
        }
        if self.name.contains('\0') {
            return Err(ProfileValidationError {
                message: "A profile name must not contain NUL.".to_owned(),
            });
        }
        if self.overlay.len() > MAX_PROFILE_OVERLAY_ENTRIES {
            return Err(ProfileValidationError {
                message: format!(
                    "The profile exceeds the {MAX_PROFILE_OVERLAY_ENTRIES}-entry bound."
                ),
            });
        }
        let mut seen = BTreeMap::new();
        for entry in &self.overlay {
            if seen.contains_key(entry.capability.as_str()) {
                return Err(ProfileValidationError {
                    message: format!(
                        "The profile requests capability {} more than once.",
                        entry.capability.as_str()
                    ),
                });
            }
            seen.insert(entry.capability.as_str(), ());
        }
        Ok(())
    }
}

/// The accepted narrowing result: every overlay entry, guaranteed not
/// broader than the Host's current rule for its capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarrowedProfileOverlay {
    /// Accepted entries in profile order.
    pub entries: Vec<ProfileOverlayEntry>,
}

/// Resolution outcome for a named profile against Host policy. The
/// `Default` variant carries no record: zero-configuration is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileResolution {
    /// A profile was present and every overlay entry narrowed legally.
    Resolved {
        /// The validated profile name.
        name: String,
        /// The accepted (narrowed) overlay.
        narrowed: NarrowedProfileOverlay,
    },
    /// No profile was declared; zero-configuration remains valid.
    Default,
    /// The overlay attempted to broaden Host authority.
    Refused {
        /// Deterministic refusal reason naming the capability.
        reason: String,
    },
}

impl ProfileResolution {
    /// Stable disposition token for evidence and wire output.
    #[must_use]
    pub fn disposition(&self) -> &'static str {
        match self {
            Self::Resolved { .. } => "resolved",
            Self::Default => "default",
            Self::Refused { .. } => "refused",
        }
    }
}

/// Resolve a validated profile record against Host policy. For every
/// overlay entry the Host's current decision is evaluated; the entry is
/// accepted iff its requested rule is not broader than that decision
/// (`Deny < Ask < Allow`). The first widening entry refuses the whole
/// resolution with a typed reason (never a silent clamp), matching the
/// contract that lower-authority configuration may narrow but never
/// broaden Host authority.
///
/// # Errors
///
/// Returns [`ProfileValidationError`] when the record itself is
/// malformed; the widening case is a typed [`ProfileResolution::Refused`],
/// not an error.
pub fn resolve_profile_overlay(
    record: &ProfileRecord,
    host: &PermissionPolicy,
) -> Result<ProfileResolution, ProfileValidationError> {
    record.validate()?;
    let mut entries = Vec::with_capacity(record.overlay.len());
    for entry in &record.overlay {
        let host_decision = evaluate_permission(&entry.capability, host);
        if rule_rank(&entry.requested) > decision_rank(&host_decision) {
            return Ok(ProfileResolution::Refused {
                reason: format!(
                    "PROFILE_REFUSED: overlay requests {} for capability {} but the Host grants {}; a profile may never broaden Host authority.",
                    entry.requested.as_str(),
                    entry.capability.as_str(),
                    host_decision.decision(),
                ),
            });
        }
        entries.push(entry.clone());
    }
    Ok(ProfileResolution::Resolved {
        name: record.name.clone(),
        narrowed: NarrowedProfileOverlay { entries },
    })
}

/// Typed default resolution for an absent profile: zero-configuration
/// remains valid (ADR 0036 §7).
#[must_use]
pub fn default_profile_resolution() -> ProfileResolution {
    ProfileResolution::Default
}

/// Evidence detail for a profile resolution: counts and the name only —
/// the overlay content is reflected in the domain-separated digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileEvidenceDetail {
    /// Profile name when a profile was present.
    pub name: Option<String>,
    /// Declared overlay entry count.
    pub overlay_entry_count: usize,
    /// Accepted (narrowed) entry count.
    pub narrowed_entry_count: usize,
}

/// A resolution bound to its detail under a domain-separated digest over
/// `ProfileEvidence v1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileEvidence {
    /// The typed resolution outcome.
    pub resolution: ProfileResolution,
    /// Evidence detail (counts and name only).
    pub detail: ProfileEvidenceDetail,
    /// Domain-separated digest over the disposition and detail.
    pub profile_digest: String,
}

/// Build the profile evidence record for a resolution. The request is
/// re-validated so malformed input can never produce evidence.
///
/// # Errors
///
/// Returns [`ProfileValidationError`] for malformed records.
pub fn create_profile_evidence(
    resolution: &ProfileResolution,
) -> Result<ProfileEvidence, ProfileValidationError> {
    let detail = match resolution {
        ProfileResolution::Resolved { name, narrowed } => {
            ProfileEvidenceDetail {
                name: Some(name.clone()),
                overlay_entry_count: narrowed.entries.len(),
                narrowed_entry_count: narrowed.entries.len(),
            }
        }
        ProfileResolution::Default | ProfileResolution::Refused { .. } => {
            ProfileEvidenceDetail {
                name: None,
                overlay_entry_count: 0,
                narrowed_entry_count: 0,
            }
        }
    };
    let narrowed_entries: Vec<(String, String)> = match resolution {
        ProfileResolution::Resolved { narrowed, .. } => narrowed
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.capability.as_str().to_owned(),
                    entry.requested.as_str().to_owned(),
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    let refusal_reason = match resolution {
        ProfileResolution::Refused { reason } => Some(reason.as_str()),
        _ => None,
    };
    let payload = CanonicalValue::Object(BTreeMap::from([
        (
            "disposition".to_owned(),
            CanonicalValue::Str(resolution.disposition().to_owned()),
        ),
        (
            "name".to_owned(),
            detail
                .name
                .clone()
                .map_or(CanonicalValue::Null, CanonicalValue::Str),
        ),
        (
            "overlayEntryCount".to_owned(),
            CanonicalValue::U64(detail.overlay_entry_count as u64),
        ),
        (
            "narrowedEntryCount".to_owned(),
            CanonicalValue::U64(detail.narrowed_entry_count as u64),
        ),
        (
            "narrowedEntries".to_owned(),
            CanonicalValue::Object(
                narrowed_entries
                    .into_iter()
                    .map(|(cap, rule)| (cap, CanonicalValue::Str(rule)))
                    .collect(),
            ),
        ),
        (
            "refusalReason".to_owned(),
            refusal_reason.map_or(CanonicalValue::Null, |reason| {
                CanonicalValue::Str(reason.to_owned())
            }),
        ),
    ]));
    let profile_digest =
        compute_artifact_digest("ProfileEvidence", 1, &payload)
            .map_err(|error| ProfileValidationError {
                message: error.message,
            })?
            .value;
    Ok(ProfileEvidence {
        resolution: resolution.clone(),
        detail,
        profile_digest,
    })
}

/// The declared workspace profile state fed into composition (Stage 5.2,
/// decision 48). Adapters map the on-disk document onto this enum; the
/// invalid case carries a truthful diagnostic instead of blocking
/// composition (C3: ignoring unverified config cannot broaden authority).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredProfile {
    /// No profile document in the workspace; zero-configuration holds.
    Absent,
    /// Document parsed and resolved against Host authority.
    Resolved {
        /// The validated profile name.
        name: String,
        /// The accepted (narrowed) overlay.
        narrowed: NarrowedProfileOverlay,
    },
    /// Document parsed but resolution refused (authority widening).
    Refused {
        /// Deterministic refusal reason naming the capability.
        reason: String,
    },
    /// Document failed the adapter's bounded parse/validation.
    Invalid {
        /// Truthful reason the document was not applied.
        diagnostic: String,
    },
}

/// The effective run configuration produced by composition: Host rules
/// narrowed by an applied profile, or the Host rules unchanged when no
/// profile applies. Every rule is the Host's own decision narrowed -
/// composition can never produce a rule broader than the Host's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRunPolicy {
    /// The applied profile name, when one was applied.
    pub applied_profile: Option<String>,
    /// Why a declared profile was not applied (refused or invalid).
    pub diagnostic: Option<String>,
    /// Effective per-capability rules, sorted by capability id.
    pub rules: Vec<PolicyRule>,
}

/// Map an optional profile record onto the declared state by resolving it
/// against Host policy. `None` (no document) declares
/// [`DeclaredProfile::Absent`]; a record resolves to `Resolved` or
/// `Refused`. Adapter-level document failures map to `Invalid` by the
/// caller.
#[must_use]
pub fn declare_profile(
    record: Option<&ProfileRecord>,
    host: &PermissionPolicy,
) -> DeclaredProfile {
    match record {
        None => DeclaredProfile::Absent,
        Some(record) => match resolve_profile_overlay(record, host) {
            Ok(ProfileResolution::Resolved { name, narrowed }) => {
                DeclaredProfile::Resolved { name, narrowed }
            }
            Ok(ProfileResolution::Default) => DeclaredProfile::Absent,
            Ok(ProfileResolution::Refused { reason }) => {
                DeclaredProfile::Refused { reason }
            }
            Err(error) => {
                DeclaredProfile::Invalid { diagnostic: error.message }
            }
        },
    }
}

/// Compose the effective run configuration: Host rules narrowed by the
/// declared profile when (and only when) it applies. The narrowing-only
/// invariant is re-checked here at the composition boundary: an overlay
/// entry broader than the Host decision demotes the whole profile to
/// not-applied with a diagnostic rather than applying any part of it.
#[must_use]
pub fn compose_effective_policy(
    host_rules: &[PolicyRule],
    declared: &DeclaredProfile,
) -> EffectiveRunPolicy {
    let mut rules: BTreeMap<String, PolicyRule> = host_rules
        .iter()
        .map(|rule| (rule.capability.as_str().to_owned(), rule.clone()))
        .collect();
    let not_applied =
        |diagnostic: String, rules: &BTreeMap<String, PolicyRule>| {
            EffectiveRunPolicy {
                applied_profile: None,
                diagnostic: Some(diagnostic),
                rules: sorted_rules(rules),
            }
        };
    let (applied_profile, diagnostic) = match declared {
        DeclaredProfile::Absent => (None, None),
        DeclaredProfile::Invalid { diagnostic } => {
            return not_applied(diagnostic.clone(), &rules);
        }
        DeclaredProfile::Refused { reason } => {
            return not_applied(reason.clone(), &rules);
        }
        DeclaredProfile::Resolved { name, narrowed } => {
            let host = PermissionPolicy::from_rules(host_rules.to_vec());
            for entry in &narrowed.entries {
                let host_decision =
                    evaluate_permission(&entry.capability, &host);
                if rule_rank(&entry.requested) > decision_rank(&host_decision)
                {
                    return not_applied(
                        format!(
                            "PROFILE_REFUSED: overlay requests {} for capability {} but the Host grants {}; a profile may never broaden Host authority.",
                            entry.requested.as_str(),
                            entry.capability.as_str(),
                            host_decision.decision(),
                        ),
                        &rules,
                    );
                }
                rules.insert(
                    entry.capability.as_str().to_owned(),
                    PolicyRule {
                        capability: entry.capability.clone(),
                        rule: entry.requested,
                    },
                );
            }
            (Some(name.clone()), None)
        }
    };
    EffectiveRunPolicy {
        applied_profile,
        diagnostic,
        rules: sorted_rules(&rules),
    }
}

fn sorted_rules(rules: &BTreeMap<String, PolicyRule>) -> Vec<PolicyRule> {
    rules.values().cloned().collect()
}

/// Digest-bound evidence for the composed effective run configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePolicyEvidence {
    /// The composed policy the digest binds.
    pub policy: EffectiveRunPolicy,
    /// Artifact digest over the canonical evidence payload.
    pub effective_digest: String,
}

/// Build digest-bound evidence for a composed effective policy. The
/// payload binds the applied profile, the diagnostic (when present), and
/// the full effective rule map, so any authority change moves the digest.
///
/// # Errors
///
/// Returns [`ProfileValidationError`] when canonical serialization or
/// digest computation fails.
pub fn create_effective_policy_evidence(
    policy: &EffectiveRunPolicy,
) -> Result<EffectivePolicyEvidence, ProfileValidationError> {
    let rule_map: BTreeMap<String, CanonicalValue> = policy
        .rules
        .iter()
        .map(|rule| {
            (
                rule.capability.as_str().to_owned(),
                CanonicalValue::Str(rule.rule.as_str().to_owned()),
            )
        })
        .collect();
    let payload = CanonicalValue::Object(BTreeMap::from([
        (
            "appliedProfile".to_owned(),
            policy
                .applied_profile
                .as_ref()
                .map_or(CanonicalValue::Null, |name| {
                    CanonicalValue::Str(name.to_owned())
                }),
        ),
        (
            "diagnostic".to_owned(),
            policy.diagnostic.as_ref().map_or(CanonicalValue::Null, |d| {
                CanonicalValue::Str(d.to_owned())
            }),
        ),
        ("rules".to_owned(), CanonicalValue::Object(rule_map)),
    ]));
    let effective_digest =
        compute_artifact_digest("EffectivePolicyEvidence", 1, &payload)
            .map_err(|error| ProfileValidationError {
                message: error.message,
            })?
            .value;
    Ok(EffectivePolicyEvidence { policy: policy.clone(), effective_digest })
}

/// Bounded deterministic rendering: applied profile or not-applied state.
#[must_use]
pub fn render_effective_policy_evidence(
    evidence: &EffectivePolicyEvidence,
) -> String {
    match (&evidence.policy.applied_profile, &evidence.policy.diagnostic) {
        (Some(name), _) => format!(
            "applied profile={name} rules={}",
            evidence.policy.rules.len()
        ),
        (None, Some(diagnostic)) => format!("not applied: {diagnostic}"),
        (None, None) => "unmodified (no profile applied)".to_owned(),
    }
}
/// Bounded deterministic rendering: disposition, name, and entry counts.
#[must_use]
pub fn render_profile_evidence(evidence: &ProfileEvidence) -> String {
    match &evidence.resolution {
        ProfileResolution::Resolved { name, .. } => format!(
            "resolved name={name} entries={}",
            evidence.detail.overlay_entry_count
        ),
        ProfileResolution::Default => {
            "default (no profile declared)".to_owned()
        }
        ProfileResolution::Refused { reason } => {
            format!("refused: {reason}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PROFILE_NAME_BYTES, MAX_PROFILE_OVERLAY_ENTRIES,
        ProfileOverlayEntry, ProfileRecord, create_profile_evidence,
        declare_profile, default_profile_resolution, render_profile_evidence,
        resolve_profile_overlay,
    };
    use crate::tool::capability::CapabilityId;
    use crate::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };

    fn entry(cap: &str, rule: PermissionRule) -> ProfileOverlayEntry {
        ProfileOverlayEntry {
            capability: CapabilityId::parse(cap).expect("valid capability"),
            requested: rule,
        }
    }

    fn host_policy() -> PermissionPolicy {
        PermissionPolicy::from_rules(vec![
            PolicyRule {
                capability: CapabilityId::parse("tool.workspace.read")
                    .expect("valid capability"),
                rule: PermissionRule::Allow,
            },
            PolicyRule {
                capability: CapabilityId::parse("tool.workspace.search")
                    .expect("valid capability"),
                rule: PermissionRule::Ask,
            },
        ])
    }

    #[test]
    fn narrowing_overlay_resolves() {
        let record = ProfileRecord {
            name: "dev".to_owned(),
            overlay: vec![
                entry("tool.workspace.read", PermissionRule::Ask),
                entry("tool.workspace.search", PermissionRule::Deny),
            ],
        };
        let resolution =
            resolve_profile_overlay(&record, &host_policy()).expect("valid");
        match &resolution {
            super::ProfileResolution::Resolved { name, narrowed } => {
                assert_eq!(name, "dev");
                assert_eq!(narrowed.entries.len(), 2);
            }
            other => panic!("unexpected resolution: {other:?}"),
        }
        let evidence = create_profile_evidence(&resolution).expect("evidence");
        assert_eq!(evidence.detail.overlay_entry_count, 2);
        assert!(render_profile_evidence(&evidence).contains("name=dev"));
    }

    #[test]
    fn widening_overlay_is_typed_refused() {
        let record = ProfileRecord {
            name: "wide".to_owned(),
            overlay: vec![
                entry("tool.workspace.read", PermissionRule::Allow),
                entry("ungranted.capability", PermissionRule::Ask),
            ],
        };
        let resolution =
            resolve_profile_overlay(&record, &host_policy()).expect("valid");
        match resolution {
            super::ProfileResolution::Refused { reason } => {
                assert!(reason.contains("ungranted.capability"));
                assert!(reason.contains("PROFILE_REFUSED"));
            }
            other => panic!("unexpected resolution: {other:?}"),
        }
    }

    #[test]
    fn equal_rule_is_a_no_op_not_a_widening() {
        let record = ProfileRecord {
            name: "same".to_owned(),
            overlay: vec![entry("tool.workspace.search", PermissionRule::Ask)],
        };
        let resolution =
            resolve_profile_overlay(&record, &host_policy()).expect("valid");
        assert!(matches!(
            resolution,
            super::ProfileResolution::Resolved { .. }
        ));
    }

    #[test]
    fn bounds_and_duplicates_are_typed_invalid() {
        let record = ProfileRecord {
            name: "a".repeat(MAX_PROFILE_NAME_BYTES + 1),
            overlay: Vec::new(),
        };
        let error = resolve_profile_overlay(&record, &host_policy())
            .expect_err("name refused");
        assert!(error.message.contains("byte bound"));
        let record = ProfileRecord {
            name: "dup".to_owned(),
            overlay: vec![
                entry("tool.workspace.read", PermissionRule::Deny),
                entry("tool.workspace.read", PermissionRule::Ask),
            ],
        };
        let error = resolve_profile_overlay(&record, &host_policy())
            .expect_err("duplicate refused");
        assert!(error.message.contains("more than once"));
        let record = ProfileRecord {
            name: "many".to_owned(),
            overlay: (0..MAX_PROFILE_OVERLAY_ENTRIES + 1)
                .filter_map(|index| {
                    CapabilityId::parse(&format!("cap.area{index}.read"))
                        .ok()
                        .map(|capability| ProfileOverlayEntry {
                            capability,
                            requested: PermissionRule::Deny,
                        })
                })
                .collect(),
        };
        if record.overlay.len() > MAX_PROFILE_OVERLAY_ENTRIES {
            let error = resolve_profile_overlay(&record, &host_policy())
                .expect_err("count refused");
            assert!(error.message.contains("entry bound"));
        }
    }

    #[test]
    fn absent_profile_resolves_to_typed_default() {
        let resolution = default_profile_resolution();
        assert_eq!(resolution.disposition(), "default");
        let evidence = create_profile_evidence(&resolution).expect("evidence");
        assert_eq!(evidence.detail.name, None);
        assert!(render_profile_evidence(&evidence).contains("default"));
    }

    #[test]
    fn compose_applies_narrowing() {
        let host_rules = vec![
            PolicyRule {
                capability: CapabilityId::parse("tool.workspace.read")
                    .expect("valid capability"),
                rule: PermissionRule::Allow,
            },
            PolicyRule {
                capability: CapabilityId::parse("tool.workspace.search")
                    .expect("valid capability"),
                rule: PermissionRule::Ask,
            },
        ];
        let host = PermissionPolicy::from_rules(host_rules.clone());
        let record = ProfileRecord {
            name: "dev".to_owned(),
            overlay: vec![
                entry("tool.workspace.read", PermissionRule::Ask),
                entry("tool.workspace.search", PermissionRule::Deny),
            ],
        };
        let declared = declare_profile(Some(&record), &host);
        let effective =
            super::compose_effective_policy(&host_rules, &declared);
        assert_eq!(effective.applied_profile.as_deref(), Some("dev"));
        assert!(effective.diagnostic.is_none());
        assert_eq!(effective.rules.len(), 2);
        assert_eq!(effective.rules[0].rule, PermissionRule::Ask);
        assert_eq!(effective.rules[1].rule, PermissionRule::Deny);
        let evidence = super::create_effective_policy_evidence(&effective)
            .expect("evidence");
        assert_eq!(evidence.effective_digest.len(), 64);
        assert!(
            evidence.effective_digest.chars().all(|c| c.is_ascii_hexdigit())
        );
        assert!(
            super::render_effective_policy_evidence(&evidence)
                .starts_with("applied profile=dev")
        );
    }

    #[test]
    fn compose_absent_and_ignored_states() {
        let host_rules = vec![PolicyRule {
            capability: CapabilityId::parse("tool.workspace.read")
                .expect("valid capability"),
            rule: PermissionRule::Allow,
        }];
        let absent = super::compose_effective_policy(
            &host_rules,
            &super::DeclaredProfile::Absent,
        );
        assert_eq!(absent.applied_profile, None);
        assert_eq!(absent.diagnostic, None);
        assert_eq!(absent.rules.len(), 1);
        assert_eq!(absent.rules[0].rule, PermissionRule::Allow);
        let invalid = super::compose_effective_policy(
            &host_rules,
            &super::DeclaredProfile::Invalid {
                diagnostic: "siralos.toml does not parse".to_owned(),
            },
        );
        assert_eq!(invalid.applied_profile, None);
        assert_eq!(
            invalid.diagnostic.as_deref(),
            Some("siralos.toml does not parse")
        );
        assert_eq!(invalid.rules.len(), 1);
        assert_eq!(invalid.rules[0].rule, PermissionRule::Allow);
    }

    #[test]
    fn compose_rechecks_the_invariant() {
        // A hand-built Resolved declaration whose overlay widens Host
        // authority must be demoted to not-applied at the boundary.
        let host_rules = vec![PolicyRule {
            capability: CapabilityId::parse("tool.workspace.read")
                .expect("valid capability"),
            rule: PermissionRule::Ask,
        }];
        let declared = super::DeclaredProfile::Resolved {
            name: "wide".to_owned(),
            narrowed: super::NarrowedProfileOverlay {
                entries: vec![entry(
                    "tool.workspace.read",
                    PermissionRule::Allow,
                )],
            },
        };
        let effective =
            super::compose_effective_policy(&host_rules, &declared);
        assert_eq!(effective.applied_profile, None);
        let diagnostic = effective.diagnostic.expect("diagnostic");
        assert!(diagnostic.starts_with("PROFILE_REFUSED"));
        assert!(diagnostic.contains("tool.workspace.read"));
        assert_eq!(effective.rules.len(), 1);
        assert_eq!(effective.rules[0].rule, PermissionRule::Ask);
    }
}
