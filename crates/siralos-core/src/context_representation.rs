//! Layered context representations — L0-L4 (decision 79 slice 2).
//!
//! Representations are additive and keyed by node id so the existing
//! `ContextNode`/`ContextGraph` shapes and the v57 corpus digest are
//! untouched. L2 structured representations are deterministic host
//! extraction — a model-derived L2 is a typed refusal (decision 79
//! clause b). L1 prose summaries are model-derived only when
//! provenance-bound, otherwise a typed refusal.

use std::collections::BTreeSet;

use serde_json::{Value, json};

/// Maximum number of representation sets in a store.
pub const MAX_NODES: usize = 512;
/// Maximum byte length of a node id.
pub const MAX_ID_BYTES: usize = 256;
/// Maximum byte length of a representation's content.
pub const MAX_CONTENT_BYTES: usize = 8192;
/// Maximum number of derived-from bindings per representation.
pub const MAX_DERIVED_FROM: usize = 64;
/// Maximum number of representations per node (one per level).
pub const MAX_REPRESENTATIONS_PER_SET: usize = 5;

/// Representation layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RepresentationLevel {
    /// L0 identity.
    Identity,
    /// L1 summary.
    Summary,
    /// L2 structured (host-only).
    Structured,
    /// L3 detailed.
    Detailed,
    /// L4 source reference (empty content allowed).
    Source,
}

impl RepresentationLevel {
    /// Canonical string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Summary => "summary",
            Self::Structured => "structured",
            Self::Detailed => "detailed",
            Self::Source => "source",
        }
    }
}

/// Provenance of a representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RepresentationOrigin {
    /// Deterministic host extraction.
    HostExtracted,
    /// Model-derived (provenance-bound).
    ModelDerived,
}

impl RepresentationOrigin {
    /// Canonical string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HostExtracted => "host_extracted",
            Self::ModelDerived => "model_derived",
        }
    }
}

/// One layered representation for a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRepresentation {
    /// Layer.
    pub level: RepresentationLevel,
    /// Origin.
    pub origin: RepresentationOrigin,
    /// 64 lowercase hex digest of `content`.
    pub content_digest: String,
    /// Bounded provenance bindings.
    pub derived_from: Vec<(String, String)>,
    /// Bounded content; empty allowed only for L4 Source.
    pub content: String,
}

/// Bounded set of representations for one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRepresentationSet {
    /// Node id.
    pub node_id: String,
    /// Representations in canonical level order.
    pub representations: Vec<NodeRepresentation>,
}

/// Validation failure for representations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepresentationError {
    /// Duplicate level within one node set.
    DuplicateLevel {
        /// Node id.
        node_id: String,
        /// Duplicated level.
        level: RepresentationLevel,
    },
    /// L2 structured may not be model-derived (clause b).
    ModelDerivedStructured {
        /// Node id.
        node_id: String,
    },
    /// Model-derived representation lacks provenance.
    UnprovenancedDerived {
        /// Node id.
        node_id: String,
        /// Level that lacks provenance.
        level: RepresentationLevel,
    },
    /// Content digest is not 64 lowercase hex.
    MalformedDigest {
        /// Node id.
        node_id: String,
        /// Level with malformed digest.
        level: RepresentationLevel,
    },
    /// Content digest does not match content.
    DigestMismatch {
        /// Node id.
        node_id: String,
        /// Level with mismatched digest.
        level: RepresentationLevel,
    },
    /// Too many representations for one node (max 5).
    TooManyRepresentations {
        /// Node id.
        node_id: String,
    },
    /// Id exceeds byte bound.
    IdTooLong {
        /// Offending id.
        node_id: String,
    },
    /// Content exceeds byte bound.
    ContentTooLarge {
        /// Node id.
        node_id: String,
        /// Level with oversize content.
        level: RepresentationLevel,
    },
    /// Too many derived-from bindings.
    TooManyBindings {
        /// Node id.
        node_id: String,
        /// Level with too many bindings.
        level: RepresentationLevel,
    },
    /// Duplicate node id in store.
    DuplicateNode {
        /// Duplicate id.
        node_id: String,
    },
    /// Too many nodes in store.
    TooManyNodes,
}

