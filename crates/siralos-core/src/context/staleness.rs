//! Targeted incremental staleness (Stage 3 — Interpretable Context
//! Architecture, ADR 0030; R10b ICM).
//!
//! Mirrors `packages/core/src/context/staleness.ts`. Staleness
//! propagates ONLY along explicit dependency manifests: unrelated
//! repository changes never stale unrelated artifacts. A change to one
//! input invalidates the affected derived artifact (and only its
//! explicit downstream path); recomputation stays with the workflow
//! owners — this is not an incremental build system.

use std::collections::{BTreeMap, BTreeSet};

use super::artifacts::ArtifactDependencyManifest;
use super::{ContextError, context_error};
use crate::determinism::{SourceRevision, stable_sort_by_key};
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

/// Inputs for [`derive_artifact_staleness`].
pub struct ArtifactStalenessInput<'a> {
    /// Dependency manifests of the derived artifacts under
    /// consideration.
    pub manifests: &'a [ArtifactDependencyManifest],
    /// Current observable input digests keyed by input artifact type.
    pub current_input_digests: &'a BTreeMap<String, String>,
}

/// Targeted staleness result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArtifactStalenessResult {
    /// artifactId → staleness reason (only artifacts whose dependencies
    /// changed).
    pub stale: BTreeMap<String, String>,
    /// Artifact ids verified current against their dependencies.
    pub current: Vec<String>,
    /// Input types that changed but are inputs of no considered
    /// artifact.
    pub unrelated_changes: Vec<String>,
}

fn abbreviate(value: &str) -> String {
    value.chars().take(8).collect()
}

/// Deterministic targeted staleness: for each manifest, compare each
/// recorded dependency digest to the CURRENT digest of that input type.
/// A mismatch marks the artifact stale with an explicit reason. Input
/// types consumed by no manifest are reported as unrelated changes.
pub fn derive_artifact_staleness(
    input: &ArtifactStalenessInput<'_>,
) -> ArtifactStalenessResult {
    let mut consumed = BTreeSet::new();
    for manifest in input.manifests {
        for dependency in &manifest.depends_on {
            consumed.insert(dependency.artifact_type.clone());
        }
    }
    let unrelated_changes: Vec<String> = input
        .current_input_digests
        .keys()
        .filter(|artifact_type| !consumed.contains(*artifact_type))
        .cloned()
        .collect();
    let mut result = ArtifactStalenessResult::default();
    let ordered =
        stable_sort_by_key(input.manifests, |entry| entry.artifact_id.clone());
    for manifest in ordered {
        let mut changed: Vec<String> = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        for dependency in &manifest.depends_on {
            match input.current_input_digests.get(&dependency.artifact_type) {
                None => missing.push(format!(
                    "{}@{}",
                    dependency.artifact_type,
                    abbreviate(&dependency.digest)
                )),
                Some(current_digest)
                    if current_digest != &dependency.digest =>
                {
                    changed.push(format!(
                        "{} {} -> {}",
                        dependency.artifact_type,
                        abbreviate(&dependency.digest),
                        abbreviate(current_digest)
                    ));
                }
                Some(_) => {}
            }
        }
        if changed.is_empty() && missing.is_empty() {
            result.current.push(manifest.artifact_id.clone());
        } else {
            let mut reasons: Vec<String> = Vec::new();
            reasons.extend(changed);
            for entry in missing {
                reasons.push(format!("{entry} no longer observable"));
            }
            result.stale.insert(
                manifest.artifact_id.clone(),
                format!("stale because {}", reasons.join("; ")),
            );
        }
    }
    result.current = stable_sort_by_key(&result.current, |id| id.clone());
    result.unrelated_changes =
        stable_sort_by_key(&unrelated_changes, |key| key.clone());
    result
}

/// Prepared-mutation staleness outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedMutationStaleness {
    /// True when any prepared source revision is no longer current.
    pub stale: bool,
    /// Prepared paths whose source revision changed or disappeared.
    pub stale_paths: Vec<String>,
}

/// Prepared-mutation staleness: a prepared mutation binds exact source
/// revisions; a source revision change makes the prepared mutation
/// stale (no automatic mutation retry under the old preparation).
pub fn is_prepared_mutation_stale(
    prepared_source_revisions: &[SourceRevision],
    current_source_revisions: &BTreeMap<String, String>,
) -> PreparedMutationStaleness {
    let mut stale_paths = Vec::new();
    for prepared in prepared_source_revisions {
        let current = current_source_revisions.get(&prepared.path);
        if current.is_none_or(|revision| revision != &prepared.revision) {
            stale_paths.push(prepared.path.clone());
        }
    }
    PreparedMutationStaleness { stale: !stale_paths.is_empty(), stale_paths }
}

