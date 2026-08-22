//! Reproducibility manifest (Stage 3 — Deterministic Execution &
//! Reproducibility, ADR 0029; R10a H2).
//!
//! Mirrors `packages/core/src/determinism/reproducibility.ts`. Immutable
//! reference set identifying the exact authoritative inputs of an
//! execution: H1 artifact digests (never duplicated contents), source
//! revision set, validation profile, provider/model runtime profile,
//! clock policy, and RNG policy. A result identifies
//! `producedUnder: <ReproducibilityManifest digest>`.

use crate::identity::{
    ArtifactIdentityError, CanonicalValue, canonical_artifact_payload,
    sha256_hex,
};
use std::collections::BTreeMap;

fn object(entries: Vec<(&str, CanonicalValue)>) -> CanonicalValue {
    CanonicalValue::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn string_value(value: &str) -> CanonicalValue {
    CanonicalValue::Str(value.to_owned())
}

fn optional_string(value: &Option<String>) -> CanonicalValue {
    match value {
        Some(text) => CanonicalValue::Str(text.clone()),
        None => CanonicalValue::Null,
    }
}

/// Format an f64 like JavaScript's `Number.prototype.toString`:
/// integral values print without a decimal point.
#[must_use]
pub fn js_number_string(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e21 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn optional_f64(value: &Option<f64>) -> CanonicalValue {
    match value {
        Some(number) => CanonicalValue::Str(js_number_string(*number)),
        None => CanonicalValue::Null,
    }
}

fn optional_u64(value: &Option<u64>) -> CanonicalValue {
    match value {
        Some(number) => CanonicalValue::U64(*number),
        None => CanonicalValue::Null,
    }
}

/// Provider/model runtime input identity.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderInputIdentity {
    /// Provider route when known.
    pub provider_route: Option<String>,
    /// Model identity when known.
    pub model_identity: Option<String>,
    /// Reasoning mode when pinned.
    pub reasoning_mode: Option<String>,
    /// Temperature setting.
    pub temperature: Option<f64>,
    /// Top-p setting.
    pub top_p: Option<f64>,
    /// Seed.
    pub seed: Option<u64>,
    /// Behavior-affecting parameters (bounded, no secrets).
    pub parameters: Vec<(String, String)>,
}

/// Digest one provider input identity (`ProviderInputIdentity` v1);
/// parameters are sorted by name before binding.
pub fn compute_provider_input_identity_digest(
    provider: &ProviderInputIdentity,
) -> Result<String, ArtifactIdentityError> {
    let mut parameters = provider.parameters.clone();
    parameters.sort_by(|left, right| left.0.cmp(&right.0));
    let parameter_values: Vec<CanonicalValue> = parameters
        .iter()
        .map(|(name, value)| {
            CanonicalValue::Object(BTreeMap::from([
                ("name".to_owned(), CanonicalValue::Str(name.clone())),
                ("value".to_owned(), CanonicalValue::Str(value.clone())),
            ]))
        })
        .collect();
    let payload = object(vec![
        ("providerRoute", optional_string(&provider.provider_route)),
        ("modelIdentity", optional_string(&provider.model_identity)),
        ("reasoningMode", optional_string(&provider.reasoning_mode)),
        ("temperature", optional_f64(&provider.temperature)),
        ("topP", optional_f64(&provider.top_p)),
        ("seed", optional_u64(&provider.seed)),
        ("parameters", CanonicalValue::Array(parameter_values)),
    ]);
    let framed =
        canonical_artifact_payload("ProviderInputIdentity", 1, &payload)
            .map_err(|error| ArtifactIdentityError {
                message: error.message,
            })?;
    Ok(sha256_hex(framed.as_bytes()))
}

/// Clock policy: system time or a fixed test value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockPolicy {
    /// System wall clock.
    #[default]
    System,
    /// Fixed millisecond value.
    Fixed(u64),
}

impl ClockPolicy {
    /// Canonical JSON form matching the oracle's `{mode, fixedMs}`.
    #[must_use]
    pub fn to_canonical(&self) -> CanonicalValue {
        match self {
            Self::System => object(vec![
                ("mode", string_value("system")),
                ("fixedMs", CanonicalValue::Null),
            ]),
            Self::Fixed(ms) => object(vec![
                ("mode", string_value("fixed")),
                ("fixedMs", CanonicalValue::U64(*ms)),
            ]),
        }
    }
}