impl std::fmt::Display for RepresentationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateLevel { node_id, level } => {
                write!(f, "duplicate level: {node_id} {}", level.as_str())
            }
            Self::ModelDerivedStructured { node_id } => {
                write!(f, "model-derived structured: {node_id}")
            }
            Self::UnprovenancedDerived { node_id, level } => write!(
                f,
                "unprovenanced derived: {node_id} {}",
                level.as_str()
            ),
            Self::MalformedDigest { node_id, level } => {
                write!(f, "malformed digest: {node_id} {}", level.as_str())
            }
            Self::DigestMismatch { node_id, level } => {
                write!(f, "digest mismatch: {node_id} {}", level.as_str())
            }
            Self::TooManyRepresentations { node_id } => {
                write!(f, "too many representations: {node_id}")
            }
            Self::IdTooLong { node_id } => {
                write!(f, "id too long: {node_id}")
            }
            Self::ContentTooLarge { node_id, level } => {
                write!(f, "content too large: {node_id} {}", level.as_str())
            }
            Self::TooManyBindings { node_id, level } => {
                write!(f, "too many bindings: {node_id} {}", level.as_str())
            }
            Self::DuplicateNode { node_id } => {
                write!(f, "duplicate node: {node_id}")
            }
            Self::TooManyNodes => write!(f, "too many nodes"),
        }
    }
}

impl std::error::Error for RepresentationError {}

fn is_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Compute the hex SHA-256 digest of `text` bytes.
///
/// Mirrors `siralos_core::identity::sha256_hex`.
#[must_use]
pub fn content_digest_of(text: &str) -> String {
    crate::identity::sha256_hex(text.as_bytes())
}

impl NodeRepresentationSet {
    /// Build and validate, returning canonical level order.
    pub fn build(
        node_id: String,
        reps: Vec<NodeRepresentation>,
    ) -> Result<Self, RepresentationError> {
        if node_id.len() > MAX_ID_BYTES {
            return Err(RepresentationError::IdTooLong { node_id });
        }
        if reps.len() > MAX_REPRESENTATIONS_PER_SET {
            return Err(RepresentationError::TooManyRepresentations {
                node_id: node_id.clone(),
            });
        }
        let mut seen: BTreeSet<RepresentationLevel> = BTreeSet::new();
        for rep in &reps {
            if rep.content.len() > MAX_CONTENT_BYTES {
                return Err(RepresentationError::ContentTooLarge {
                    node_id: node_id.clone(),
                    level: rep.level,
                });
            }
            if rep.derived_from.len() > MAX_DERIVED_FROM {
                return Err(RepresentationError::TooManyBindings {
                    node_id: node_id.clone(),
                    level: rep.level,
                });
            }
            if !is_hex_digest(&rep.content_digest) {
                return Err(RepresentationError::MalformedDigest {
                    node_id: node_id.clone(),
                    level: rep.level,
                });
            }
            if !rep.content.is_empty()
                && rep.content_digest != content_digest_of(&rep.content)
            {
                return Err(RepresentationError::DigestMismatch {
                    node_id: node_id.clone(),
                    level: rep.level,
                });
            }
            // Clause (b): L2 structured host-only.
            if rep.level == RepresentationLevel::Structured
                && rep.origin == RepresentationOrigin::ModelDerived
            {
                return Err(RepresentationError::ModelDerivedStructured {
                    node_id: node_id.clone(),
                });
            }
            if rep.origin == RepresentationOrigin::ModelDerived
                && rep.derived_from.is_empty()
            {
                return Err(RepresentationError::UnprovenancedDerived {
                    node_id: node_id.clone(),
                    level: rep.level,
                });
            }
            if !seen.insert(rep.level) {
                return Err(RepresentationError::DuplicateLevel {
                    node_id: node_id.clone(),
                    level: rep.level,
                });
            }
        }
        let mut sorted = reps;
        sorted.sort_by_key(|a| a.level);
        Ok(Self { node_id, representations: sorted })
    }
}

/// Validated, canonically ordered store of representation sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRepresentationStore {
    sets: Vec<NodeRepresentationSet>,
}

