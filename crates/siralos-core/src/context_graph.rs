//! Context Graph — reconstructable demand-paging foundation (decision 79).
//!
//! The graph is reconstructable from existing host sources and carries no
//! new persisted store in v1. It bridges the existing seams
//! `artifact-digest`, `context::staleness`, and `context::provenance`
//! without duplicating their engines.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

/// Maximum number of nodes in a single graph.
pub const MAX_NODES: usize = 512;
/// Maximum number of edges in a single graph.
pub const MAX_EDGES: usize = 4096;
/// Maximum byte length of a node id.
pub const MAX_ID_BYTES: usize = 256;
/// Maximum byte length of a node summary.
pub const MAX_SUMMARY_BYTES: usize = 2048;
/// Maximum bindings per node.
pub const MAX_SOURCE_BINDINGS: usize = 64;

/// Kind of a context node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContextNodeKind {
    /// Source material.
    Source,
    /// Decision record.
    Decision,
    /// Knowledge entry.
    Knowledge,
    /// Run evidence.
    Run,
    /// Skill guidance.
    Skill,
    /// Task definition.
    Task,
}

impl ContextNodeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Decision => "decision",
            Self::Knowledge => "knowledge",
            Self::Run => "run",
            Self::Skill => "skill",
            Self::Task => "task",
        }
    }
}

/// Kind of a context edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContextEdgeKind {
    /// Contains relationship.
    Contains,
    /// DependsOn relationship.
    DependsOn,
    /// References relationship.
    References,
    /// TestedBy relationship.
    TestedBy,
    /// Implements relationship.
    Implements,
    /// RelevantTo relationship.
    RelevantTo,
}

impl ContextEdgeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::DependsOn => "depends_on",
            Self::References => "references",
            Self::TestedBy => "tested_by",
            Self::Implements => "implements",
            Self::RelevantTo => "relevant_to",
        }
    }

    fn order(self) -> u8 {
        match self {
            Self::Contains => 0,
            Self::DependsOn => 1,
            Self::References => 2,
            Self::TestedBy => 3,
            Self::Implements => 4,
            Self::RelevantTo => 5,
        }
    }
}

/// One node in the context graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextNode {
    /// Unique node id (host data, bounded).
    pub id: String,
    /// Node kind.
    pub kind: ContextNodeKind,
    /// 64 lowercase hex digest of the node's content.
    pub content_digest: String,
    /// Bounded summary, may be empty.
    pub summary: String,
    /// Bounded (dependency id, digest) pairs for staleness.
    pub source_bindings: Vec<(String, String)>,
    /// Estimated tokens (budgeting only).
    pub token_estimate: usize,
}

/// One directed edge in the context graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEdge {
    /// Source node id.
    pub from: String,
    /// Target node id.
    pub to: String,
    /// Edge kind.
    pub kind: ContextEdgeKind,
}

/// Deterministic budgeting-only token estimate: `ceil(chars/4)`.
///
/// This is never a billing measure; it is only for working-context
/// budgeting and deterministic demotion on overflow.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    chars.div_ceil(4)
}

/// Validation failure for the context graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextGraphError {
    /// Duplicate node id.
    DuplicateNode {
        /// Duplicate id.
        id: String,
    },
    /// Edge references a missing endpoint.
    DanglingEdge {
        /// Edge from.
        from: String,
        /// Edge to.
        to: String,
    },
    /// Content digest is not 64 lowercase hex.
    MalformedDigest {
        /// Node id with the malformed digest.
        id: String,
    },
    /// Too many nodes.
    TooManyNodes,
    /// Too many edges.
    TooManyEdges,
    /// Id exceeds byte bound.
    IdTooLong {
        /// Offending id.
        id: String,
    },
    /// Summary exceeds byte bound.
    SummaryTooLarge {
        /// Node id with oversize summary.
        id: String,
    },
    /// Too many source bindings on a node.
    TooManySourceBindings {
        /// Node id.
        id: String,
    },
}