/// RNG policy: none, seeded, or ambient system randomness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RngPolicy {
    /// No randomness.
    #[default]
    None,
    /// Seeded generator.
    Seeded(u64),
    /// Ambient system randomness.
    System,
}

impl RngPolicy {
    /// Canonical JSON form matching the oracle's `{mode, seed}`.
    #[must_use]
    pub fn to_canonical(&self) -> CanonicalValue {
        match self {
            Self::None => object(vec![
                ("mode", string_value("none")),
                ("seed", CanonicalValue::Null),
            ]),
            Self::Seeded(seed) => object(vec![
                ("mode", string_value("seeded")),
                ("seed", CanonicalValue::U64(*seed)),
            ]),
            Self::System => object(vec![
                ("mode", string_value("system")),
                ("seed", CanonicalValue::Null),
            ]),
        }
    }
}

/// One source revision in the reference set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRevision {
    /// Workspace-relative path.
    pub path: String,
    /// Exact revision handle.
    pub revision: String,
}

/// Inputs for [`create_reproducibility_manifest`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReproducibilityManifestInput {
    /// Owning task id.
    pub task_id: String,
    /// Execution-input environment digest.
    pub execution_input_digest: Option<String>,
    /// Environment digest.
    pub environment_digest: Option<String>,
    /// TaskContract content digest.
    pub task_contract_digest: Option<String>,
    /// TaskPlan content digest.
    pub task_plan_digest: Option<String>,
    /// Guidance aggregate digest.
    pub guidance_digest: Option<String>,
    /// Tool surface digest.
    pub tool_surface_digest: Option<String>,
    /// Capability snapshot digest.
    pub capability_digest: Option<String>,
    /// Source revision set (sorted here by path).
    pub source_revision_set: Vec<SourceRevision>,
    /// Validation profile id.
    pub validation_profile: Option<String>,
    /// Provider input identity when a provider participates.
    pub provider_input: Option<ProviderInputIdentity>,
    /// Clock policy.
    pub clock_policy: ClockPolicy,
    /// RNG policy.
    pub rng_policy: RngPolicy,
}

/// Immutable reproducibility reference set with its digest.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReproducibilityManifest {
    /// Normalized inputs (source revisions sorted).
    pub inputs: ReproducibilityManifestInput,
    /// Deterministic digest (`ReproducibilityManifest` v1).
    pub digest: String,
}

/// Create the reproducibility manifest: source revisions are sorted by
/// path; the provider identity is digested separately and its digest
/// enters the binding payload (`ReproducibilityManifest` v1).
pub fn create_reproducibility_manifest(
    mut input: ReproducibilityManifestInput,
) -> Result<ReproducibilityManifest, ArtifactIdentityError> {
    input
        .source_revision_set
        .sort_by(|left, right| left.path.cmp(&right.path));
    let provider_input_digest = match &input.provider_input {
        Some(provider) => {
            Some(compute_provider_input_identity_digest(provider)?)
        }
        None => None,
    };
    let revision_values: Vec<CanonicalValue> = input
        .source_revision_set
        .iter()
        .map(|revision| {
            object(vec![
                ("path", string_value(&revision.path)),
                ("revision", string_value(&revision.revision)),
            ])
        })
        .collect();
    let payload = object(vec![
        ("taskId", string_value(&input.task_id)),
        (
            "executionInputDigest",
            optional_string(&input.execution_input_digest),
        ),
        ("environmentDigest", optional_string(&input.environment_digest)),
        ("taskContractDigest", optional_string(&input.task_contract_digest)),
        ("taskPlanDigest", optional_string(&input.task_plan_digest)),
        ("guidanceDigest", optional_string(&input.guidance_digest)),
        ("toolSurfaceDigest", optional_string(&input.tool_surface_digest)),
        ("capabilityDigest", optional_string(&input.capability_digest)),
        ("sourceRevisionSet", CanonicalValue::Array(revision_values)),
        ("validationProfile", optional_string(&input.validation_profile)),
        ("providerInputDigest", optional_string(&provider_input_digest)),
        ("clockPolicy", input.clock_policy.to_canonical()),
        ("rngPolicy", input.rng_policy.to_canonical()),
    ]);
    let framed =
        canonical_artifact_payload("ReproducibilityManifest", 1, &payload)
            .map_err(|error| ArtifactIdentityError {
                message: error.message,
            })?;
    let digest = sha256_hex(framed.as_bytes());
    Ok(ReproducibilityManifest { inputs: input, digest })
}