/// Deterministic digest of a staleness result (`ArtifactStalenessResult`
/// v1; evidence identity only — a digest never implies trust).
pub fn compute_staleness_digest(
    result: &ArtifactStalenessResult,
) -> Result<String, ContextError> {
    let stale = CanonicalValue::Array(
        result
            .stale
            .iter()
            .map(|(id, reason)| {
                object(vec![
                    ("id", string_value(id)),
                    ("reason", string_value(reason)),
                ])
            })
            .collect(),
    );
    let current = CanonicalValue::Array(
        result.current.iter().map(|id| string_value(id)).collect(),
    );
    let unrelated = CanonicalValue::Array(
        result.unrelated_changes.iter().map(|key| string_value(key)).collect(),
    );
    let payload = object(vec![
        ("stale", stale),
        ("current", current),
        ("unrelatedChanges", unrelated),
    ]);
    let digest =
        compute_artifact_digest("ArtifactStalenessResult", 1, &payload)
            .map_err(|error| context_error(error.message))?;
    Ok(digest.value)
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactDependencyManifest, ArtifactStalenessInput,
        PreparedMutationStaleness, compute_staleness_digest,
        derive_artifact_staleness, is_prepared_mutation_stale,
    };
    use crate::context::artifacts::ArtifactDependency;
    use crate::determinism::SourceRevision;
    use std::collections::BTreeMap;

    fn digest(character: char) -> String {
        std::iter::repeat_n(character, 64).collect()
    }

    fn manifest(
        artifact_id: &str,
        dependencies: &[(&str, String)],
    ) -> ArtifactDependencyManifest {
        crate::context::create_artifact_dependency_manifest(
            "Derived",
            artifact_id,
            &dependencies
                .iter()
                .map(|(artifact_type, value)| ArtifactDependency {
                    artifact_type: (*artifact_type).to_owned(),
                    digest: value.clone(),
                })
                .collect::<Vec<_>>(),
        )
        .expect("valid manifest")
    }

    #[test]
    fn staleness_is_targeted_and_content_addressed() {
        let task_before = digest('a');
        let task_current = digest('f');
        let guidance = digest('b');
        let review = digest('c');
        let manifests = vec![
            manifest(
                "zeta.artifact",
                &[("TaskContract", task_before.clone())],
            ),
            manifest(
                "alpha.artifact",
                &[
                    ("TaskContract", task_before.clone()),
                    ("GuidanceManifest", guidance),
                ],
            ),
            manifest("mike.artifact", &[("ReviewVerdict", review.clone())]),
        ];
        let mut current = BTreeMap::new();
        current.insert("TaskContract".to_owned(), task_current.clone());
        current.insert("ReviewVerdict".to_owned(), review);
        current.insert("Unrelated".to_owned(), digest('e'));
        let result = derive_artifact_staleness(&ArtifactStalenessInput {
            manifests: &manifests,
            current_input_digests: &current,
        });
        assert_eq!(
            result.stale.get("alpha.artifact"),
            Some(&format!(
                "stale because TaskContract {} -> {}; GuidanceManifest@{} no longer observable",
                &task_before[..8],
                &task_current[..8],
                &digest('b')[..8]
            ))
        );
        assert_eq!(
            result.stale.get("zeta.artifact"),
            Some(&format!(
                "stale because TaskContract {0} -> {1}",
                &task_before[..8],
                &task_current[..8]
            ))
        );
        // Processed in artifact-id order; the unchanged dependency
        // keeps `mike.artifact` current.
        assert_eq!(result.current, vec!["mike.artifact"]);
        assert_eq!(result.unrelated_changes, vec!["Unrelated"]);
    }

    #[test]
    fn staleness_digest_is_stable_and_content_sensitive() {
        let manifests =
            vec![manifest("only", &[("TaskContract", digest('a'))])];
        let mut current = BTreeMap::new();
        current.insert("TaskContract".to_owned(), digest('b'));
        let result = derive_artifact_staleness(&ArtifactStalenessInput {
            manifests: &manifests,
            current_input_digests: &current,
        });
        let first = compute_staleness_digest(&result).expect("digest");
        assert_eq!(compute_staleness_digest(&result), Ok(first.clone()));
        assert_eq!(first.len(), 64);
        let mut quiet = BTreeMap::new();
        quiet.insert("TaskContract".to_owned(), digest('a'));
        let calm = derive_artifact_staleness(&ArtifactStalenessInput {
            manifests: &manifests,
            current_input_digests: &quiet,
        });
        let calm_digest = compute_staleness_digest(&calm).expect("digest");
        assert_ne!(calm_digest, first);
    }

    #[test]
    fn prepared_mutations_go_stale_on_revision_change_or_loss() {
        let prepared = vec![
            SourceRevision {
                path: "src/a.rs".to_owned(),
                revision: "r1".to_owned(),
            },
            SourceRevision {
                path: "src/b.txt".to_owned(),
                revision: "r2".to_owned(),
            },
        ];
        let mut current = BTreeMap::new();
        current.insert("src/a.rs".to_owned(), "r1".to_owned());
        assert_eq!(
            is_prepared_mutation_stale(&prepared, &current),
            PreparedMutationStaleness {
                stale: true,
                stale_paths: vec!["src/b.txt".to_owned()],
            }
        );
        current.insert("src/a.rs".to_owned(), "r9".to_owned());
        current.insert("src/b.txt".to_owned(), "r2".to_owned());
        assert_eq!(
            is_prepared_mutation_stale(&prepared, &current),
            PreparedMutationStaleness {
                stale: true,
                stale_paths: vec!["src/a.rs".to_owned()],
            }
        );
        current.insert("src/a.rs".to_owned(), "r1".to_owned());
        assert_eq!(
            is_prepared_mutation_stale(&prepared, &current),
            PreparedMutationStaleness { stale: false, stale_paths: vec![] }
        );
    }
}
