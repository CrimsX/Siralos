//! Workflow artifact dependency manifests (Stage 3 — Interpretable
//! Context Architecture, ADR 0030; R10b ICM).
//!
//! Dependency-manifest subset of
//! `packages/core/src/context/artifacts.ts`: major phases communicate
//! through typed, digest-bound artifacts rather than conversation
//! history. Dependency manifests record only high-value explicit
//! dependencies (H1 digests). Envelope identity and bounded lineage are
//! not exercised by any wired differential subject yet and stay
//! unported.

use std::collections::BTreeMap;

use super::{ContextError, context_error};
use crate::determinism::stable_sort_by_key;
use crate::identity::{CanonicalValue, compute_artifact_digest};

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

/// One explicit content-addressed dependency (`artifactType` + digest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactDependency {
    /// Artifact type of the dependency.
    pub artifact_type: String,
    /// Exact content digest of the observed dependency.
    pub digest: String,
}

/// Digest-bound manifest of one derived artifact's explicit inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactDependencyManifest {
    /// Artifact type this manifest describes.
    pub artifact_type: String,
    /// Instance id of the derived artifact.
    pub artifact_id: String,
    /// Explicit dependencies in canonical (`type:digest`) order.
    pub depends_on: Vec<ArtifactDependency>,
    /// Domain-separated manifest digest (`…ArtifactDependencyManifest:v1`).
    pub digest: String,
}