#[cfg(test)]
mod tests {
    use super::{
        ClockPolicy, ProviderInputIdentity, ReproducibilityManifestInput,
        RngPolicy, SourceRevision, create_reproducibility_manifest,
    };

    fn provider() -> ProviderInputIdentity {
        ProviderInputIdentity {
            provider_route: Some("fake".to_owned()),
            model_identity: Some("deterministic-v1".to_owned()),
            reasoning_mode: None,
            temperature: Some(0.5),
            top_p: Some(1.0),
            seed: Some(42),
            parameters: vec![("maxTokens".to_owned(), "1024".to_owned())],
        }
    }

    fn input() -> ReproducibilityManifestInput {
        ReproducibilityManifestInput {
            task_id: "task-1".to_owned(),
            execution_input_digest: Some("exec-digest".to_owned()),
            environment_digest: Some("env-digest".to_owned()),
            task_contract_digest: Some("contract-digest".to_owned()),
            task_plan_digest: None,
            guidance_digest: Some("guidance-digest".to_owned()),
            tool_surface_digest: Some("surface-digest".to_owned()),
            capability_digest: Some("capability-digest".to_owned()),
            source_revision_set: vec![
                SourceRevision {
                    path: "res://b.gd".to_owned(),
                    revision: "rev_b".to_owned(),
                },
                SourceRevision {
                    path: "res://a.gd".to_owned(),
                    revision: "rev_a".to_owned(),
                },
            ],
            validation_profile: Some("develop-offline".to_owned()),
            provider_input: Some(provider()),
            clock_policy: ClockPolicy::Fixed(1_000),
            rng_policy: RngPolicy::Seeded(7),
        }
    }

    #[test]
    fn source_revisions_sort_by_path_and_digest_is_deterministic() {
        let manifest =
            create_reproducibility_manifest(input()).expect("manifest");
        assert_eq!(
            manifest
                .inputs
                .source_revision_set
                .iter()
                .map(|r| r.path.as_str())
                .collect::<Vec<_>>(),
            vec!["res://a.gd", "res://b.gd"]
        );
        let again =
            create_reproducibility_manifest(input()).expect("manifest");
        assert_eq!(manifest.digest, again.digest);
    }

    #[test]
    fn digest_binds_every_section_including_provider_and_policies() {
        let mut changed = input();
        if let Some(provider) = &mut changed.provider_input {
            provider.temperature = Some(0.9);
        }
        changed.clock_policy = ClockPolicy::System;
        let base = create_reproducibility_manifest(input()).expect("base");
        let other = create_reproducibility_manifest(changed).expect("changed");
        assert_ne!(base.digest, other.digest);
    }

    #[test]
    fn provider_parameter_order_does_not_change_the_digest() {
        let mut reordered = input();
        reordered.provider_input.as_mut().expect("provider").parameters = vec![
            ("maxTokens".to_owned(), "1024".to_owned()),
            ("topK".to_owned(), "40".to_owned()),
        ];
        let mut original = input();
        original.provider_input.as_mut().expect("provider").parameters = vec![
            ("topK".to_owned(), "40".to_owned()),
            ("maxTokens".to_owned(), "1024".to_owned()),
        ];
        let left = create_reproducibility_manifest(reordered).expect("left");
        let right = create_reproducibility_manifest(original).expect("right");
        assert_eq!(left.digest, right.digest);
    }

    #[test]
    fn no_provider_still_produces_a_manifest() {
        let mut without_provider = input();
        without_provider.provider_input = None;
        let manifest = create_reproducibility_manifest(without_provider)
            .expect("manifest");
        assert!(manifest.digest.len() == 64);
    }
}
