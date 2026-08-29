//! Context controls (Stage 5.3, decision 49): how tightly Siralos binds
//! what it gives the model.
//!
//! A [`ContextPolicy`] is pure declarative data over a content digest:
//! [`ContextPolicy::Live`] asserts nothing and binds nothing;
//! [`ContextPolicy::Pinned`] binds a content digest and reports
//! staleness truthfully while remaining usable;
//! [`ContextPolicy::Frozen`] binds a content digest and refuses use of
//! stale content. The one mechanical property: a control can only
//! narrow what Siralos claims about content — it can never claim a
//! freshness it has not verified. No network, no spawn, no wall clock.

use std::collections::BTreeMap;

use crate::context::{ContextError, context_error};
use crate::identity::CanonicalValue;
use crate::identity::compute_artifact_digest;

fn content_digest(digest: &str) -> Result<(), ContextError> {
    let valid = digest.len() == 64
        && digest
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    if valid {
        Ok(())
    } else {
        Err(context_error(
            "A context control digest must be 64 lowercase hex characters.",
        ))
    }
}

fn pinned(digest: &str) -> Result<String, ContextError> {
    content_digest(digest)?;
    Ok(digest.to_owned())
}

/// A declarative context control over content identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPolicy {
    /// Assert nothing, bind nothing: the content is used as observed.
    Live,
    /// Bind a content digest; a mismatch is stale but still usable.
    Pinned {
        /// The bound content digest (64 lowercase hex characters).
        digest: String,
    },
    /// Bind a content digest; a mismatch refuses use of the content.
    Frozen {
        /// The bound content digest (64 lowercase hex characters).
        digest: String,
    },
}

impl ContextPolicy {
    /// Construct a policy, validating pinned/frozen digests.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError`] when a `Pinned`/`Frozen` digest is not 64
    /// lowercase hex characters.
    pub fn new(
        kind: &str,
        digest: Option<&str>,
    ) -> Result<Self, ContextError> {
        match (kind, digest) {
            ("live", _) => Ok(Self::Live),
            ("pinned", Some(digest)) => {
                Ok(Self::Pinned { digest: pinned(digest)? })
            }
            ("frozen", Some(digest)) => {
                Ok(Self::Frozen { digest: pinned(digest)? })
            }
            (kind, _) => Err(context_error(format!(
                "Unknown context control kind {kind:?}."
            ))),
        }
    }

    /// Stable disposition token for evidence and wire output.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Pinned { .. } => "pinned",
            Self::Frozen { .. } => "frozen",
        }
    }
}

/// The evaluated outcome of a context control against observed content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextControlOutcome {
    /// The content may be used; the bound digest when one was pinned.
    Fresh {
        /// The verified bound digest, when the policy binds one.
        bound: Option<String>,
    },
    /// Bound content changed: usable under `Pinned`, labelled stale.
    Stale {
        /// The digest the policy bound.
        expected: String,
        /// The observed content digest.
        actual: String,
    },
    /// Bound content changed under `Frozen`: use is refused.
    Blocked {
        /// The digest the policy bound.
        expected: String,
        /// The observed content digest.
        actual: String,
    },
}

impl ContextControlOutcome {
    /// Stable disposition token for evidence and wire output.
    #[must_use]
    pub fn disposition(&self) -> &'static str {
        match self {
            Self::Fresh { .. } => "fresh",
            Self::Stale { .. } => "stale",
            Self::Blocked { .. } => "blocked",
        }
    }

    /// Whether the content may be used at all. Only `Blocked` refuses.
    #[must_use]
    pub fn usable(&self) -> bool {
        !matches!(self, Self::Blocked { .. })
    }
}