impl std::fmt::Display for ContextGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateNode { id } => write!(f, "duplicate node: {id}"),
            Self::DanglingEdge { from, to } => {
                write!(f, "dangling edge: {from} -> {to}")
            }
            Self::MalformedDigest { id } => {
                write!(f, "malformed digest: {id}")
            }
            Self::TooManyNodes => write!(f, "too many nodes"),
            Self::TooManyEdges => write!(f, "too many edges"),
            Self::IdTooLong { id } => write!(f, "id too long: {id}"),
            Self::SummaryTooLarge { id } => {
                write!(f, "summary too large: {id}")
            }
            Self::TooManySourceBindings { id } => {
                write!(f, "too many source bindings: {id}")
            }
        }
    }
}

impl std::error::Error for ContextGraphError {}

fn is_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The validated, canonically ordered context graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextGraph {
    nodes: Vec<ContextNode>,
    edges: Vec<ContextEdge>,
}

impl ContextGraph {
    /// Build and validate the graph, returning the canonical ordering.
    ///
    /// Enforces:
    /// - `MAX_NODES` / `MAX_EDGES`
    /// - `MAX_ID_BYTES` / `MAX_SUMMARY_BYTES` / `MAX_SOURCE_BINDINGS`
    /// - unique node ids
    /// - every edge endpoint exists
    /// - `content_digest` is exactly 64 lowercase hex
    /// - canonical storage order nodes-sorted-by-id then edges-sorted-by-(from, kind, to)
    pub fn build(
        nodes: Vec<ContextNode>,
        edges: Vec<ContextEdge>,
    ) -> Result<Self, ContextGraphError> {
        if nodes.len() > MAX_NODES {
            return Err(ContextGraphError::TooManyNodes);
        }
        if edges.len() > MAX_EDGES {
            return Err(ContextGraphError::TooManyEdges);
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for node in &nodes {
            if node.id.len() > MAX_ID_BYTES {
                return Err(ContextGraphError::IdTooLong {
                    id: node.id.clone(),
                });
            }
            if node.summary.len() > MAX_SUMMARY_BYTES {
                return Err(ContextGraphError::SummaryTooLarge {
                    id: node.id.clone(),
                });
            }
            if node.source_bindings.len() > MAX_SOURCE_BINDINGS {
                return Err(ContextGraphError::TooManySourceBindings {
                    id: node.id.clone(),
                });
            }
            if !is_hex_digest(&node.content_digest) {
                return Err(ContextGraphError::MalformedDigest {
                    id: node.id.clone(),
                });
            }
            if !seen.insert(node.id.clone()) {
                return Err(ContextGraphError::DuplicateNode {
                    id: node.id.clone(),
                });
            }
        }
        // Canonical node order.
        let mut sorted_nodes = nodes;
        sorted_nodes.sort_by(|a, b| a.id.cmp(&b.id));
        // Edge endpoint existence.
        let ids: BTreeSet<&str> =
            sorted_nodes.iter().map(|n| n.id.as_str()).collect();
        for edge in &edges {
            if !ids.contains(edge.from.as_str())
                || !ids.contains(edge.to.as_str())
            {
                return Err(ContextGraphError::DanglingEdge {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                });
            }
        }
        let mut sorted_edges = edges;
        sorted_edges.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then_with(|| a.kind.order().cmp(&b.kind.order()))
                .then_with(|| a.to.cmp(&b.to))
        });
        Ok(Self { nodes: sorted_nodes, edges: sorted_edges })
    }

    /// Nodes in canonical order.
    #[must_use]
    pub fn nodes(&self) -> &[ContextNode] {
        &self.nodes
    }

    /// Edges in canonical order.
    #[must_use]
    pub fn edges(&self) -> &[ContextEdge] {
        &self.edges
    }

    /// One node by id.
    #[must_use]
    pub fn node(&self, id: &str) -> Option<&ContextNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

