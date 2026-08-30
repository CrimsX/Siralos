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

use std::collections::{BTreeMap, BTreeSet};

pub mod lock;

pub use lock::{LockPluginIdentity, WorkspaceLock, create_workspace_lock};

use crate::context::{
    ContextControlOutcome, ContextPolicy, evaluate_context_policy,
};
use crate::identity::{CanonicalValue, compute_artifact_digest};
use crate::skills::{
    SkillCatalog, SkillResolution, SkillResolutionEvidence,
    create_skill_resolution_evidence, render_skill_resolution_evidence,
    resolve_profile_skills,
};
use crate::tool::capability::CapabilityId;
use crate::tool::permission::{
    PermissionDecision, PermissionPolicy, PermissionRule, PolicyRule,
    evaluate_permission,
};

/// Maximum profile name length in UTF-8 bytes.
pub const MAX_PROFILE_NAME_BYTES: usize = 64;
/// Maximum number of permission-overlay entries in one profile.
pub const MAX_PROFILE_OVERLAY_ENTRIES: usize = 16;
/// Maximum number of plugin ids in one profile selection.
pub const MAX_PROFILE_PLUGIN_ENTRIES: usize = 16;
/// Maximum profile plugin id length in UTF-8 bytes.
pub const MAX_PROFILE_PLUGIN_ID_BYTES: usize = 64;

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
    /// Optional plugin-selection filter: the profile may only narrow
    /// which Host-enabled plugins a session activates (Stage 5.5).
    pub plugins: Option<Vec<String>>,
    /// Optional context control: the profile may only narrow what the
    /// session claims about content — Live/Pinned/Frozen visibility
    /// (Stage 5.8).
    pub context: Option<ContextPolicy>,
    /// Optional opt-in skill selection: the profile may only bind
    /// declarative guidance from the workspace catalog — never authority
    /// (Stage 5.10).
    pub skills: Option<Vec<String>>,
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
        validate_plugin_selection(&self.plugins)
    }
}

/// The result of applying a profile's plugin selection to the
/// Host-enabled set (Stage 5.5): `activated = enabled ∩ selected`. The
/// intersection can only shrink the enabled set, so a profile can never
/// broaden activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSelection {
    /// Sorted ids that may activate.
    pub activated: Vec<String>,
    /// True when the profile declared no selection (nothing filtered).
    pub unfiltered: bool,
    /// Sorted selected ids that are not Host-enabled (diagnostics only).
    pub unknown: Vec<String>,
}

impl PluginSelection {
    /// Typed disposition: `unfiltered` or `narrowed`.
    pub fn disposition(&self) -> &'static str {
        if self.unfiltered { "unfiltered" } else { "narrowed" }
    }
}

/// Digest-bound evidence for one plugin-selection resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSelectionEvidence {
    /// The sorted activated ids.
    pub activated: Vec<String>,
    /// Typed disposition (`unfiltered` or `narrowed`).
    pub disposition: String,
    /// The bound selection digest.
    pub selection_digest: String,
    /// Sorted selected ids that are not Host-enabled.
    pub unknown: Vec<String>,
}

/// Create digest-bound evidence for a plugin selection over the single
/// artifact-digest primitive.
///
/// # Errors
///
/// Returns [`ProfileValidationError`] when the digest primitive fails.
pub fn create_plugin_selection_evidence(
    selection: &PluginSelection,
) -> Result<PluginSelectionEvidence, ProfileValidationError> {
    let activated: Vec<CanonicalValue> = selection
        .activated
        .iter()
        .map(|id| CanonicalValue::Str(id.clone()))
        .collect();
    let unknown: Vec<CanonicalValue> = selection
        .unknown
        .iter()
        .map(|id| CanonicalValue::Str(id.clone()))
        .collect();
    let payload = CanonicalValue::Object(BTreeMap::from([
        ("activated".to_owned(), CanonicalValue::Array(activated)),
        (
            "disposition".to_owned(),
            CanonicalValue::Str(selection.disposition().to_owned()),
        ),
        ("unknown".to_owned(), CanonicalValue::Array(unknown)),
    ]));
    let selection_digest =
        compute_artifact_digest("PluginSelectionEvidence", 1, &payload)
            .map_err(|error| ProfileValidationError {
                message: error.message,
            })?
            .value;
    Ok(PluginSelectionEvidence {
        activated: selection.activated.clone(),
        disposition: selection.disposition().to_owned(),
        selection_digest,
        unknown: selection.unknown.clone(),
    })
}