/// Evaluate a context control against the observed content digest.
#[must_use]
pub fn evaluate_context_policy(
    policy: &ContextPolicy,
    actual_digest: &str,
) -> ContextControlOutcome {
    match policy {
        ContextPolicy::Live => ContextControlOutcome::Fresh { bound: None },
        ContextPolicy::Pinned { digest } => {
            if digest == actual_digest {
                ContextControlOutcome::Fresh { bound: Some(digest.clone()) }
            } else {
                ContextControlOutcome::Stale {
                    expected: digest.clone(),
                    actual: actual_digest.to_owned(),
                }
            }
        }
        ContextPolicy::Frozen { digest } => {
            if digest == actual_digest {
                ContextControlOutcome::Fresh { bound: Some(digest.clone()) }
            } else {
                ContextControlOutcome::Blocked {
                    expected: digest.clone(),
                    actual: actual_digest.to_owned(),
                }
            }
        }
    }
}

/// Digest-bound evidence for one evaluated context control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextControlEvidence {
    /// The policy that was evaluated.
    pub policy: ContextPolicy,
    /// The outcome that was produced.
    pub outcome: ContextControlOutcome,
    /// Artifact digest over the canonical evidence payload.
    pub control_digest: String,
}

/// Build digest-bound evidence for an evaluated control. The payload
/// binds the control kind, every digest it names, and the disposition,
/// so any change to what Siralos claims about the content moves the
/// digest.
///
/// # Errors
///
/// Returns [`ContextError`] when canonical serialization or digest
/// computation fails.
pub fn create_context_control_evidence(
    policy: &ContextPolicy,
    outcome: &ContextControlOutcome,
) -> Result<ContextControlEvidence, ContextError> {
    let (expected_digest, actual_digest, bound_digest) = match outcome {
        ContextControlOutcome::Fresh { bound } => (None, None, bound.clone()),
        ContextControlOutcome::Stale { expected, actual } => {
            (Some(expected.clone()), Some(actual.clone()), None)
        }
        ContextControlOutcome::Blocked { expected, actual } => {
            (Some(expected.clone()), Some(actual.clone()), None)
        }
    };
    let mut map = BTreeMap::new();
    map.insert(
        "actualDigest".to_owned(),
        actual_digest.map_or(CanonicalValue::Null, CanonicalValue::Str),
    );
    map.insert(
        "boundDigest".to_owned(),
        bound_digest.map_or(CanonicalValue::Null, CanonicalValue::Str),
    );
    map.insert(
        "disposition".to_owned(),
        CanonicalValue::Str(outcome.disposition().to_owned()),
    );
    map.insert(
        "expectedDigest".to_owned(),
        expected_digest.map_or(CanonicalValue::Null, CanonicalValue::Str),
    );
    map.insert(
        "policyKind".to_owned(),
        CanonicalValue::Str(policy.kind().to_owned()),
    );
    let control_digest = compute_artifact_digest(
        "ContextControlEvidence",
        1,
        &CanonicalValue::Object(map),
    )
    .map_err(|error| context_error(error.message))?
    .value;
    Ok(ContextControlEvidence {
        policy: policy.clone(),
        outcome: outcome.clone(),
        control_digest,
    })
}

/// Bounded deterministic rendering of an evaluated control.
#[must_use]
pub fn render_context_control_evidence(
    evidence: &ContextControlEvidence,
) -> String {
    let abbrev = |digest: &str| digest.chars().take(8).collect::<String>();
    match (&evidence.policy, &evidence.outcome) {
        (ContextPolicy::Live, ContextControlOutcome::Fresh { .. }) => {
            "live fresh (unbound)".to_owned()
        }
        (
            ContextPolicy::Pinned { digest },
            ContextControlOutcome::Fresh { .. },
        ) => {
            format!("pinned fresh bound={}", abbrev(digest))
        }
        (
            ContextPolicy::Frozen { digest },
            ContextControlOutcome::Fresh { .. },
        ) => {
            format!("frozen fresh bound={}", abbrev(digest))
        }
        (
            ContextPolicy::Pinned { .. },
            ContextControlOutcome::Stale { expected, actual },
        ) => {
            format!(
                "pinned stale expected={} actual={} (usable)",
                abbrev(expected),
                abbrev(actual)
            )
        }
        (
            ContextPolicy::Frozen { .. },
            ContextControlOutcome::Blocked { expected, actual },
        ) => {
            format!(
                "frozen stale expected={} actual={} (blocked)",
                abbrev(expected),
                abbrev(actual)
            )
        }
        _ => "inconsistent control outcome".to_owned(),
    }
}