/// Compute the digest of a context graph.
///
/// Uses the single artifact-digest primitive `siralos:ContextGraph:v1\0`
/// + canonical JSON, mirroring `digest_artifact_payload("ContextGraph", 1, …)`.
///
/// Payload shape:
///
/// ```json
/// {
///   "edges": [{"from": "...", "kind": "...", "to": "..."}],
///   "nodes": [{"id": "...", "kind": "...", "contentDigest": "...",
///              "summary": "...", "sourceBindings": [{"dependency": "...", "digest": "..."}],
///              "tokenEstimate": 0}]
/// }
/// ```
/// Nodes are in stored order (canonical by id) and edges in stored order.
/// Field order is canonicalized by the digest primitive.
#[must_use]
pub fn compute_context_graph_digest(graph: &ContextGraph) -> String {
    let nodes: Vec<Value> = graph
        .nodes
        .iter()
        .map(|n| {
            let bindings: Vec<Value> = n
                .source_bindings
                .iter()
                .map(|(dep, digest)| json!({"dependency": dep, "digest": digest}))
                .collect();
            json!({
                "id": n.id,
                "kind": n.kind.as_str(),
                "contentDigest": n.content_digest,
                "summary": n.summary,
                "sourceBindings": bindings,
                "tokenEstimate": n.token_estimate,
            })
        })
        .collect();
    let edges: Vec<Value> = graph
        .edges
        .iter()
        .map(|e| {
            json!({
                "from": e.from,
                "kind": e.kind.as_str(),
                "to": e.to,
            })
        })
        .collect();
    let payload = json!({
        "nodes": nodes,
        "edges": edges,
    });
    crate::determinism::helpers::digest_artifact_payload(
        "ContextGraph",
        1,
        &payload,
    )
    .expect("ContextGraph digest is infallible")
}

/// One stale node with the bindings that changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleContextNode {
    /// Node id that is stale.
    pub node_id: String,
    /// Dependency ids whose digests changed.
    pub stale_bindings: Vec<String>,
}

/// Targeted content-addressed staleness over node source bindings.
///
/// A node is stale iff at least one of its `source_bindings` has a
/// current digest differing from the bound digest. A binding whose
/// dependency is absent from `current` is **not** stale — absent
/// evidence never fabricates staleness. This mirrors the semantics of
/// `crate::identity::derive_identity_staleness` and
/// `crate::context::derive_artifact_staleness` (targeted, content-addressed),
/// but is implemented locally because that helper treats missing inputs as
/// stale (`no longer observable`), while this graph treats absent as not-stale.
/// Tests pin the identical targeting.
#[must_use]
pub fn stale_context_nodes(
    graph: &ContextGraph,
    current: &[(String, String)],
) -> Vec<StaleContextNode> {
    let current_map: BTreeMap<&str, &str> =
        current.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut result = Vec::new();
    for node in graph.nodes() {
        let mut stale = Vec::new();
        for (dep, bound_digest) in &node.source_bindings {
            if let Some(cur) = current_map.get(dep.as_str()) {
                if cur != bound_digest {
                    stale.push(dep.clone());
                }
            }
        }
        if !stale.is_empty() {
            stale.sort();
            result.push(StaleContextNode {
                node_id: node.id.clone(),
                stale_bindings: stale,
            });
        }
    }
    result.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    result
}

#[cfg(test)]
mod tests {
    use super::{
        ContextEdge, ContextEdgeKind, ContextGraph, ContextGraphError,
        ContextNode, ContextNodeKind, compute_context_graph_digest,
        estimate_tokens, stale_context_nodes,
    };

    fn digest(ch: char) -> String {
        std::iter::repeat_n(ch, 64).collect()
    }

    fn node(
        id: &str,
        kind: ContextNodeKind,
        digest: String,
        bindings: Vec<(String, String)>,
    ) -> ContextNode {
        ContextNode {
            id: id.to_owned(),
            kind,
            content_digest: digest,
            summary: String::new(),
            source_bindings: bindings,
            token_estimate: 0,
        }
    }

