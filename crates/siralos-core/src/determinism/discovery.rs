//! Deterministic repository discovery and ownership resolution
//! (Stage 3 — Deterministic Execution & Reproducibility, ADR 0029;
//! R10a H2).
//!
//! Mirrors `packages/core/src/determinism/discovery.ts`. Baseline
//! discovery is repeatable: search → classify relevance → stable
//! rank/order → apply bounds. Same task + same repository state → same
//! result; filesystem enumeration order never decides the baseline.
//! The model may request additional exploration, but the baseline is
//! host-owned.

use super::ports::stable_sort_by_key;
use crate::identity::{
    ArtifactIdentityError, CanonicalValue, compute_artifact_digest,
};
use std::collections::BTreeMap;

/// Relevance class of one discovery candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryRelevance {
    /// Host-verified relevance.
    Verified,
    /// Candidate (unverified).
    Candidate,
}

impl DiscoveryRelevance {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Candidate => "candidate",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "verified" => Some(Self::Verified),
            "candidate" => Some(Self::Candidate),
            _ => None,
        }
    }
}

/// One discovery candidate before ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryCandidateInput {
    /// Workspace-relative path.
    pub path: String,
    /// Declared relevance.
    pub relevance: DiscoveryRelevance,
    /// Bounded classification evidence.
    pub evidence: Vec<String>,
}

/// One ranked candidate in a discovery result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryCandidate {
    /// Workspace-relative path.
    pub path: String,
    /// Relevance class.
    pub relevance: DiscoveryRelevance,
    /// Bounded classification evidence.
    pub evidence: Vec<String>,
    /// Rank signal: lower is more relevant (host-derived, never a model
    /// claim).
    pub rank: u32,
}

/// Deterministic discovery result with its digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryResult {
    /// Candidates in canonical order after bounding.
    pub candidates: Vec<DiscoveryCandidate>,
    /// Digest over the ordered candidates (`DiscoveryResult` v1).
    pub digest: String,
}

/// Inputs for [`discover_repository`].
pub struct DiscoveryInput<'a> {
    /// Unordered candidates.
    pub unordered_candidates: &'a [DiscoveryCandidateInput],
    /// Maximum candidates after bounding.
    pub max_candidates: usize,
    /// Normalized workspace-relative target paths of the task.
    pub task_targets: &'a [String],
}

const STRONG_EVIDENCE: [&str; 5] = [
    "exact task target",
    "direct import",
    "structural relationship",
    "explicit architecture ownership",
    "test mapping",
];

/// Deterministic baseline discovery: exact task targets rank first
/// (rank 0), strong-evidence/verified rank 1, everything else rank 2;
/// ties break by canonical path; bounds apply last. Shuffled input
/// order cannot change the result.
pub fn discover_repository(
    input: &DiscoveryInput<'_>,
) -> Result<DiscoveryResult, ArtifactIdentityError> {
    let ranked: Vec<DiscoveryCandidate> = input
        .unordered_candidates
        .iter()
        .map(|candidate| {
            let exact_target = input.task_targets.contains(&candidate.path);
            let strong_evidence = candidate.relevance
                == DiscoveryRelevance::Verified
                || candidate
                    .evidence
                    .iter()
                    .any(|item| STRONG_EVIDENCE.contains(&item.as_str()));
            let rank = if exact_target {
                0
            } else if strong_evidence {
                1
            } else {
                2
            };
            DiscoveryCandidate {
                path: candidate.path.clone(),
                relevance: candidate.relevance,
                evidence: candidate.evidence.clone(),
                rank,
            }
        })
        .collect();
    let ordered = stable_sort_by_key(&ranked, |candidate| {
        format!("{:04}:{}", candidate.rank, candidate.path)
    });
    let bounded: Vec<DiscoveryCandidate> =
        ordered.into_iter().take(input.max_candidates).collect();
    let candidate_values: Vec<CanonicalValue> = bounded
        .iter()
        .map(|candidate| {
            CanonicalValue::Object(BTreeMap::from([
                (
                    "path".to_owned(),
                    CanonicalValue::Str(candidate.path.clone()),
                ),
                (
                    "relevance".to_owned(),
                    CanonicalValue::Str(
                        candidate.relevance.as_str().to_owned(),
                    ),
                ),
                (
                    "rank".to_owned(),
                    CanonicalValue::U64(u64::from(candidate.rank)),
                ),
                (
                    "evidence".to_owned(),
                    CanonicalValue::Array(
                        candidate
                            .evidence
                            .iter()
                            .map(|item| CanonicalValue::Str(item.clone()))
                            .collect(),
                    ),
                ),
            ]))
        })
        .collect();
    let digest = compute_artifact_digest(
        "DiscoveryResult",
        1,
        &CanonicalValue::Object(BTreeMap::from([(
            "candidates".to_owned(),
            CanonicalValue::Array(candidate_values),
        )])),
    )?
    .value;
    Ok(DiscoveryResult { candidates: bounded, digest })
}

#[cfg(test)]
mod tests {
    use super::{
        DiscoveryCandidateInput, DiscoveryInput, DiscoveryRelevance,
        discover_repository,
    };
    use crate::determinism::{list_ownership, resolve_owner};

    fn candidate(
        path: &str,
        relevance: DiscoveryRelevance,
    ) -> DiscoveryCandidateInput {
        DiscoveryCandidateInput {
            path: path.to_owned(),
            relevance,
            evidence: Vec::new(),
        }
    }

    #[test]
    fn exact_targets_rank_first_and_ties_break_by_path() {
        let candidates = [
            candidate("res://z.tscn", DiscoveryRelevance::Verified),
            candidate("res://a.gd", DiscoveryRelevance::Candidate),
            candidate("res://player.gd", DiscoveryRelevance::Verified),
        ];
        let targets = vec!["res://player.gd".to_owned()];
        let result = discover_repository(&DiscoveryInput {
            unordered_candidates: &candidates,
            max_candidates: 10,
            task_targets: &targets,
        })
        .expect("discovers");
        // Exact target first (rank 0), then verified (rank 1), then
        // candidate (rank 2); ties break by path.
        assert_eq!(
            result
                .candidates
                .iter()
                .map(|c| c.path.as_str())
                .collect::<Vec<_>>(),
            vec!["res://player.gd", "res://z.tscn", "res://a.gd"]
        );
        assert_eq!(result.candidates[0].rank, 0);
        assert_eq!(result.digest.len(), 64);
    }

    #[test]
    fn bounds_apply_after_ordering() {
        let candidates = [
            candidate("res://a.gd", DiscoveryRelevance::Verified),
            candidate("res://b.gd", DiscoveryRelevance::Candidate),
        ];
        let result = discover_repository(&DiscoveryInput {
            unordered_candidates: &candidates,
            max_candidates: 1,
            task_targets: &[],
        })
        .expect("discovers");
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].path, "res://a.gd");
    }

    #[test]
    fn ownership_resolves_exact_and_overlap_aliases() {
        let exact = resolve_owner("tool projection").expect("exact match");
        assert_eq!(exact.owner, "ToolProjector");
        let overlap = resolve_owner("hashing").expect("overlap alias");
        assert_eq!(overlap.owner, "ArtifactDigest");
        assert!(resolve_owner("nonexistent responsibility").is_none());
        let listed = list_ownership();
        assert!(listed.len() >= 18);
        assert!(
            listed
                .windows(2)
                .all(|pair| pair[0].responsibility <= pair[1].responsibility)
        );
    }
}