#[cfg(test)]
mod controls_tests {
    use super::{
        ContextControlOutcome, ContextPolicy, create_context_control_evidence,
        evaluate_context_policy, render_context_control_evidence,
    };

    const CURRENT: &str =
        "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
    const REVISED: &str =
        "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";

    #[test]
    fn live_asserts_nothing() {
        let policy = ContextPolicy::new("live", None).expect("policy");
        let outcome = evaluate_context_policy(&policy, REVISED);
        assert_eq!(outcome.disposition(), "fresh");
        assert!(outcome.usable());
        match &outcome {
            ContextControlOutcome::Fresh { bound } => {
                assert_eq!(bound.as_deref(), None);
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
        let evidence = create_context_control_evidence(&policy, &outcome)
            .expect("evidence");
        assert_eq!(evidence.control_digest.len(), 64);
        assert_eq!(
            render_context_control_evidence(&evidence),
            "live fresh (unbound)"
        );
    }

    #[test]
    fn pinned_current_is_fresh_and_bound() {
        let policy =
            ContextPolicy::new("pinned", Some(CURRENT)).expect("policy");
        let outcome = evaluate_context_policy(&policy, CURRENT);
        assert_eq!(outcome.disposition(), "fresh");
        match &outcome {
            ContextControlOutcome::Fresh { bound } => {
                assert_eq!(bound.as_deref(), Some(CURRENT));
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
        let evidence = create_context_control_evidence(&policy, &outcome)
            .expect("evidence");
        assert_eq!(
            render_context_control_evidence(&evidence),
            format!("pinned fresh bound={}", &CURRENT[..8]),
        );
    }

    #[test]
    fn pinned_stale_is_truthful_but_usable() {
        let policy =
            ContextPolicy::new("pinned", Some(CURRENT)).expect("policy");
        let outcome = evaluate_context_policy(&policy, REVISED);
        assert_eq!(outcome.disposition(), "stale");
        assert!(outcome.usable());
        match &outcome {
            ContextControlOutcome::Stale { expected, actual } => {
                assert_eq!(expected, CURRENT);
                assert_eq!(actual, REVISED);
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
        let evidence = create_context_control_evidence(&policy, &outcome)
            .expect("evidence");
        assert!(
            render_context_control_evidence(&evidence).contains("(usable)")
        );
    }

    #[test]
    fn frozen_stale_blocks_use() {
        let policy =
            ContextPolicy::new("frozen", Some(CURRENT)).expect("policy");
        let outcome = evaluate_context_policy(&policy, REVISED);
        assert_eq!(outcome.disposition(), "blocked");
        assert!(!outcome.usable());
        let evidence = create_context_control_evidence(&policy, &outcome)
            .expect("evidence");
        assert!(
            render_context_control_evidence(&evidence).contains("(blocked)")
        );
        let frozen_current = evaluate_context_policy(&policy, CURRENT);
        assert_eq!(frozen_current.disposition(), "fresh");
        assert!(frozen_current.usable());
    }

    #[test]
    fn invalid_digests_and_kinds_are_typed() {
        let error = ContextPolicy::new("pinned", Some("nothex"))
            .expect_err("digest refused");
        assert!(error.message.contains("64 lowercase hex characters"));
        let error =
            ContextPolicy::new("stale", None).expect_err("kind refused");
        assert!(error.message.contains("Unknown context control kind"));
    }
}