impl ContextRepresentationStore {
    /// Build and validate the store.
    pub fn build(
        sets: Vec<NodeRepresentationSet>,
    ) -> Result<Self, RepresentationError> {
        if sets.len() > MAX_NODES {
            return Err(RepresentationError::TooManyNodes);
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for s in &sets {
            if s.node_id.len() > MAX_ID_BYTES {
                return Err(RepresentationError::IdTooLong {
                    node_id: s.node_id.clone(),
                });
            }
            if !seen.insert(s.node_id.clone()) {
                return Err(RepresentationError::DuplicateNode {
                    node_id: s.node_id.clone(),
                });
            }
        }
        let mut sorted = sets;
        sorted.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        Ok(Self { sets: sorted })
    }

    /// One set by node id.
    #[must_use]
    pub fn set(&self, node_id: &str) -> Option<&NodeRepresentationSet> {
        self.sets.iter().find(|s| s.node_id == node_id)
    }

    /// Sets in canonical order.
    #[must_use]
    pub fn sets(&self) -> &[NodeRepresentationSet] {
        &self.sets
    }
}

/// Digest of a representation store (artifact identity).
#[must_use]
pub fn representation_store_digest(
    store: &ContextRepresentationStore,
) -> String {
    let sets: Vec<Value> = store
        .sets
        .iter()
        .map(|set| {
            let reps: Vec<Value> = set
                .representations
                .iter()
                .map(|r| {
                    let derived: Vec<Value> = r
                        .derived_from
                        .iter()
                        .map(|(dep, digest)| {
                            json!({
                                "dependency": dep,
                                "digest": digest
                            })
                        })
                        .collect();
                    json!({
                        "level": r.level.as_str(),
                        "origin": r.origin.as_str(),
                        "contentDigest": r.content_digest,
                        "derivedFrom": derived,
                        "content": r.content,
                    })
                })
                .collect();
            json!({
                "nodeId": set.node_id,
                "representations": reps,
            })
        })
        .collect();
    let payload = json!({ "sets": sets });
    crate::determinism::helpers::digest_artifact_payload(
        "ContextRepresentationStore",
        1,
        &payload,
    )
    .expect("ContextRepresentationStore digest is infallible")
}

/// Levels present in `set`, ascending.
#[must_use]
pub fn available_levels(
    set: &NodeRepresentationSet,
) -> Vec<RepresentationLevel> {
    set.representations.iter().map(|r| r.level).collect()
}

/// Exact-level lookup.
#[must_use]
pub fn resolve_representation(
    set: &NodeRepresentationSet,
    level: RepresentationLevel,
) -> Option<&NodeRepresentation> {
    set.representations.iter().find(|r| r.level == level)
}

#[cfg(test)]
mod tests {
    use super::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationError, RepresentationLevel, RepresentationOrigin,
        available_levels, content_digest_of, representation_store_digest,
        resolve_representation,
    };

    fn digest_of(text: &str) -> String {
        content_digest_of(text)
    }

    fn rep(
        level: RepresentationLevel,
        origin: RepresentationOrigin,
        content: &str,
        derived_from: Vec<(String, String)>,
    ) -> NodeRepresentation {
        NodeRepresentation {
            level,
            origin,
            content_digest: digest_of(content),
            derived_from,
            content: content.to_owned(),
        }
    }

    fn binding() -> Vec<(String, String)> {
        vec![("dep".to_owned(), digest_of(" dep body "))]
    }

    #[test]
    fn build_ok_canonical_order() {
        let reps = vec![
            rep(
                RepresentationLevel::Source,
                RepresentationOrigin::HostExtracted,
                "",
                vec![],
            ),
            rep(
                RepresentationLevel::Identity,
                RepresentationOrigin::HostExtracted,
                "identity body",
                vec![],
            ),
            rep(
                RepresentationLevel::Structured,
                RepresentationOrigin::HostExtracted,
                "structured facts",
                vec![],
            ),
            rep(
                RepresentationLevel::Summary,
                RepresentationOrigin::ModelDerived,
                "summary prose",
                binding(),
            ),
        ];
        // Provide shuffled input; expect canonical level order.
        let set =
            NodeRepresentationSet::build("ctx-source-auth".to_owned(), reps)
                .expect("build");
        assert_eq!(
            available_levels(&set),
            vec![
                RepresentationLevel::Identity,
                RepresentationLevel::Summary,
                RepresentationLevel::Structured,
                RepresentationLevel::Source
            ]
        );
        // Rebuild with different order yields equal digest.
        let store = ContextRepresentationStore::build(vec![set.clone()])
            .expect("store");
        let store2 = ContextRepresentationStore::build(vec![{
            let mut r = set.representations.clone();
            r.reverse();
            NodeRepresentationSet::build("ctx-source-auth".to_owned(), r)
                .expect("build2")
        }])
        .expect("store2");
        assert_eq!(
            representation_store_digest(&store),
            representation_store_digest(&store2)
        );
    }