/// Deterministic report-safe rendering of plugin-selection evidence.
pub fn render_plugin_selection_evidence(
    evidence: &PluginSelectionEvidence,
) -> String {
    let base = format!(
        "{} plugins={}",
        evidence.disposition,
        evidence.activated.len()
    );
    if evidence.unknown.is_empty() {
        base
    } else {
        format!("{base} unknown={}", evidence.unknown.len())
    }
}
/// Apply a profile's plugin selection to the Host-enabled set.
/// `selected` is the profile's declared list (`None` = unfiltered).
pub fn select_profile_plugins(
    enabled: &[String],
    selected: Option<&[String]>,
) -> PluginSelection {
    let Some(selected) = selected else {
        let mut activated: Vec<String> = enabled.to_vec();
        activated.sort();
        return PluginSelection {
            activated,
            unfiltered: true,
            unknown: Vec::new(),
        };
    };
    let enabled_set: BTreeSet<&String> = enabled.iter().collect();
    let selected_set: BTreeSet<&String> = selected.iter().collect();
    let activated = enabled_set
        .intersection(&selected_set)
        .map(|id| (*id).clone())
        .collect::<Vec<String>>();
    let unknown = selected_set
        .difference(&enabled_set)
        .map(|id| (*id).clone())
        .collect::<Vec<String>>();
    PluginSelection { activated, unfiltered: false, unknown }
}
/// Validate the optional plugin-selection list: bounded count and id
/// length, non-empty ids, no duplicates.
fn validate_plugin_selection(
    plugins: &Option<Vec<String>>,
) -> Result<(), ProfileValidationError> {
    let Some(plugins) = plugins else {
        return Ok(());
    };
    if plugins.len() > MAX_PROFILE_PLUGIN_ENTRIES {
        return Err(ProfileValidationError {
            message: format!(
                "The profile exceeds the {MAX_PROFILE_PLUGIN_ENTRIES}-plugin bound."
            ),
        });
    }
    let mut seen = BTreeMap::new();
    for id in plugins {
        if id.is_empty() || id.len() > MAX_PROFILE_PLUGIN_ID_BYTES {
            return Err(ProfileValidationError {
                message: format!(
                    "A profile plugin id must be 1..={MAX_PROFILE_PLUGIN_ID_BYTES} bytes."
                ),
            });
        }
        if id.contains('\0') {
            return Err(ProfileValidationError {
                message: "A profile plugin id must not contain NUL."
                    .to_owned(),
            });
        }
        if seen.contains_key(id.as_str()) {
            return Err(ProfileValidationError {
                message: format!(
                    "The profile selects plugin id {id} more than once."
                ),
            });
        }
        seen.insert(id.as_str(), ());
    }
    Ok(())
}

/// The typed outcome of one per-id activation attempt through the
/// Stage 5.7 gate (Host authority first, then the profile filter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginActivationOutcome {
    /// The id may activate.
    Activated,
    /// Host-enabled, but outside the applied profile selection.
    RefusedFiltered,
    /// Not enabled by the Host; a profile can never enable (decision 39).
    RefusedNotEnabled,
}

impl PluginActivationOutcome {
    /// Typed decision string used by evidence and rendering.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Activated => "activated",
            Self::RefusedFiltered => "refused-filtered",
            Self::RefusedNotEnabled => "refused-not-enabled",
        }
    }
}

/// The result of gating one requested plugin id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginActivationDecision {
    /// The typed outcome.
    pub outcome: PluginActivationOutcome,
    /// Truthful, report-safe refusal reason; `None` when activated.
    pub reason: Option<String>,
}

/// Gate one requested plugin id: Host authority first (a profile can
/// never enable - decision 39), then the frozen Stage 5.5 profile
/// filter (activated = enabled ∩ selected).
#[must_use]
pub fn decide_plugin_activation(
    enabled: &[String],
    selected: Option<&[String]>,
    requested: &str,
) -> PluginActivationDecision {
    if !enabled.iter().any(|id| id == requested) {
        return PluginActivationDecision {
            outcome: PluginActivationOutcome::RefusedNotEnabled,
            reason: Some(format!(
                "the Host has not enabled {requested:?}; a profile can never enable"
            )),
        };
    }
    let Some(selected) = selected else {
        return PluginActivationDecision {
            outcome: PluginActivationOutcome::Activated,
            reason: None,
        };
    };
    if selected.iter().any(|id| id == requested) {
        PluginActivationDecision {
            outcome: PluginActivationOutcome::Activated,
            reason: None,
        }
    } else {
        PluginActivationDecision {
            outcome: PluginActivationOutcome::RefusedFiltered,
            reason: Some(format!(
                "the workspace profile does not select {requested:?}; it stays inactive"
            )),
        }
    }
}

/// Digest-bound evidence for one activation-gate decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginActivationEvidence {
    /// The typed decision string.
    pub decision: String,
    /// The bound decision digest.
    pub activation_digest: String,
    /// Truthful refusal reason; `None` when activated.
    pub reason: Option<String>,
    /// `"absent"` or `"present"`: whether a profile selection applies.
    pub selection: String,
}

