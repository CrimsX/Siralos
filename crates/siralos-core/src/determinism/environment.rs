//! Environment manifest (Stage 3 — Deterministic Execution &
//! Reproducibility, ADR 0029; R10a H2).
//!
//! Mirrors `packages/core/src/determinism/environment.ts`: bounded
//! identity of the execution-relevant environment — build/version,
//! Node/npm identity, OS/platform/architecture, Godot fingerprint,
//! sandbox identity, explicit locale and timezone policy, and the
//! relevant environment allowlist (names only). Secrets are never
//! included. The digest binds into the ReproducibilityManifest.

use crate::identity::{
    CanonicalValue, canonical_artifact_payload, canonicalize, sha256_hex,
};
use std::collections::BTreeMap;

fn optional_value(value: &Option<String>) -> CanonicalValue {
    match value {
        Some(text) => CanonicalValue::Str(text.clone()),
        None => CanonicalValue::Null,
    }
}

fn string_list(values: &[String]) -> CanonicalValue {
    CanonicalValue::Array(
        values
            .iter()
            .map(|value| CanonicalValue::Str(value.clone()))
            .collect(),
    )
}

fn tool_list(tools: &[(String, String)]) -> CanonicalValue {
    CanonicalValue::Array(
        tools
            .iter()
            .map(|(name, digest)| {
                CanonicalValue::Object(BTreeMap::from([
                    ("name".to_owned(), CanonicalValue::Str(name.clone())),
                    ("digest".to_owned(), CanonicalValue::Str(digest.clone())),
                ]))
            })
            .collect(),
    )
}

/// Inputs for one environment manifest. `None` fields serialize as
/// null; the allowlist is name-only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvironmentManifestInput {
    /// Siralos build version.
    pub siralos_version: Option<String>,
    /// Node runtime version.
    pub node_version: Option<String>,
    /// npm version.
    pub npm_version: Option<String>,
    /// Platform identifier.
    pub platform: Option<String>,
    /// Architecture identifier.
    pub arch: Option<String>,
    /// OS release string.
    pub os_release: Option<String>,
    /// Verified Godot executable fingerprint.
    pub godot_executable_fingerprint: Option<String>,
    /// Sandbox backend id.
    pub sandbox_backend_id: Option<String>,
    /// Sandbox version.
    pub sandbox_version: Option<String>,
    /// Explicit locale policy (never ambient).
    pub locale_policy: Option<String>,
    /// Explicit timezone policy (never ambient).
    pub timezone_policy: Option<String>,
    /// Allowlist names only, never secret values.
    pub environment_allowlist: Vec<String>,
    /// Tool identities (name, digest), sorted by name during creation.
    pub tool_identities: Vec<(String, String)>,
}

/// Immutable bounded environment identity with its digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentManifest {
    /// Normalized inputs (allowlist and tools sorted).
    pub inputs: EnvironmentManifestInput,
    /// Deterministic digest (`EnvironmentManifest` v1).
    pub digest: String,
}

fn environment_sections(
    manifest: &EnvironmentManifest,
) -> BTreeMap<&'static str, CanonicalValue> {
    let input = &manifest.inputs;
    [
        ("siralosVersion", optional_value(&input.siralos_version)),
        ("nodeVersion", optional_value(&input.node_version)),
        ("npmVersion", optional_value(&input.npm_version)),
        ("platform", optional_value(&input.platform)),
        ("arch", optional_value(&input.arch)),
        ("osRelease", optional_value(&input.os_release)),
        (
            "godotExecutableFingerprint",
            optional_value(&input.godot_executable_fingerprint),
        ),
        ("sandboxBackendId", optional_value(&input.sandbox_backend_id)),
        ("sandboxVersion", optional_value(&input.sandbox_version)),
        ("localePolicy", optional_value(&input.locale_policy)),
        ("timezonePolicy", optional_value(&input.timezone_policy)),
        ("environmentAllowlist", string_list(&input.environment_allowlist)),
        ("toolIdentities", tool_list(&input.tool_identities)),
    ]
    .into_iter()
    .collect()
}

/// Declared environment sections in oracle order.
pub const ENVIRONMENT_SECTIONS: [&str; 13] = [
    "siralosVersion",
    "nodeVersion",
    "npmVersion",
    "platform",
    "arch",
    "osRelease",
    "godotExecutableFingerprint",
    "sandboxBackendId",
    "sandboxVersion",
    "localePolicy",
    "timezonePolicy",
    "environmentAllowlist",
    "toolIdentities",
];