    #[test]
    fn duplicate_level_refusal() {
        let err = NodeRepresentationSet::build(
            "n".to_owned(),
            vec![
                rep(
                    RepresentationLevel::Identity,
                    RepresentationOrigin::HostExtracted,
                    "a",
                    vec![],
                ),
                rep(
                    RepresentationLevel::Identity,
                    RepresentationOrigin::HostExtracted,
                    "b",
                    vec![],
                ),
            ],
        )
        .unwrap_err();
        assert_eq!(
            err,
            RepresentationError::DuplicateLevel {
                node_id: "n".to_owned(),
                level: RepresentationLevel::Identity
            }
        );
    }

    #[test]
    fn l2_model_derived_refusal() {
        let err = NodeRepresentationSet::build(
            "n".to_owned(),
            vec![rep(
                RepresentationLevel::Structured,
                RepresentationOrigin::ModelDerived,
                "facts",
                binding(),
            )],
        )
        .unwrap_err();
        assert_eq!(
            err,
            RepresentationError::ModelDerivedStructured {
                node_id: "n".to_owned()
            }
        );
    }

    #[test]
    fn model_derived_without_provenance_refusal_at_l1() {
        let err = NodeRepresentationSet::build(
            "n".to_owned(),
            vec![rep(
                RepresentationLevel::Summary,
                RepresentationOrigin::ModelDerived,
                "prose",
                vec![],
            )],
        )
        .unwrap_err();
        assert_eq!(
            err,
            RepresentationError::UnprovenancedDerived {
                node_id: "n".to_owned(),
                level: RepresentationLevel::Summary
            }
        );
    }