/// Create digest-bound evidence for an activation-gate decision.
///
/// # Errors
///
/// Returns [`ProfileValidationError`] when the digest primitive fails.
pub fn create_plugin_activation_evidence(
    decision: &PluginActivationDecision,
    selection_present: bool,
) -> Result<PluginActivationEvidence, ProfileValidationError> {
    let payload = CanonicalValue::Object(BTreeMap::from([
        (
            "decision".to_owned(),
            CanonicalValue::Str(decision.outcome.as_str().to_owned()),
        ),
        (
            "reason".to_owned(),
            match &decision.reason {
                Some(reason) => CanonicalValue::Str(reason.clone()),
                None => CanonicalValue::Null,
            },
        ),
        (
            "selection".to_owned(),
            CanonicalValue::Str(
                if selection_present { "present" } else { "absent" }
                    .to_owned(),
            ),
        ),
    ]));
    let activation_digest =
        compute_artifact_digest("PluginActivationEvidence", 1, &payload)
            .map_err(|error| ProfileValidationError {
                message: error.message,
            })?
            .value;
    Ok(PluginActivationEvidence {
        decision: decision.outcome.as_str().to_owned(),
        activation_digest,
        reason: decision.reason.clone(),
        selection: if selection_present {
            "present".to_owned()
        } else {
            "absent".to_owned()
        },
    })
}

/// Deterministic report-safe rendering of activation-gate evidence.
#[must_use]
pub fn render_plugin_activation_evidence(
    evidence: &PluginActivationEvidence,
    requested: &str,
) -> String {
    let base = format!("{} {requested}", evidence.decision);
    match &evidence.reason {
        Some(reason) => format!("{base} ({reason})"),
        None => base,
    }
}

/// The composition-level context-control decision (Stage 5.8, decision
/// 54): the frozen Stage 5.3 evaluation applied to the session's content
/// claim, where an absent control is transparent (`Live`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextControlDecision {
    /// The typed outcome from the frozen 5.3 evaluation.
    pub outcome: ContextControlOutcome,
    /// Truthful, report-safe reason; `None` when the claim is fresh.
    pub reason: Option<String>,
}

/// Decide the session's context claim under the applied profile's
/// control. `None` (no control declared, or no applied profile) is
/// transparent: the claim behaves exactly as `Live`.
#[must_use]
pub fn decide_context_control(
    policy: Option<&ContextPolicy>,
    actual_digest: &str,
) -> ContextControlDecision {
    let outcome = match policy {
        None => evaluate_context_policy(&ContextPolicy::Live, actual_digest),
        Some(control) => evaluate_context_policy(control, actual_digest),
    };
    let reason = match &outcome {
        ContextControlOutcome::Fresh { .. } => None,
        ContextControlOutcome::Stale { expected, actual } => Some(format!(
            "the pinned content changed: expected {expected}, observed {actual}; the claim stays labelled"
        )),
        ContextControlOutcome::Blocked { expected, actual } => Some(format!(
            "the frozen content changed: expected {expected}, observed {actual}; the claim is refused"
        )),
    };
    ContextControlDecision { outcome, reason }
}

/// Digest-bound evidence for one context-control decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextControlDecisionEvidence {
    /// The disposition token: `fresh`, `stale`, or `blocked`.
    pub disposition: String,
    /// The bound decision digest.
    pub control_digest: String,
    /// Truthful reason; `None` when the claim is fresh.
    pub reason: Option<String>,
    /// `"absent"` or `"present"`: whether a control was declared.
    pub control: String,
}

/// Create digest-bound evidence for a context-control decision.
///
/// # Errors
///
/// Returns [`ProfileValidationError`] when the digest primitive fails.
pub fn create_context_control_decision_evidence(
    decision: &ContextControlDecision,
    control_present: bool,
) -> Result<ContextControlDecisionEvidence, ProfileValidationError> {
    let payload = CanonicalValue::Object(BTreeMap::from([
        (
            "control".to_owned(),
            CanonicalValue::Str(
                if control_present { "present" } else { "absent" }.to_owned(),
            ),
        ),
        (
            "disposition".to_owned(),
            CanonicalValue::Str(decision.outcome.disposition().to_owned()),
        ),
        (
            "reason".to_owned(),
            match &decision.reason {
                Some(reason) => CanonicalValue::Str(reason.clone()),
                None => CanonicalValue::Null,
            },
        ),
    ]));
    let control_digest =
        compute_artifact_digest("ContextControlDecisionEvidence", 1, &payload)
            .map_err(|error| ProfileValidationError {
                message: error.message,
            })?
            .value;
    Ok(ContextControlDecisionEvidence {
        disposition: decision.outcome.disposition().to_owned(),
        control_digest,
        reason: decision.reason.clone(),
        control: if control_present {
            "present".to_owned()
        } else {
            "absent".to_owned()
        },
    })
}

/// Deterministic report-safe rendering of context-control evidence.
#[must_use]
pub fn render_context_control_decision(
    evidence: &ContextControlDecisionEvidence,
) -> String {
    if evidence.control == "absent" {
        return "context claim unbound".to_owned();
    }
    let base = format!("context claim {}", evidence.disposition);
    match &evidence.reason {
        Some(reason) => format!("{base} ({reason})"),
        None => base,
    }
}