    #[test]
    fn build_ok_canonical_order() {
        let nodes = vec![
            node("b", ContextNodeKind::Source, digest('b'), vec![]),
            node("a", ContextNodeKind::Decision, digest('a'), vec![]),
            node("c", ContextNodeKind::Knowledge, digest('c'), vec![]),
        ];
        let edges = vec![
            ContextEdge {
                from: "c".to_owned(),
                to: "a".to_owned(),
                kind: ContextEdgeKind::RelevantTo,
            },
            ContextEdge {
                from: "a".to_owned(),
                to: "b".to_owned(),
                kind: ContextEdgeKind::Contains,
            },
            ContextEdge {
                from: "a".to_owned(),
                to: "c".to_owned(),
                kind: ContextEdgeKind::DependsOn,
            },
        ];
        let graph = ContextGraph::build(nodes, edges).expect("build");
        assert_eq!(
            graph.nodes().iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(
            graph.edges().iter().map(|e| e.from.as_str()).collect::<Vec<_>>(),
            vec!["a", "a", "c"]
        );
        // Shuffled input yields identical stored order and digest.
        let nodes2 = vec![
            node("c", ContextNodeKind::Knowledge, digest('c'), vec![]),
            node("a", ContextNodeKind::Decision, digest('a'), vec![]),
            node("b", ContextNodeKind::Source, digest('b'), vec![]),
        ];
        let edges2 = vec![
            ContextEdge {
                from: "a".to_owned(),
                to: "c".to_owned(),
                kind: ContextEdgeKind::DependsOn,
            },
            ContextEdge {
                from: "c".to_owned(),
                to: "a".to_owned(),
                kind: ContextEdgeKind::RelevantTo,
            },
            ContextEdge {
                from: "a".to_owned(),
                to: "b".to_owned(),
                kind: ContextEdgeKind::Contains,
            },
        ];
        let graph2 = ContextGraph::build(nodes2, edges2).expect("build2");
        assert_eq!(graph.nodes(), graph2.nodes());
        assert_eq!(graph.edges(), graph2.edges());
        assert_eq!(
            compute_context_graph_digest(&graph),
            compute_context_graph_digest(&graph2)
        );
    }

    #[test]
    fn duplicate_dangling_malformed_bounds_refusals_typed() {
        // Duplicate
        let dup = ContextGraph::build(
            vec![
                node("a", ContextNodeKind::Source, digest('a'), vec![]),
                node("a", ContextNodeKind::Decision, digest('b'), vec![]),
            ],
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            dup,
            ContextGraphError::DuplicateNode { id: "a".to_owned() }
        );

        // Dangling
        let dang = ContextGraph::build(
            vec![node("a", ContextNodeKind::Source, digest('a'), vec![])],
            vec![ContextEdge {
                from: "a".to_owned(),
                to: "missing".to_owned(),
                kind: ContextEdgeKind::References,
            }],
        )
        .unwrap_err();
        assert_eq!(
            dang,
            ContextGraphError::DanglingEdge {
                from: "a".to_owned(),
                to: "missing".to_owned()
            }
        );

        // Malformed digest
        let mal = ContextGraph::build(
            vec![ContextNode {
                id: "a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "not-hex".to_owned(),
                summary: String::new(),
                source_bindings: vec![],
                token_estimate: 0,
            }],
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            mal,
            ContextGraphError::MalformedDigest { id: "a".to_owned() }
        );

        // Too many nodes (check fast: build 513)
        let many_nodes: Vec<ContextNode> = (0..513)
            .map(|i| {
                node(
                    &format!("n{i:04}"),
                    ContextNodeKind::Source,
                    digest('a'),
                    vec![],
                )
            })
            .collect();
        assert_eq!(
            ContextGraph::build(many_nodes, vec![]).unwrap_err(),
            ContextGraphError::TooManyNodes
        );

        // Too many edges (1 node, 4097 edges looping)
        let nodes =
            vec![node("a", ContextNodeKind::Source, digest('a'), vec![])];
        let edges: Vec<ContextEdge> = (0..4097)
            .map(|_| ContextEdge {
                from: "a".to_owned(),
                to: "a".to_owned(),
                kind: ContextEdgeKind::References,
            })
            .collect();
        assert_eq!(
            ContextGraph::build(nodes, edges).unwrap_err(),
            ContextGraphError::TooManyEdges
        );

        // Id too long
        let long_id = "x".repeat(257);
        assert_eq!(
            ContextGraph::build(
                vec![node(
                    &long_id,
                    ContextNodeKind::Source,
                    digest('a'),
                    vec![]
                )],
                vec![]
            )
            .unwrap_err(),
            ContextGraphError::IdTooLong { id: long_id.clone() }
        );

        // Summary too large
        let big_summary = "y".repeat(2049);
        assert_eq!(
            ContextGraph::build(
                vec![ContextNode {
                    id: "a".to_owned(),
                    kind: ContextNodeKind::Source,
                    content_digest: digest('a'),
                    summary: big_summary.clone(),
                    source_bindings: vec![],
                    token_estimate: 0,
                }],
                vec![]
            )
            .unwrap_err(),
            ContextGraphError::SummaryTooLarge { id: "a".to_owned() }
        );

        // Too many source bindings
        let many_bindings: Vec<(String, String)> =
            (0..65).map(|i| (format!("dep{i}"), digest('a'))).collect();
        assert_eq!(
            ContextGraph::build(
                vec![node(
                    "a",
                    ContextNodeKind::Source,
                    digest('a'),
                    many_bindings
                )],
                vec![]
            )
            .unwrap_err(),
            ContextGraphError::TooManySourceBindings { id: "a".to_owned() }
        );
    }

    #[test]
    fn estimate_tokens_heuristic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        // multibyte chars count as one char each
        assert_eq!(estimate_tokens("界界界界"), 1);
        assert_eq!(estimate_tokens("界界界界界"), 2);
    }

