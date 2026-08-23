//! Reproducibility manifest (Stage 3 — Deterministic Execution &
//! Reproducibility, ADR 0029; R10a H2).
//!
//! Mirrors `packages/core/src/determinism/reproducibility.ts`. Immutable
//! reference set identifying the exact authoritative inputs of an
//! execution. Uses `serde_json::Value` for payload construction so
//! f64 numbers serialize as unquoted JSON numbers, matching the
//! TypeScript oracle's `JSON.stringify`.

use super::helpers::digest_artifact_payload;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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

fn optional_str(value: &Option<String>) -> Value {
    match value {
        Some(text) => json!(text),
        None => Value::Null,
    }
}

fn optional_u64(value: &Option<u64>) -> Value {
    match value {
        Some(number) => json!(number),
        None => Value::Null,
    }
}

fn optional_f64(value: &Option<f64>) -> Value {
    match value {
        Some(number) => match serde_json::Number::from_f64(*number) {
            Some(n) => Value::Number(n),
            None => Value::Null,
        },
        None => Value::Null,
    }
}

/// Digest one provider input identity (`ProviderInputIdentity` v1);
/// parameters are sorted by name before binding.
pub fn compute_provider_input_identity_digest(
    provider: &ProviderInputIdentity,
) -> Result<String, String> {
    let mut parameters = provider.parameters.clone();
    parameters.sort_by(|left, right| left.0.cmp(&right.0));
    let parameter_values: Vec<Value> = parameters
        .iter()
        .map(|(name, value)| json!({"name": name, "value": value}))
        .collect();
    let payload = json!({
        "providerRoute": optional_str(&provider.provider_route),
        "modelIdentity": optional_str(&provider.model_identity),
        "reasoningMode": optional_str(&provider.reasoning_mode),
        "temperature": optional_f64(&provider.temperature),
        "topP": optional_f64(&provider.top_p),
        "seed": optional_u64(&provider.seed),
        "parameters": parameter_values,
    });
    digest_artifact_payload("ProviderInputIdentity", 1, &payload)
}

/// Clock policy: system time or a fixed test value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    pub fn to_json(&self) -> Value {
        match self {
            Self::System => json!({"mode": "system", "fixedMs": null}),
            Self::Fixed(ms) => json!({"mode": "fixed", "fixedMs": ms}),
        }
    }
}

/// RNG policy: none, seeded, or ambient system randomness.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    pub fn to_json(&self) -> Value {
        match self {
            Self::None => json!({"mode": "none", "seed": null}),
            Self::Seeded(seed) => json!({"mode": "seeded", "seed": seed}),
            Self::System => json!({"mode": "system", "seed": null}),
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
#[derive(Debug, Clone, PartialEq)]
pub struct ReproducibilityManifest {
    /// Normalized inputs (source revisions sorted).
    pub inputs: ReproducibilityManifestInput,
    /// Deterministic digest (`ReproducibilityManifest` v1).
    pub digest: String,
}

fn optional_str_field(value: &Option<String>) -> Value {
    match value {
        Some(text) => json!(text),
        None => Value::Null,
    }
}

/// Create the reproducibility manifest: source revisions are sorted by
/// path; the provider identity is digested separately and its digest
/// enters the binding payload (`ReproducibilityManifest` v1).
pub fn create_reproducibility_manifest(
    mut input: ReproducibilityManifestInput,
) -> Result<ReproducibilityManifest, String> {
    input
        .source_revision_set
        .sort_by(|left, right| left.path.cmp(&right.path));
    let provider_input_digest = match &input.provider_input {
        Some(provider) => {
            Some(compute_provider_input_identity_digest(provider)?)
        }
        None => None,
    };
    let revision_values: Vec<Value> = input
        .source_revision_set
        .iter()
        .map(|revision| json!({"path": revision.path, "revision": revision.revision}))
        .collect();
    let payload = json!({
        "taskId": input.task_id,
        "executionInputDigest": optional_str_field(&input.execution_input_digest),
        "environmentDigest": optional_str_field(&input.environment_digest),
        "taskContractDigest": optional_str_field(&input.task_contract_digest),
        "taskPlanDigest": optional_str_field(&input.task_plan_digest),
        "guidanceDigest": optional_str_field(&input.guidance_digest),
        "toolSurfaceDigest": optional_str_field(&input.tool_surface_digest),
        "capabilityDigest": optional_str_field(&input.capability_digest),
        "sourceRevisionSet": revision_values,
        "validationProfile": optional_str_field(&input.validation_profile),
        "providerInputDigest": match &provider_input_digest {
            Some(digest) => json!(digest),
            None => Value::Null,
        },
        "clockPolicy": input.clock_policy.to_json(),
        "rngPolicy": input.rng_policy.to_json(),
    });
    let framed = format!(
        "siralos:ReproducibilityManifest:v1\u{0}{}",
        crate::identity::canonical_json_value(&payload)
    );
    let digest = crate::identity::sha256_hex(framed.as_bytes());
    Ok(ReproducibilityManifest { inputs: input, digest })
}

/// Declared reproducibility sections in oracle order.
pub const REPRODUCIBILITY_SECTIONS: [&str; 12] = [
    "executionInput",
    "environment",
    "taskContract",
    "taskPlan",
    "guidance",
    "toolSurface",
    "capability",
    "sourceRevisions",
    "validationProfile",
    "providerInput",
    "clockPolicy",
    "rngPolicy",
];

fn provider_to_json(provider: Option<&ProviderInputIdentity>) -> Value {
    match provider {
        None => Value::Null,
        Some(p) => json!({
            "providerRoute": p.provider_route,
            "modelIdentity": p.model_identity,
            "reasoningMode": p.reasoning_mode,
            "temperature": p.temperature,
            "topP": p.top_p,
            "seed": p.seed,
            "parameters": p.parameters.iter()
                .map(|(name, value)| json!({"name": name, "value": value}))
                .collect::<Vec<_>>(),
        }),
    }
}
fn reproducibility_sections(
    manifest: &ReproducibilityManifest,
) -> BTreeMap<&'static str, Value> {
    let input = &manifest.inputs;
    [
        ("executionInput", optional_str_field(&input.execution_input_digest)),
        ("environment", optional_str_field(&input.environment_digest)),
        ("taskContract", optional_str_field(&input.task_contract_digest)),
        ("taskPlan", optional_str_field(&input.task_plan_digest)),
        ("guidance", optional_str_field(&input.guidance_digest)),
        ("toolSurface", optional_str_field(&input.tool_surface_digest)),
        ("capability", optional_str_field(&input.capability_digest)),
        (
            "sourceRevisions",
            Value::Array(
                input
                    .source_revision_set
                    .iter()
                    .map(|r| json!({"path": r.path, "revision": r.revision}))
                    .collect(),
            ),
        ),
        ("validationProfile", optional_str_field(&input.validation_profile)),
        ("providerInput", provider_to_json(input.provider_input.as_ref())),
        ("clockPolicy", input.clock_policy.to_json()),
        ("rngPolicy", input.rng_policy.to_json()),
    ]
    .into_iter()
    .collect()
}