/// Create the environment manifest: allowlist entries and tool
/// identities are sorted deterministically before binding
/// (`EnvironmentManifest` v1).
#[must_use]
pub fn create_environment_manifest(
    mut input: EnvironmentManifestInput,
) -> EnvironmentManifest {
    input.environment_allowlist.sort();
    input.tool_identities.sort_by(|left, right| left.0.cmp(&right.0));
    let sections = environment_sections(&EnvironmentManifest {
        inputs: input.clone(),
        digest: String::new(),
    });
    let mut payload = BTreeMap::new();
    for section in ENVIRONMENT_SECTIONS {
        payload.insert(
            section.to_owned(),
            sections.get(section).cloned().unwrap_or(CanonicalValue::Null),
        );
    }
    let framed = canonical_artifact_payload(
        "EnvironmentManifest",
        1,
        &CanonicalValue::Object(payload),
    )
    .expect("constant artifact type");
    let digest = sha256_hex(framed.as_bytes());
    EnvironmentManifest { inputs: input, digest }
}

/// Bounded deterministic delta between two environment manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentDelta {
    /// Base environment digest.
    pub base_digest: String,
    /// Result environment digest.
    pub result_digest: String,
    /// Sections whose canonical form differs.
    pub changed: Vec<String>,
    /// Sections whose canonical form matches.
    pub unchanged: Vec<String>,
    /// True when nothing differs.
    pub unchanged_content: bool,
}

/// Compute the bounded delta over the thirteen declared sections.
#[must_use]
pub fn compute_environment_delta(
    base: &EnvironmentManifest,
    result: &EnvironmentManifest,
) -> EnvironmentDelta {
    let base_sections = environment_sections(base);
    let result_sections = environment_sections(result);
    let empty = CanonicalValue::Null;
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    for section in ENVIRONMENT_SECTIONS {
        let base_value = base_sections.get(section).unwrap_or(&empty);
        let result_value = result_sections.get(section).unwrap_or(&empty);
        if canonicalize(base_value) == canonicalize(result_value) {
            unchanged.push((*section).to_owned());
        } else {
            changed.push((*section).to_owned());
        }
    }
    let unchanged_content = changed.is_empty();
    EnvironmentDelta {
        base_digest: base.digest.clone(),
        result_digest: result.digest.clone(),
        changed,
        unchanged,
        unchanged_content,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EnvironmentManifestInput, compute_environment_delta,
        create_environment_manifest,
    };

    fn input() -> EnvironmentManifestInput {
        EnvironmentManifestInput {
            siralos_version: Some("0.1.0".to_owned()),
            platform: Some("win32".to_owned()),
            locale_policy: Some("en-US".to_owned()),
            timezone_policy: Some("UTC".to_owned()),
            environment_allowlist: vec!["PATH".to_owned(), "HOME".to_owned()],
            ..EnvironmentManifestInput::default()
        }
    }

    #[test]
    fn allowlist_and_tools_sort_deterministically() {
        let mut manifest = create_environment_manifest(input());
        assert_eq!(
            manifest.inputs.environment_allowlist,
            vec!["HOME".to_owned(), "PATH".to_owned()]
        );
        let first = manifest.digest.clone();
        manifest = create_environment_manifest(input());
        assert_eq!(manifest.digest, first);
    }

    #[test]
    fn digest_is_content_sensitive() {
        let base = create_environment_manifest(input());
        let mut changed_input = input();
        changed_input.node_version = Some("22.0.0".to_owned());
        let changed = create_environment_manifest(changed_input);
        assert_ne!(base.digest, changed.digest);
    }

    #[test]
    fn delta_reports_changed_and_unchanged_sections_in_order() {
        let base = create_environment_manifest(input());
        let mut changed_input = input();
        changed_input.node_version = Some("22.0.0".to_owned());
        let result = create_environment_manifest(changed_input);
        let delta = compute_environment_delta(&base, &result);
        assert_eq!(delta.changed, vec!["nodeVersion"]);
        assert_eq!(delta.unchanged.len(), 12);
        assert!(!delta.unchanged_content);
        let identical = compute_environment_delta(&base, &base);
        assert!(identical.unchanged_content);
        assert_eq!(identical.changed.len(), 0);
        assert_eq!(identical.unchanged.len(), 13);
    }
}
