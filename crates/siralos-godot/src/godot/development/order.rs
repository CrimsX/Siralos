//! Deterministic unified apply ordering (Stage 3 milestone 11,
//! ADR 0027).
//!
//! Mirrors `packages/core/src/godot/development/unified-order.ts`: the
//! apply order across targets of a mixed change set is explicit and
//! evidenced. Explicit cross-target dependency edges are resolved from
//! the prepared targets (a target referencing another target's path must
//! apply after it), then a deterministic topological sort with a path
//! tie-break orders the targets. "Scripts first" or "scenes first" are
//! never hardcoded: with no dependency edge the order is deterministic
//! path order and the rationale records that no ordering semantics were
//! required.

use super::DevelopmentError;

/// One target of a unified change set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedOrderTarget {
    /// Stable target id.
    pub target_id: String,
    /// Workspace-relative document path.
    pub path: String,
    /// Workspace-relative paths this target references (scene
    /// ext_resources, resource references, script attachments), resolved
    /// by the host from the current documents.
    pub references: Vec<String>,
}

/// The derived apply order with its deterministic rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedApplyOrder {
    /// Target ids in apply order.
    pub order: Vec<String>,
    /// Deterministic rationale for the derived order.
    pub rationale: String,
}

/// Dependency edge: `before` must apply before `after`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedOrderEdge {
    /// The referenced target (applies first).
    pub before: String,
    /// The referencing target (applies after).
    pub after: String,
}

/// One reference that did not resolve to another target in the set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedReference {
    /// The referencing target id.
    pub target_id: String,
    /// The unresolved referenced path.
    pub path: String,
}

/// Resolve the explicit cross-target edges. A reference to another
/// target's path creates the edge `referenced -> referencing` (the
/// referenced path must already be in its final state).
pub fn derive_unified_order_edges(
    targets: &[UnifiedOrderTarget],
) -> (Vec<UnifiedOrderEdge>, Vec<UnresolvedReference>) {
    let mut path_to_target = std::collections::HashMap::new();
    for target in targets {
        path_to_target.insert(target.path.as_str(), target.target_id.as_str());
    }
    let mut edges: Vec<UnifiedOrderEdge> = Vec::new();
    let mut unresolved_references: Vec<UnresolvedReference> = Vec::new();
    for target in targets {
        for reference in &target.references {
            let referenced_target = path_to_target.get(reference.as_str());
            match referenced_target {
                None => unresolved_references.push(UnresolvedReference {
                    target_id: target.target_id.clone(),
                    path: reference.clone(),
                }),
                Some(referenced) if *referenced == target.target_id => {}
                Some(referenced) => edges.push(UnifiedOrderEdge {
                    before: (*referenced).to_owned(),
                    after: target.target_id.clone(),
                }),
            }
        }
    }
    (edges, unresolved_references)
}