    #[test]
    fn digest_stable_and_field_order_canonical() {
        let n1 = ContextNode {
            id: "a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('a'),
            summary: "hello".to_owned(),
            source_bindings: vec![],
            token_estimate: 4,
        };
        let n2 = ContextNode {
            id: "b".to_owned(),
            kind: ContextNodeKind::Decision,
            content_digest: digest('b'),
            summary: String::new(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let g1 = ContextGraph::build(vec![n1.clone(), n2.clone()], vec![])
            .expect("build");
        let g2 = ContextGraph::build(vec![n2, n1], vec![]).expect("build2");
        let d1 = compute_context_graph_digest(&g1);
        let d2 = compute_context_graph_digest(&g2);
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
        // Changing a field changes digest.
        let mut g3_nodes = g1.nodes().to_vec();
        g3_nodes[0].summary = "different".to_owned();
        let g3 = ContextGraph::build(g3_nodes, g1.edges().to_vec())
            .expect("build3");
        assert_ne!(compute_context_graph_digest(&g3), d1);
    }

    #[test]
    fn staleness_targeted() {
        let a_digest = digest('a');
        let b_digest = digest('b');
        let b_mutated = digest('c');
        // Graph: source nodes a,b ; knowledge node c depends on both via bindings.
        let nodes = vec![
            node("a", ContextNodeKind::Source, a_digest.clone(), vec![]),
            node("b", ContextNodeKind::Decision, b_digest.clone(), vec![]),
            node(
                "c",
                ContextNodeKind::Knowledge,
                digest('d'),
                vec![
                    ("a".to_owned(), a_digest.clone()),
                    ("b".to_owned(), b_digest.clone()),
                ],
            ),
        ];
        let graph = ContextGraph::build(nodes, vec![]).expect("build");
        // No change -> no stale.
        let current = vec![
            ("a".to_owned(), a_digest.clone()),
            ("b".to_owned(), b_digest.clone()),
        ];
        assert_eq!(stale_context_nodes(&graph, &current), vec![]);

        // Mutated a -> only c stale with that binding.
        let current_mut = vec![
            ("a".to_owned(), b_mutated.clone()),
            ("b".to_owned(), b_digest.clone()),
        ];
        let stale = stale_context_nodes(&graph, &current_mut);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].node_id, "c");
        assert_eq!(stale[0].stale_bindings, vec!["a"]);

        // Absent binding not stale.
        let current_absent = vec![("a".to_owned(), a_digest.clone())];
        // b absent, c should not be considered stale for b absent (only if b present and mismatched)
        // But since a unchanged, c has no stale binding.
        assert_eq!(stale_context_nodes(&graph, &current_absent), vec![]);

        // Unrelated node untouched: a and b themselves have no bindings, so never stale.
        let current_all_changed = vec![
            ("a".to_owned(), b_mutated.clone()),
            ("b".to_owned(), b_mutated.clone()),
            ("x".to_owned(), digest('e')),
        ];
        let stale2 = stale_context_nodes(&graph, &current_all_changed);
        assert_eq!(stale2.len(), 1);
        assert_eq!(stale2[0].node_id, "c");
    }
}