/// Create a digest-bound dependency manifest. Dependencies are ordered
/// canonically; the declared order never affects the digest.
pub fn create_artifact_dependency_manifest(
    artifact_type: &str,
    artifact_id: &str,
    depends_on: &[ArtifactDependency],
) -> Result<ArtifactDependencyManifest, ContextError> {
    if artifact_type.is_empty() || artifact_id.is_empty() {
        return Err(context_error(
            "A dependency manifest requires an artifact type and id.",
        ));
    }
    if depends_on.is_empty() {
        return Err(context_error(format!(
            "A dependency manifest for {artifact_type} requires at least one dependency."
        )));
    }
    let ordered = stable_sort_by_key(depends_on, |entry| {
        format!("{}:{}", entry.artifact_type, entry.digest)
    });
    let payload = object(vec![
        ("artifactType", string_value(artifact_type)),
        ("artifactId", string_value(artifact_id)),
        (
            "dependsOn",
            CanonicalValue::Array(
                ordered
                    .iter()
                    .map(|entry| {
                        object(vec![
                            (
                                "artifactType",
                                string_value(&entry.artifact_type),
                            ),
                            ("digest", string_value(&entry.digest)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]);
    let digest =
        compute_artifact_digest("ArtifactDependencyManifest", 1, &payload)
            .map_err(|error| ContextError { message: error.message })?
            .value;
    Ok(ArtifactDependencyManifest {
        artifact_type: artifact_type.to_owned(),
        artifact_id: artifact_id.to_owned(),
        depends_on: ordered,
        digest,
    })
}

/// One entry of the high-value dependency table: `input` names the key
/// in a caller's current-digest map that supplies the dependency's
/// exact digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighValueDependency {
    /// Artifact type of the dependency.
    pub artifact_type: &'static str,
    /// Current-digest map key for this dependency.
    pub input: &'static str,
}

/// Deterministic dependency tables for the known high-value artifacts.
/// Each table lists ONLY the explicit inputs that materially affect the
/// derived artifact.
#[must_use]
pub fn high_value_dependencies(
    artifact_type: &str,
) -> Option<&'static [HighValueDependency]> {
    const TASK_PLAN: [HighValueDependency; 3] = [
        HighValueDependency {
            artifact_type: "TaskContract",
            input: "taskContractDigest",
        },
        HighValueDependency {
            artifact_type: "GuidanceManifest",
            input: "guidanceDigest",
        },
        HighValueDependency {
            artifact_type: "SourceRevisions",
            input: "verifiedSourceRevisions",
        },
    ];
    const REVIEW_VERDICT: [HighValueDependency; 4] = [
        HighValueDependency {
            artifact_type: "TaskContract",
            input: "taskContractDigest",
        },
        HighValueDependency {
            artifact_type: "Changeset",
            input: "changesetDigest",
        },
        HighValueDependency {
            artifact_type: "ReviewContextManifest",
            input: "reviewContextDigest",
        },
        HighValueDependency {
            artifact_type: "ValidationEvidence",
            input: "validationEvidenceDigest",
        },
    ];
    const ACCEPTANCE_RESULT: [HighValueDependency; 4] = [
        HighValueDependency {
            artifact_type: "AcceptanceCriteria",
            input: "acceptanceDigest",
        },
        HighValueDependency {
            artifact_type: "ValidationEvidence",
            input: "validationEvidenceDigest",
        },
        HighValueDependency {
            artifact_type: "ReviewVerdict",
            input: "reviewVerdictDigest",
        },
        HighValueDependency {
            artifact_type: "MutationVerificationEvidence",
            input: "mutationVerificationDigest",
        },
    ];
    const PREPARED_CHANGESET: [HighValueDependency; 2] = [
        HighValueDependency {
            artifact_type: "TaskPlan",
            input: "taskPlanDigest",
        },
        HighValueDependency {
            artifact_type: "SourceRevisions",
            input: "sourceRevisionDigests",
        },
    ];
    match artifact_type {
        "TaskPlan" => Some(&TASK_PLAN),
        "ReviewVerdict" => Some(&REVIEW_VERDICT),
        "AcceptanceResult" => Some(&ACCEPTANCE_RESULT),
        "PreparedChangeset" => Some(&PREPARED_CHANGESET),
        _ => None,
    }
}

/// Build the dependency manifest for a known artifact from its inputs.
/// Unknown artifact types and fully-empty input maps produce `None`.
/// A `None` map value (JSON null) means the input is unobserved and is
/// skipped, exactly like an absent key.
pub fn build_dependency_manifest(
    artifact_type: &str,
    artifact_id: &str,
    current_digests: &BTreeMap<String, Option<String>>,
) -> Result<Option<ArtifactDependencyManifest>, ContextError> {
    let Some(table) = high_value_dependencies(artifact_type) else {
        return Ok(None);
    };
    let mut depends_on = Vec::new();
    for entry in table {
        let digest = match current_digests.get(entry.input) {
            Some(Some(digest)) => digest.clone(),
            _ => continue,
        };
        depends_on.push(ArtifactDependency {
            artifact_type: entry.artifact_type.to_owned(),
            digest,
        });
    }
    if depends_on.is_empty() {
        return Ok(None);
    }
    create_artifact_dependency_manifest(
        artifact_type,
        artifact_id,
        &depends_on,
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactDependency, build_dependency_manifest,
        create_artifact_dependency_manifest, high_value_dependencies,
    };
    use crate::context::{ContextError, context_error};
    use std::collections::BTreeMap;

    fn dependency(artifact_type: &str, digest: &str) -> ArtifactDependency {
        ArtifactDependency {
            artifact_type: artifact_type.to_owned(),
            digest: digest.to_owned(),
        }
    }

    #[test]
    fn manifests_order_dependencies_canonically() {
        let manifest = create_artifact_dependency_manifest(
            "TaskPlan",
            "plan-1",
            &[
                dependency("SourceRevisions", "c".repeat(64).as_str()),
                dependency("GuidanceManifest", "a".repeat(64).as_str()),
                dependency("TaskContract", "b".repeat(64).as_str()),
            ],
        )
        .expect("valid dependencies");
        let types: Vec<&str> = manifest
            .depends_on
            .iter()
            .map(|entry| entry.artifact_type.as_str())
            .collect();
        // Canonical `type:digest` order, not declaration order.
        assert_eq!(
            types,
            ["GuidanceManifest", "SourceRevisions", "TaskContract"]
        );
        assert_eq!(manifest.digest.len(), 64);
        let reordered = create_artifact_dependency_manifest(
            "TaskPlan",
            "plan-1",
            &[
                dependency("TaskContract", "b".repeat(64).as_str()),
                dependency("SourceRevisions", "c".repeat(64).as_str()),
                dependency("GuidanceManifest", "a".repeat(64).as_str()),
            ],
        )
        .expect("valid dependencies");
        assert_eq!(reordered.digest, manifest.digest);
    }

    #[test]
    fn manifest_creation_rejects_empty_inputs_with_oracle_messages() {
        assert_eq!(
            create_artifact_dependency_manifest("", "plan-1", &[]),
            Err(context_error(
                "A dependency manifest requires an artifact type and id."
            ))
        );
        assert_eq!(
            create_artifact_dependency_manifest(
                "TaskPlan",
                "",
                &[dependency("TaskContract", "d")]
            ),
            Err(context_error(
                "A dependency manifest requires an artifact type and id."
            ))
        );
        assert_eq!(
            create_artifact_dependency_manifest("TaskPlan", "plan-1", &[]),
            Err(ContextError {
                message:
                    "A dependency manifest for TaskPlan requires at least one dependency."
                        .to_owned()
            })
        );
    }

    #[test]
    fn build_uses_the_high_value_table_and_skips_unobserved_inputs() {
        assert!(high_value_dependencies("Nope").is_none());
        let mut digests: BTreeMap<String, Option<String>> = BTreeMap::new();
        digests.insert("taskContractDigest".to_owned(), Some("b".repeat(64)));
        digests.insert("guidanceDigest".to_owned(), None);
        digests.insert(
            "verifiedSourceRevisions".to_owned(),
            Some("c".repeat(64)),
        );
        let built = build_dependency_manifest("TaskPlan", "p1", &digests)
            .expect("buildable")
            .expect("observed inputs exist");
        assert_eq!(built.depends_on.len(), 2);
        let unknown = build_dependency_manifest("Mystery", "m1", &digests)
            .expect("buildable");
        assert_eq!(unknown, None);
        // ReviewVerdict consumes only the shared contract digest here;
        // its other three inputs stay unobserved.
        let partial =
            build_dependency_manifest("ReviewVerdict", "r1", &digests)
                .expect("buildable")
                .expect("the contract digest is observed");
        assert_eq!(partial.depends_on.len(), 1);
        assert_eq!(partial.depends_on[0].artifact_type, "TaskContract");
        assert_eq!(
            build_dependency_manifest(
                "PreparedChangeset",
                "pc1",
                &BTreeMap::new()
            ),
            Ok(None)
        );
    }
}