/// Deterministic topological sort with path tie-break. Cycles (which
/// cannot occur from path references of one target set) are reported as
/// errors rather than silently reordered.
pub fn derive_unified_apply_order(
    targets: &[UnifiedOrderTarget],
    edges: &[UnifiedOrderEdge],
) -> Result<UnifiedApplyOrder, DevelopmentError> {
    if targets.is_empty() {
        return Ok(UnifiedApplyOrder {
            order: Vec::new(),
            rationale: "No targets to order.".to_owned(),
        });
    }
    let mut paths_by_id: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::new();
    for target in targets {
        paths_by_id.insert(target.target_id.as_str(), target.path.as_str());
    }
    let mut incoming: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    let mut dependents: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for target in targets {
        incoming.insert(target.target_id.as_str(), Vec::new());
        dependents.insert(target.target_id.as_str(), Vec::new());
    }
    for edge in edges {
        let (Some(before), Some(after)) = (
            paths_by_id.get(edge.before.as_str()),
            paths_by_id.get(edge.after.as_str()),
        ) else {
            continue;
        };
        // Edge endpoints must name known targets.
        if !incoming.contains_key(edge.before.as_str())
            || !incoming.contains_key(edge.after.as_str())
        {
            continue;
        }
        let _ = (before, after);
        if let Some(list) = incoming.get_mut(edge.after.as_str()) {
            list.push(edge.before.as_str());
        }
        if let Some(list) = dependents.get_mut(edge.before.as_str()) {
            list.push(edge.after.as_str());
        }
    }
    let mut ordered: Vec<String> = Vec::with_capacity(targets.len());
    let mut pending: Vec<&str> =
        targets.iter().map(|target| target.target_id.as_str()).collect();
    while !pending.is_empty() {
        let mut ready: Vec<&str> = pending
            .iter()
            .copied()
            .filter(|id| incoming.get(*id).is_some_and(Vec::is_empty))
            .collect();
        ready.sort_by(|left, right| {
            let path_left = paths_by_id.get(left).copied().unwrap_or("");
            let path_right = paths_by_id.get(right).copied().unwrap_or("");
            path_left.cmp(path_right)
        });
        if ready.is_empty() {
            let mut cycle: Vec<&str> = pending.clone();
            cycle.sort_unstable();
            return Err(DevelopmentError {
                message: format!(
                    "The unified apply order contains a dependency cycle: {}.",
                    cycle.join(", ")
                ),
            });
        }
        for id in &ready {
            ordered.push((*id).to_owned());
        }
        pending.retain(|id| !ready.contains(id));
        for id in &ready {
            if let Some(dependent_list) = dependents.get(*id) {
                for dependent in dependent_list {
                    if let Some(incoming_list) = incoming.get_mut(*dependent) {
                        incoming_list.retain(|entry| entry != id);
                    }
                }
            }
        }
    }
    let edge_count = edges.len();
    let rationale = if edge_count == 0 {
        format!(
            "No cross-target dependency was resolved; targets apply in deterministic \
             path order ({}).",
            ordered
                .iter()
                .map(|id| paths_by_id.get(id.as_str()).copied().unwrap_or(id))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        format!(
            "Apply order derived from {edge_count} resolved cross-target dependency \
             edge(s); ties broken deterministically by path."
        )
    };
    Ok(UnifiedApplyOrder { order: ordered, rationale })
}

#[cfg(test)]
mod tests {
    use super::{
        UnifiedOrderEdge, UnifiedOrderTarget, derive_unified_apply_order,
        derive_unified_order_edges,
    };

    fn target(
        id: &str,
        path: &str,
        references: &[&str],
    ) -> UnifiedOrderTarget {
        UnifiedOrderTarget {
            target_id: id.to_owned(),
            path: path.to_owned(),
            references: references
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
        }
    }

    #[test]
    fn empty_target_sets_order_trivially() {
        let order = derive_unified_apply_order(&[], &[]).expect("orders");
        assert!(order.order.is_empty());
        assert_eq!(order.rationale, "No targets to order.");
    }

    #[test]
    fn references_create_referenced_first_edges_and_record_unresolved() {
        let targets = vec![
            target("scene", "res://main.tscn", &[]),
            target(
                "script",
                "res://main.gd",
                &["res://main.tscn", "res://missing.tscn"],
            ),
            target("self", "res://self.gd", &["res://self.gd"]),
        ];
        let (edges, unresolved) = derive_unified_order_edges(&targets);
        assert_eq!(
            edges,
            vec![UnifiedOrderEdge {
                before: "scene".to_owned(),
                after: "script".to_owned(),
            }]
        );
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].target_id, "script");
        assert_eq!(unresolved[0].path, "res://missing.tscn");
    }

    #[test]
    fn zero_edge_orders_use_deterministic_path_order_with_rationale() {
        let targets = vec![
            target("b", "res://b.tscn", &[]),
            target("a", "res://a.gd", &[]),
        ];
        let order = derive_unified_apply_order(&targets, &[]).expect("orders");
        assert_eq!(order.order, vec!["a", "b"]);
        assert_eq!(
            order.rationale,
            "No cross-target dependency was resolved; targets apply in deterministic \
             path order (res://a.gd, res://b.tscn)."
        );
    }

    #[test]
    fn dependency_edges_override_path_order() {
        let targets = vec![
            target("z_script", "res://aaa.gd", &["res://zzz.tscn"]),
            target("a_scene", "res://zzz.tscn", &[]),
        ];
        let (edges, _) = derive_unified_order_edges(&targets);
        let order =
            derive_unified_apply_order(&targets, &edges).expect("orders");
        // Path order would put aaa first; the dependency edge wins.
        assert_eq!(order.order, vec!["a_scene", "z_script"]);
        assert!(order.rationale.contains("1 resolved cross-target"));
    }

    #[test]
    fn cycles_are_reported_not_silently_reordered() {
        let targets = vec![
            target("x", "res://x.gd", &["res://y.gd"]),
            target("y", "res://y.gd", &["res://x.gd"]),
        ];
        let (edges, _) = derive_unified_order_edges(&targets);
        let error = derive_unified_apply_order(&targets, &edges)
            .expect_err("cycle rejected");
        assert_eq!(
            error.message,
            "The unified apply order contains a dependency cycle: x, y."
        );
    }
}