/// What the loading side learned about the on-disk lock (Stage 5.9,
/// decision 55): missing (no lock file), trusted (a well-formed lock
/// whose re-derived digest is carried), or untrusted (the adapter
/// refused the lock; the truthful reason is carried).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredLockDigest {
    /// No lock file exists.
    Missing,
    /// A well-formed stored lock; its re-derived digest.
    Trusted(String),
    /// The stored lock is not trusted; the truthful reason.
    Untrusted(String),
}

/// The typed lock-verification outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockVerificationOutcome {
    /// No on-disk lock: verification is transparent.
    Missing,
    /// The stored digest matches the recomputed one.
    Current,
    /// The stored digest drifted from the recomputed one.
    Stale,
    /// The stored lock is untrusted (corrupt or out of bounds).
    Invalid,
}

impl LockVerificationOutcome {
    /// Stable disposition token for evidence and wire output.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Invalid => "invalid",
        }
    }
}

/// The result of verifying the on-disk lock against the recomputed
/// current lock. The lock never gates authority: every outcome is
/// advisory, and the session proceeds on live Host state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockVerificationDecision {
    /// The typed outcome.
    pub outcome: LockVerificationOutcome,
    /// Truthful, report-safe reason; `None` when missing/current.
    pub reason: Option<String>,
}

/// Decide the session's lock verification. `Missing` is transparent;
/// a trusted digest equal to the recomputed one is current; any other
/// trusted digest is stale with expected/actual; an untrusted lock is
/// invalid with the adapter's truthful reason.
#[must_use]
pub fn decide_lock_verification(
    stored: StoredLockDigest,
    current_digest: &str,
) -> LockVerificationDecision {
    match stored {
        StoredLockDigest::Missing => LockVerificationDecision {
            outcome: LockVerificationOutcome::Missing,
            reason: None,
        },
        StoredLockDigest::Untrusted(reason) => LockVerificationDecision {
            outcome: LockVerificationOutcome::Invalid,
            reason: Some(format!(
                "the on-disk lock could not be trusted: {reason}"
            )),
        },
        StoredLockDigest::Trusted(digest) => {
            if digest == current_digest {
                LockVerificationDecision {
                    outcome: LockVerificationOutcome::Current,
                    reason: None,
                }
            } else {
                LockVerificationDecision {
                    outcome: LockVerificationOutcome::Stale,
                    reason: Some(format!(
                        "the on-disk lock does not match the recomputed workspace lock: expected {current_digest}, actual {digest}"
                    )),
                }
            }
        }
    }
}

/// Digest-bound evidence for one lock-verification decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockVerificationEvidence {
    /// The typed decision string.
    pub decision: String,
    /// The recomputed current lock digest.
    pub lock_digest: String,
    /// Truthful reason; `None` when missing/current.
    pub reason: Option<String>,
    /// The bound decision digest.
    pub verification_digest: String,
}

/// Create digest-bound evidence for a lock-verification decision.
///
/// # Errors
///
/// Returns [`ProfileValidationError`] when the digest primitive fails.
pub fn create_lock_verification_evidence(
    decision: &LockVerificationDecision,
    current_digest: &str,
) -> Result<LockVerificationEvidence, ProfileValidationError> {
    let payload = CanonicalValue::Object(BTreeMap::from([
        (
            "decision".to_owned(),
            CanonicalValue::Str(decision.outcome.as_str().to_owned()),
        ),
        (
            "reason".to_owned(),
            match &decision.reason {
                Some(reason) => CanonicalValue::Str(reason.clone()),
                None => CanonicalValue::Null,
            },
        ),
    ]));
    let verification_digest =
        compute_artifact_digest("LockVerificationEvidence", 1, &payload)
            .map_err(|error| ProfileValidationError {
                message: error.message,
            })?
            .value;
    Ok(LockVerificationEvidence {
        decision: decision.outcome.as_str().to_owned(),
        lock_digest: current_digest.to_owned(),
        reason: decision.reason.clone(),
        verification_digest,
    })
}

/// Deterministic report-safe rendering of lock-verification evidence.
#[must_use]
pub fn render_lock_verification_evidence(
    evidence: &LockVerificationEvidence,
) -> String {
    match evidence.decision.as_str() {
        "missing" => "lock verification missing (transparent)".to_owned(),
        "current" => "lock verified current".to_owned(),
        other => match &evidence.reason {
            Some(reason) => format!("lock {other} ({reason})"),
            None => format!("lock {other}"),
        },
    }
}

/// The session-side skill-catalog state fed to the consumption
/// decision (Stage 5.10, decision 56): the workspace declares no
/// skills directory, or it loads a validated catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillCatalogState<'a> {
    /// No `.siralos/skills` directory: nothing can bind.
    Absent,
    /// The validated workspace catalog.
    Loaded(&'a SkillCatalog),
}