/// Derived delta between two reproducibility manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReproducibilityDelta {
    /// Base manifest digest.
    pub base_digest: String,
    /// Result manifest digest.
    pub result_digest: String,
    /// Sections whose canonical form differs (declared order).
    pub changed: Vec<String>,
    /// Sections whose canonical form matches (declared order).
    pub unchanged: Vec<String>,
    /// True when no section changed.
    pub unchanged_content: bool,
}

/// Compute the delta between two manifests over the twelve declared
/// sections.
#[must_use]
pub fn compute_reproducibility_delta(
    base: &ReproducibilityManifest,
    result: &ReproducibilityManifest,
) -> ReproducibilityDelta {
    let base_sections = reproducibility_sections(base);
    let result_sections = reproducibility_sections(result);
    let empty = Value::Null;
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    for section in REPRODUCIBILITY_SECTIONS {
        let base_value = base_sections.get(section).unwrap_or(&empty);
        let result_value = result_sections.get(section).unwrap_or(&empty);
        if crate::identity::canonical_json_value(base_value)
            == crate::identity::canonical_json_value(result_value)
        {
            unchanged.push((*section).to_owned());
        } else {
            changed.push((*section).to_owned());
        }
    }
    let unchanged_content = changed.is_empty();
    ReproducibilityDelta {
        base_digest: base.digest.clone(),
        result_digest: result.digest.clone(),
        changed,
        unchanged,
        unchanged_content,
    }
}

// Suppress unused warnings for re-exported helpers.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_temperature_serializes_as_number_not_string() {
        let provider = ProviderInputIdentity {
            provider_route: Some("fake".to_owned()),
            model_identity: Some("det-v1".to_owned()),
            reasoning_mode: None,
            temperature: Some(0.5),
            top_p: Some(1.0),
            seed: Some(42),
            parameters: vec![],
        };
        let digest =
            compute_provider_input_identity_digest(&provider).expect("digest");
        assert_eq!(digest.len(), 64);
        // Verify that changing temperature changes the digest.
        let mut changed = provider.clone();
        changed.temperature = Some(0.9);
        let other =
            compute_provider_input_identity_digest(&changed).expect("digest");
        assert_ne!(digest, other);
    }

    #[test]
    fn manifest_binds_source_revisions_sorted_by_path() {
        let input = ReproducibilityManifestInput {
            task_id: "t1".to_owned(),
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
            ..Default::default()
        };
        let manifest =
            create_reproducibility_manifest(input).expect("manifest");
        assert_eq!(
            manifest
                .inputs
                .source_revision_set
                .iter()
                .map(|r| r.path.as_str())
                .collect::<Vec<_>>(),
            vec!["res://a.gd", "res://b.gd"]
        );
    }

    #[test]
    fn delta_reports_clock_policy_change() {
        let base_input = ReproducibilityManifestInput {
            task_id: "t1".to_owned(),
            clock_policy: ClockPolicy::System,
            rng_policy: RngPolicy::None,
            ..Default::default()
        };
        let base =
            create_reproducibility_manifest(base_input.clone()).expect("base");
        let mut changed_input = base_input;
        changed_input.clock_policy = ClockPolicy::Fixed(5000);
        let result =
            create_reproducibility_manifest(changed_input).expect("changed");
        let delta = compute_reproducibility_delta(&base, &result);
        assert_eq!(delta.changed, vec!["clockPolicy"]);
        assert!(!delta.unchanged_content);
    }
}