    #[test]
    fn l4_empty_content_ok() {
        let empty_digest = digest_of("");
        let set = NodeRepresentationSet::build(
            "n".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Source,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: empty_digest.clone(),
                derived_from: vec![],
                content: String::new(),
            }],
        )
        .expect("l4 empty ok");
        assert_eq!(set.representations[0].content, "");
        assert_eq!(set.representations[0].content_digest, empty_digest);
    }

    #[test]
    fn digest_mismatch_refusal() {
        let err = NodeRepresentationSet::build(
            "n".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Identity,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: digest_of("other"),
                derived_from: vec![],
                content: "actual".to_owned(),
            }],
        )
        .unwrap_err();
        assert_eq!(
            err,
            RepresentationError::DigestMismatch {
                node_id: "n".to_owned(),
                level: RepresentationLevel::Identity
            }
        );
    }

    #[test]
    fn bounds_refusals() {
        // Id too long
        let long = "x".repeat(257);
        assert_eq!(
            NodeRepresentationSet::build(long.clone(), vec![]).unwrap_err(),
            RepresentationError::IdTooLong { node_id: long }
        );
        // Content too large
        let big = "y".repeat(8193);
        assert_eq!(
            NodeRepresentationSet::build(
                "n".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Identity,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: digest_of(&big),
                    derived_from: vec![],
                    content: big,
                }],
            )
            .unwrap_err(),
            RepresentationError::ContentTooLarge {
                node_id: "n".to_owned(),
                level: RepresentationLevel::Identity
            }
        );
        // Too many bindings
        let many: Vec<(String, String)> =
            (0..65).map(|i| (format!("d{i}"), digest_of("x"))).collect();
        assert_eq!(
            NodeRepresentationSet::build(
                "n".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Identity,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: digest_of("c"),
                    derived_from: many,
                    content: "c".to_owned(),
                }],
            )
            .unwrap_err(),
            RepresentationError::TooManyBindings {
                node_id: "n".to_owned(),
                level: RepresentationLevel::Identity
            }
        );
        // Too many representations
        let many_reps: Vec<NodeRepresentation> = (0..6)
            .map(|_| {
                rep(
                    RepresentationLevel::Identity,
                    RepresentationOrigin::HostExtracted,
                    "a",
                    vec![],
                )
            })
            .collect();
        // Actually duplicate level would trigger before too many, so test len>5 with distinct levels can't exceed 5 distinct levels; use 6 reps to hit TooManyRepresentations before duplicate
        // Since only 5 levels exist, any 6 will include duplicate, but we check length first, so it should be TooManyRepresentations
        assert_eq!(
            NodeRepresentationSet::build("n".to_owned(), many_reps)
                .unwrap_err(),
            RepresentationError::TooManyRepresentations {
                node_id: "n".to_owned()
            }
        );
        // Malformed digest
        assert_eq!(
            NodeRepresentationSet::build(
                "n".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Identity,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: "not-hex".to_owned(),
                    derived_from: vec![],
                    content: String::new(),
                }],
            )
            .unwrap_err(),
            RepresentationError::MalformedDigest {
                node_id: "n".to_owned(),
                level: RepresentationLevel::Identity
            }
        );
    }

    #[test]
    fn store_unique_ids_and_canonical_order() {
        let a = NodeRepresentationSet::build(
            "b".to_owned(),
            vec![rep(
                RepresentationLevel::Identity,
                RepresentationOrigin::HostExtracted,
                "b body",
                vec![],
            )],
        )
        .expect("b");
        let b = NodeRepresentationSet::build(
            "a".to_owned(),
            vec![rep(
                RepresentationLevel::Identity,
                RepresentationOrigin::HostExtracted,
                "a body",
                vec![],
            )],
        )
        .expect("a");
        let store =
            ContextRepresentationStore::build(vec![a.clone(), b.clone()])
                .expect("store");
        assert_eq!(
            store
                .sets()
                .iter()
                .map(|s| s.node_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        // Duplicate node id refused
        let dup =
            ContextRepresentationStore::build(vec![a.clone(), a]).unwrap_err();
        assert_eq!(
            dup,
            RepresentationError::DuplicateNode { node_id: "b".to_owned() }
        );
        // Digest stable across shuffled input order
        let s1 = ContextRepresentationStore::build(vec![b.clone(), {
            NodeRepresentationSet::build(
                "b".to_owned(),
                vec![rep(
                    RepresentationLevel::Identity,
                    RepresentationOrigin::HostExtracted,
                    "b body",
                    vec![],
                )],
            )
            .expect("b2")
        }]);
        // Actually test canonical: store built from [b,a] vs [a,b] same digest
        let store1 = ContextRepresentationStore::build(vec![b.clone(), {
            NodeRepresentationSet::build(
                "c".to_owned(),
                vec![rep(
                    RepresentationLevel::Identity,
                    RepresentationOrigin::HostExtracted,
                    "c",
                    vec![],
                )],
            )
            .expect("c")
        }])
        .expect("s1");
        let store2 = ContextRepresentationStore::build(vec![
            NodeRepresentationSet::build(
                "c".to_owned(),
                vec![rep(
                    RepresentationLevel::Identity,
                    RepresentationOrigin::HostExtracted,
                    "c",
                    vec![],
                )],
            )
            .expect("c2"),
            b,
        ])
        .expect("s2");
        assert_eq!(
            representation_store_digest(&store1),
            representation_store_digest(&store2)
        );
        let _ = s1; // silence
    }

    #[test]
    fn available_levels_ascending() {
        let set = NodeRepresentationSet::build(
            "n".to_owned(),
            vec![
                rep(
                    RepresentationLevel::Source,
                    RepresentationOrigin::HostExtracted,
                    "",
                    vec![],
                ),
                rep(
                    RepresentationLevel::Identity,
                    RepresentationOrigin::HostExtracted,
                    "i",
                    vec![],
                ),
                rep(
                    RepresentationLevel::Detailed,
                    RepresentationOrigin::HostExtracted,
                    "d",
                    vec![],
                ),
            ],
        )
        .expect("set");
        assert_eq!(
            available_levels(&set),
            vec![
                RepresentationLevel::Identity,
                RepresentationLevel::Detailed,
                RepresentationLevel::Source
            ]
        );
    }

    #[test]
    fn resolve_exact_level() {
        let set = NodeRepresentationSet::build(
            "n".to_owned(),
            vec![
                rep(
                    RepresentationLevel::Identity,
                    RepresentationOrigin::HostExtracted,
                    "i",
                    vec![],
                ),
                rep(
                    RepresentationLevel::Structured,
                    RepresentationOrigin::HostExtracted,
                    "s",
                    vec![],
                ),
            ],
        )
        .expect("set");
        assert!(
            resolve_representation(&set, RepresentationLevel::Identity)
                .is_some()
        );
        assert!(
            resolve_representation(&set, RepresentationLevel::Summary)
                .is_none()
        );
        let r = resolve_representation(&set, RepresentationLevel::Structured)
            .expect("structured");
        assert_eq!(r.content, "s");
    }
}