/// The typed skill-consumption outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillConsumptionOutcome {
    /// Nothing binds (no selection, empty selection, or no catalog).
    None,
    /// Every selected skill bound to the catalog.
    Bound,
    /// Some selected skills are not declared by the workspace.
    Unknown,
}

impl SkillConsumptionOutcome {
    /// Stable disposition token for evidence and wire output.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bound => "bound",
            Self::Unknown => "unknown",
        }
    }
}

/// The result of consuming the frozen 5.6 skill seam at a session
/// boundary. Guidance only: the decision can never add capability,
/// Tool, or permission, and absent inputs are transparent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillConsumptionDecision {
    /// The typed outcome.
    pub outcome: SkillConsumptionOutcome,
    /// The frozen 5.6 resolution underneath the decision.
    pub resolution: SkillResolution,
}

/// Decide the session's skill consumption. Absent selection or an
/// absent catalog binds nothing (transparent); otherwise the frozen
/// opt-in intersection applies and unknown selections surface
/// truthfully as the `unknown` outcome.
#[must_use]
pub fn compose_skill_consumption(
    selected: Option<&[String]>,
    catalog: SkillCatalogState<'_>,
) -> SkillConsumptionDecision {
    let resolution = match (catalog, selected) {
        (SkillCatalogState::Absent, _) | (_, None) => {
            SkillResolution { bound: Vec::new(), unknown: Vec::new() }
        }
        (SkillCatalogState::Loaded(catalog), Some(selected)) => {
            resolve_profile_skills(catalog, Some(selected))
        }
    };
    let outcome =
        if resolution.unknown.is_empty() && resolution.bound.is_empty() {
            SkillConsumptionOutcome::None
        } else if resolution.unknown.is_empty() {
            SkillConsumptionOutcome::Bound
        } else {
            SkillConsumptionOutcome::Unknown
        };
    SkillConsumptionDecision { outcome, resolution }
}

/// Digest-bound evidence for one skill-consumption decision. Like the
/// 5.6 evidence it binds, the payload literally carries
/// `authority = none`: the digest is only valid for a consumption that
/// grants no capability at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillConsumptionEvidence {
    /// The typed outcome string.
    pub outcome: String,
    /// The frozen 5.6 resolution evidence underneath the decision.
    pub resolution: SkillResolutionEvidence,
    /// The bound consumption digest.
    pub consumption_digest: String,
}

/// Create digest-bound evidence for a skill-consumption decision.
///
/// # Errors
///
/// Returns [`ProfileValidationError`] when the digest primitive fails.
pub fn create_skill_consumption_evidence(
    decision: &SkillConsumptionDecision,
) -> Result<SkillConsumptionEvidence, ProfileValidationError> {
    let resolution = create_skill_resolution_evidence(&decision.resolution)
        .map_err(|error| ProfileValidationError { message: error.message })?;
    let payload = CanonicalValue::Object(BTreeMap::from([
        ("authority".to_owned(), CanonicalValue::Str("none".to_owned())),
        (
            "outcome".to_owned(),
            CanonicalValue::Str(decision.outcome.as_str().to_owned()),
        ),
        (
            "resolutionDigest".to_owned(),
            CanonicalValue::Str(resolution.resolution_digest.clone()),
        ),
    ]));
    let consumption_digest =
        compute_artifact_digest("SkillConsumptionEvidence", 1, &payload)
            .map_err(|error| ProfileValidationError {
                message: error.message,
            })?
            .value;
    Ok(SkillConsumptionEvidence {
        outcome: decision.outcome.as_str().to_owned(),
        resolution,
        consumption_digest,
    })
}

/// Deterministic report-safe rendering of skill-consumption evidence.
#[must_use]
pub fn render_skill_consumption_evidence(
    evidence: &SkillConsumptionEvidence,
) -> String {
    match evidence.outcome.as_str() {
        "none" => "skills none (guidance only)".to_owned(),
        other => format!(
            "skills {other} {}",
            render_skill_resolution_evidence(&evidence.resolution)
        ),
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
            plugins: None,
            context: None,
            skills: None,
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
            plugins: None,
            context: None,
            skills: None,
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
            plugins: None,
            context: None,
            skills: None,
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
            plugins: None,
            context: None,
            skills: None,
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
            plugins: None,
            context: None,
            skills: None,
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
            plugins: None,
            context: None,
            skills: None,
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
            plugins: None,
            context: None,
            skills: None,
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

    #[test]
    fn lock_resolution_is_deterministic_and_sorted() {
        let mk = |id: &str, digest: &str| super::lock::LockPluginIdentity {
            id: id.to_owned(),
            path: format!("{id}.wasm"),
            digest: digest.to_owned(),
        };
        let low = "a".repeat(64);
        let high = "b".repeat(64);
        let profile = "c".repeat(64);
        let first = super::lock::create_workspace_lock(
            Some(&profile),
            &[mk("zeta", &low), mk("alpha", &high)],
        )
        .expect("lock");
        let second = super::lock::create_workspace_lock(
            Some(&profile),
            &[mk("alpha", &high), mk("zeta", &low)],
        )
        .expect("lock");
        assert_eq!(first.lock_digest, second.lock_digest);
        assert_eq!(first.plugins[0].id, "alpha");
        assert_eq!(first.plugins[1].id, "zeta");
        assert_eq!(first.profile_digest.as_deref(), Some(profile.as_str()));
        let absent =
            super::lock::create_workspace_lock(None, &[]).expect("lock");
        assert_eq!(absent.plugins.len(), 0);
        assert!(absent.profile_digest.is_none());
        assert_ne!(absent.lock_digest, first.lock_digest);
    }

    #[test]
    fn lock_rejects_malformed_identities() {
        let good = "a".repeat(64);
        let error = super::lock::create_workspace_lock(Some("nothex"), &[])
            .expect_err("profile digest refused");
        assert!(error.message.contains("64 lowercase hex"));
        let error = super::lock::create_workspace_lock(
            None,
            &[
                super::lock::LockPluginIdentity {
                    id: "dup".to_owned(),
                    path: "a.wasm".to_owned(),
                    digest: good.clone(),
                },
                super::lock::LockPluginIdentity {
                    id: "dup".to_owned(),
                    path: "b.wasm".to_owned(),
                    digest: good.clone(),
                },
            ],
        )
        .expect_err("duplicate refused");
        assert!(error.message.contains("more than once"));
        let error = super::lock::create_workspace_lock(
            None,
            &[super::lock::LockPluginIdentity {
                id: String::new(),
                path: "a.wasm".to_owned(),
                digest: good,
            }],
        )
        .expect_err("empty id refused");
        assert!(error.message.contains("1..=64 bytes"));
    }

    #[test]
    fn plugin_selection_intersects_and_reports_unknown() {
        let enabled =
            vec!["zeta".to_owned(), "alpha".to_owned(), "guest".to_owned()];
        let selection = super::select_profile_plugins(
            &enabled,
            Some(&["zeta".to_owned(), "ghost".to_owned()]),
        );
        assert_eq!(selection.disposition(), "narrowed");
        assert_eq!(selection.activated, vec!["zeta".to_owned()]);
        assert_eq!(selection.unknown, vec!["ghost".to_owned()]);
        let unfiltered = super::select_profile_plugins(&enabled, None);
        assert_eq!(unfiltered.disposition(), "unfiltered");
        assert_eq!(
            unfiltered.activated,
            vec!["alpha".to_owned(), "guest".to_owned(), "zeta".to_owned()]
        );
        let empty = super::select_profile_plugins(&enabled, Some(&[]));
        assert_eq!(empty.disposition(), "narrowed");
        assert!(empty.activated.is_empty());
        let evidence = super::create_plugin_selection_evidence(&selection)
            .expect("evidence");
        assert_eq!(evidence.activated.len(), 1);
        assert_eq!(evidence.unknown.len(), 1);
        assert_eq!(
            super::render_plugin_selection_evidence(&evidence),
            "narrowed plugins=1 unknown=1"
        );
        let unfiltered_evidence =
            super::create_plugin_selection_evidence(&unfiltered)
                .expect("evidence");
        assert_eq!(
            super::render_plugin_selection_evidence(&unfiltered_evidence),
            "unfiltered plugins=3"
        );
    }

    #[test]
    fn profile_plugin_selection_validates_bounds_and_uniqueness() {
        let record = super::ProfileRecord {
            name: "dev".to_owned(),
            overlay: Vec::new(),
            plugins: Some(vec!["a".to_owned(), "a".to_owned()]),
            context: None,
            skills: None,
        };
        let error = record.validate().expect_err("duplicate refused");
        assert!(error.message.contains("more than once"));
        let record = super::ProfileRecord {
            name: "dev".to_owned(),
            overlay: Vec::new(),
            plugins: Some(vec![String::new()]),
            context: None,
            skills: None,
        };
        let error = record.validate().expect_err("empty id refused");
        assert!(error.message.contains("1..=64 bytes"));
        let record = super::ProfileRecord {
            name: "dev".to_owned(),
            overlay: Vec::new(),
            plugins: Some(vec!["a".to_owned()]),
            context: None,
            skills: None,
        };
        record.validate().expect("valid");
    }

    #[test]
    fn skill_consumption_binds_guidance_only_and_transparently() {
        use super::{
            SkillCatalogState, SkillConsumptionOutcome,
            compose_skill_consumption, create_skill_consumption_evidence,
            render_skill_consumption_evidence,
        };
        use crate::skills::{SkillCatalog, SkillDefinition};
        let catalog = SkillCatalog::new(vec![
            SkillDefinition::new("alpha", "guidance for alpha")
                .expect("skill"),
            SkillDefinition::new("guest", "guidance for guest")
                .expect("skill"),
        ])
        .expect("catalog");
        let loaded = SkillCatalogState::Loaded(&catalog);

        // Absent selection binds nothing (transparent).
        let none = compose_skill_consumption(None, loaded);
        assert_eq!(none.outcome, SkillConsumptionOutcome::None);
        // An absent catalog binds nothing even with a selection.
        let absent = compose_skill_consumption(
            Some(&["alpha".to_owned()]),
            SkillCatalogState::Absent,
        );
        assert_eq!(absent.outcome, SkillConsumptionOutcome::None);

        // A full selection binds every skill (guidance only).
        let bound = compose_skill_consumption(
            Some(&["guest".to_owned(), "alpha".to_owned()]),
            SkillCatalogState::Loaded(&catalog),
        );
        assert_eq!(bound.outcome, SkillConsumptionOutcome::Bound);
        assert_eq!(bound.resolution.bound.len(), 2);
        assert_eq!(bound.resolution.bound[0].name, "alpha");

        // Unknown selections surface truthfully; the bound subset applies.
        let unknown = compose_skill_consumption(
            Some(&["alpha".to_owned(), "ghost".to_owned()]),
            SkillCatalogState::Loaded(&catalog),
        );
        assert_eq!(unknown.outcome, SkillConsumptionOutcome::Unknown);
        assert_eq!(unknown.resolution.unknown, vec!["ghost".to_owned()]);
        assert_eq!(unknown.resolution.bound.len(), 1);

        // Evidence is digest-bound, carries authority = none transitively,
        // and renders deterministically.
        let evidence =
            create_skill_consumption_evidence(&bound).expect("evidence");
        assert_eq!(evidence.outcome, "bound");
        assert_eq!(
            evidence.consumption_digest,
            create_skill_consumption_evidence(&bound)
                .expect("evidence")
                .consumption_digest
        );
        assert_ne!(
            evidence.consumption_digest,
            create_skill_consumption_evidence(&none)
                .expect("evidence")
                .consumption_digest
        );
        assert_eq!(
            render_skill_consumption_evidence(
                &create_skill_consumption_evidence(&none).expect("evidence"),
            ),
            "skills none (guidance only)"
        );
        assert_eq!(
            render_skill_consumption_evidence(&evidence),
            "skills bound bound skills=2 (guidance only)"
        );
    }
    #[test]
    fn lock_verification_never_gates_and_reports_truthfully() {
        use super::{
            LockVerificationOutcome, StoredLockDigest,
            create_lock_verification_evidence, decide_lock_verification,
            render_lock_verification_evidence,
        };
        let current = "a".repeat(64);
        let drifted = "b".repeat(64);

        // Missing is transparent; the session proceeds unchanged.
        let missing =
            decide_lock_verification(StoredLockDigest::Missing, &current);
        assert_eq!(missing.outcome, LockVerificationOutcome::Missing);
        assert_eq!(missing.reason, None);

        // A matching trusted digest is current.
        let current_outcome = decide_lock_verification(
            StoredLockDigest::Trusted(current.clone()),
            &current,
        );
        assert_eq!(current_outcome.outcome, LockVerificationOutcome::Current);
        assert_eq!(current_outcome.reason, None);

        // Any other trusted digest is stale with expected/actual.
        let stale = decide_lock_verification(
            StoredLockDigest::Trusted(drifted.clone()),
            &current,
        );
        assert_eq!(stale.outcome, LockVerificationOutcome::Stale);
        assert!(stale
            .reason
            .as_deref()
            .is_some_and(|reason| {
                reason.starts_with("the on-disk lock does not match the recomputed workspace lock: expected ")
                    && reason.ends_with(&format!("actual {drifted}"))
            }));

        // An untrusted lock is invalid with the truthful reason.
        let invalid = decide_lock_verification(
            StoredLockDigest::Untrusted("corrupt".to_owned()),
            &current,
        );
        assert_eq!(invalid.outcome, LockVerificationOutcome::Invalid);
        assert_eq!(
            invalid.reason.as_deref(),
            Some("the on-disk lock could not be trusted: corrupt")
        );

        // Evidence is digest-bound and renders deterministically.
        let evidence = create_lock_verification_evidence(&stale, &current)
            .expect("evidence");
        assert_eq!(evidence.decision, "stale");
        assert_eq!(evidence.lock_digest, current);
        assert_eq!(
            evidence.verification_digest,
            create_lock_verification_evidence(&stale, &current)
                .expect("evidence")
                .verification_digest
        );
        let moved = decide_lock_verification(
            StoredLockDigest::Trusted("c".repeat(64)),
            &current,
        );
        assert_ne!(
            evidence.verification_digest,
            create_lock_verification_evidence(&moved, &current)
                .expect("evidence")
                .verification_digest
        );
        assert_eq!(
            render_lock_verification_evidence(
                &create_lock_verification_evidence(&missing, &current)
                    .expect("evidence"),
            ),
            "lock verification missing (transparent)"
        );
        assert_eq!(
            render_lock_verification_evidence(
                &create_lock_verification_evidence(&current_outcome, &current)
                    .expect("evidence"),
            ),
            "lock verified current"
        );
        assert!(
            render_lock_verification_evidence(&evidence)
                .starts_with("lock stale (the on-disk lock does not match")
        );
    }
    #[test]
    fn context_control_narrowing_and_refusal() {
        use super::{
            create_context_control_decision_evidence, decide_context_control,
            render_context_control_decision,
        };
        use crate::context::ContextPolicy;
        let bound = "a".repeat(64);
        let other = "b".repeat(64);
        // Absent control is transparent: the claim behaves as Live.
        let absent = decide_context_control(None, &other);
        assert_eq!(absent.outcome.disposition(), "fresh");
        assert_eq!(absent.reason, None);
        // Pinned current: fresh and digest-bound.
        let policy =
            ContextPolicy::new("pinned", Some(&bound)).expect("policy");
        let current = decide_context_control(Some(&policy), &bound);
        assert_eq!(current.outcome.disposition(), "fresh");
        assert_eq!(current.reason, None);
        // Pinned stale: usable but labelled with expected/actual.
        let stale = decide_context_control(Some(&policy), &other);
        assert_eq!(stale.outcome.disposition(), "stale");
        assert!(
            stale
                .reason
                .as_deref()
                .unwrap_or("")
                .starts_with("the pinned content changed: expected ")
        );
        assert!(stale.reason.as_deref().unwrap_or("").contains(&other));
        // Frozen stale: refused with expected/actual.
        let frozen =
            ContextPolicy::new("frozen", Some(&bound)).expect("policy");
        let blocked = decide_context_control(Some(&frozen), &other);
        assert_eq!(blocked.outcome.disposition(), "blocked");
        assert!(
            blocked
                .reason
                .as_deref()
                .unwrap_or("")
                .contains("the claim is refused")
        );
        // Evidence binds control presence, disposition, and reason.
        let evidence = create_context_control_decision_evidence(&stale, true)
            .expect("evidence");
        assert_eq!(evidence.disposition, "stale");
        assert_eq!(evidence.control, "present");
        assert_eq!(
            render_context_control_decision(&evidence),
            format!(
                "context claim stale ({})",
                stale.reason.as_deref().unwrap_or("")
            ),
        );
        let transparent =
            create_context_control_decision_evidence(&absent, false)
                .expect("evidence");
        assert_eq!(
            render_context_control_decision(&transparent),
            "context claim unbound"
        );
        // Evidence digest moves with the decision payload.
        let fresh_bound =
            create_context_control_decision_evidence(&current, true)
                .expect("evidence");
        assert_ne!(evidence.control_digest, fresh_bound.control_digest);
    }
    #[test]
    fn activation_gate_precedence_and_filtering() {
        use super::{
            PluginActivationOutcome, create_plugin_activation_evidence,
            decide_plugin_activation, render_plugin_activation_evidence,
        };
        let enabled = vec!["zeta".to_owned(), "alpha".to_owned()];
        // Host authority first: un-enabled id refused even when selected.
        let refused = decide_plugin_activation(
            &enabled,
            Some(&["alpha".to_owned(), "ghost".to_owned()]),
            "ghost",
        );
        assert_eq!(
            refused.outcome,
            PluginActivationOutcome::RefusedNotEnabled
        );
        assert!(refused.reason.as_deref().unwrap().contains("never enable"));
        // Filtered: enabled but not selected.
        let filtered = decide_plugin_activation(
            &enabled,
            Some(&["alpha".to_owned()]),
            "zeta",
        );
        assert_eq!(filtered.outcome, PluginActivationOutcome::RefusedFiltered);
        assert!(
            filtered.reason.as_deref().unwrap().contains("does not select")
        );
        // Narrowed allow.
        let allowed = decide_plugin_activation(
            &enabled,
            Some(&["alpha".to_owned(), "zeta".to_owned()]),
            "alpha",
        );
        assert_eq!(allowed.outcome, PluginActivationOutcome::Activated);
        assert!(allowed.reason.is_none());
        // Transparent without a selection.
        let transparent = decide_plugin_activation(&enabled, None, "zeta");
        assert_eq!(transparent.outcome, PluginActivationOutcome::Activated);
        // Digest-bound evidence and rendering.
        let evidence = create_plugin_activation_evidence(&filtered, true)
            .expect("evidence");
        assert_eq!(evidence.decision, "refused-filtered");
        assert_eq!(evidence.selection, "present");
        assert_eq!(
            render_plugin_activation_evidence(&evidence, "zeta"),
            "refused-filtered zeta (the workspace profile does not select \"zeta\"; it stays inactive)"
        );
        let absent = create_plugin_activation_evidence(&transparent, false)
            .expect("evidence");
        assert_eq!(absent.selection, "absent");
        assert_eq!(
            render_plugin_activation_evidence(&absent, "zeta"),
            "activated zeta"
        );
    }
}
