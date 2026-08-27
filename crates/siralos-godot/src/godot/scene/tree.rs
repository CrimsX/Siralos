//! Deterministic scene node tree (R8).
//! Mirrors `packages/core/src/godot/scene/scene-tree.ts`.

use std::collections::{HashMap, HashSet};

use super::models::{GodotSceneModel, GodotSceneNode};

/// One node in the tree view.
#[derive(Debug, Clone, PartialEq)]
pub struct GodotSceneTreeNode {
    /// The underlying node.
    pub node: GodotSceneNode,
    /// Scene node path, e.g. `Player/Weapon` (root is `"."`).
    pub path: String,
    /// Child tree nodes.
    pub children: Vec<GodotSceneTreeNode>,
}

/// Deterministic node tree for a parsed scene model.
#[derive(Debug, Clone, PartialEq)]
pub struct GodotSceneNodeTree {
    /// The scene root (first node); `None` when no nodes.
    pub root: Option<GodotSceneTreeNode>,
    /// Nodes whose parent could not be resolved.
    pub orphans: Vec<GodotSceneNode>,
    /// Lookup by node path.
    pub nodes_by_path: HashMap<String, GodotSceneTreeNode>,
    /// All declared node paths in declaration order (`"."` first).
    pub paths: Vec<String>,
}

/// Build the deterministic node tree for a parsed scene model.
#[must_use]
pub fn build_scene_node_tree(model: &GodotSceneModel) -> GodotSceneNodeTree {
    let nodes = &model.nodes;
    if nodes.is_empty() {
        return GodotSceneNodeTree {
            root: None,
            orphans: Vec::new(),
            nodes_by_path: HashMap::new(),
            paths: Vec::new(),
        };
    }
    let root = nodes[0].clone();
    let root_key = "\u{0000}root";
    let mut children_by_parent: HashMap<String, Vec<GodotSceneNode>> =
        HashMap::new();
    let mut orphans: Vec<GodotSceneNode> = Vec::new();
    for (idx, node) in nodes.iter().enumerate() {
        if idx == 0 {
            continue;
        }
        let effective_parent = node.parent_path.clone();
        let Some(parent) = effective_parent else {
            orphans.push(node.clone());
            continue;
        };
        let key = if parent == "." { root_key.to_owned() } else { parent };
        children_by_parent.entry(key).or_default().push(node.clone());
    }
    let mut by_path: HashMap<String, GodotSceneTreeNode> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut visited_nodes: HashSet<String> = HashSet::new();
    fn build_recursive(
        parent_key: &str,
        prefix: &str,
        children_by_parent: &HashMap<String, Vec<GodotSceneNode>>,
        by_path: &mut HashMap<String, GodotSceneTreeNode>,
        order: &mut Vec<String>,
        visited: &mut HashSet<String>,
        visited_nodes: &mut HashSet<String>,
    ) -> Vec<GodotSceneTreeNode> {
        let Some(children) = children_by_parent.get(parent_key) else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for node in children.clone() {
            let key = format!("{}::{}", parent_key, node.name);
            visited_nodes.insert(key);
            let path = if prefix.is_empty() {
                node.name.clone()
            } else {
                format!("{prefix}/{}", node.name)
            };
            visited.insert(path.clone());
            let child_nodes = build_recursive(
                &path,
                &path,
                children_by_parent,
                by_path,
                order,
                visited,
                visited_nodes,
            );
            let tree_node = GodotSceneTreeNode {
                node: node.clone(),
                path: path.clone(),
                children: child_nodes,
            };
            by_path.insert(path.clone(), tree_node.clone());
            order.push(path);
            result.push(tree_node);
        }
        result
    }
    let root_children = build_recursive(
        root_key,
        "",
        &children_by_parent,
        &mut by_path,
        &mut order,
        &mut visited,
        &mut visited_nodes,
    );
    let tree_root = GodotSceneTreeNode {
        node: root.clone(),
        path: ".".to_owned(),
        children: root_children,
    };
    by_path.insert(".".to_owned(), tree_root.clone());
    for children in children_by_parent.values() {
        for node in children {
            let key = format!(
                "{}_{}",
                node.parent_path.as_deref().unwrap_or(""),
                node.name
            );
            if !visited_nodes.contains(&key) {
                let mut found = false;
                for v in by_path.values() {
                    if v.node.name == node.name
                        && v.node.parent_path == node.parent_path
                    {
                        found = true;
                        break;
                    }
                }
                if !found
                    && !orphans.iter().any(|o| {
                        o.name == node.name
                            && o.parent_path == node.parent_path
                    })
                {
                    let mut is_visited = false;
                    for p in by_path.keys() {
                        if p == &node.name
                            || p.ends_with(&format!("/{}", node.name))
                        {
                            is_visited = true;
                            break;
                        }
                    }
                    if !is_visited {
                        orphans.push(node.clone());
                    }
                }
            }
        }
    }
    let mut paths = vec![".".to_owned()];
    paths.extend(order);
    GodotSceneNodeTree {
        root: Some(tree_root),
        orphans,
        nodes_by_path: by_path,
        paths,
    }
}

/// Query which nodes belong to a group.
#[must_use]
pub fn nodes_in_group(
    tree: &GodotSceneNodeTree,
    group: &str,
) -> Vec<GodotSceneNode> {
    let mut matches = Vec::new();
    fn visit(
        node: &GodotSceneTreeNode,
        group: &str,
        out: &mut Vec<GodotSceneNode>,
    ) {
        if node.node.groups.iter().any(|g| g == group) {
            out.push(node.node.clone());
        }
        for child in &node.children {
            visit(child, group, out);
        }
    }
    if let Some(root) = &tree.root {
        visit(root, group, &mut matches);
    }
    for orphan in &tree.orphans {
        if orphan.groups.iter().any(|g| g == group) {
            matches.push(orphan.clone());
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::build_scene_node_tree;
    use crate::godot::scene::models::{GodotSceneModel, GodotSceneNode};
    fn empty_model() -> GodotSceneModel {
        GodotSceneModel {
            path: "test.tscn".to_owned(),
            revision: None,
            uid: None,
            format: None,
            load_steps: None,
            base_scene: None,
            external_resources: Vec::new(),
            sub_resources: Vec::new(),
            nodes: Vec::new(),
            connections: Vec::new(),
            editable_instances: Vec::new(),
        }
    }
    #[test]
    fn empty_model_has_no_root() {
        let m = empty_model();
        let tree = build_scene_node_tree(&m);
        assert!(tree.root.is_none());
        assert!(tree.paths.is_empty());
    }
    #[test]
    fn single_root_node() {
        let mut m = empty_model();
        m.nodes.push(GodotSceneNode {
            name: "Root".to_owned(),
            type_name: Some("Node".to_owned()),
            parent_path: None,
            owner_path: None,
            instance: None,
            script: None,
            groups: Vec::new(),
            properties: Vec::new(),
            raw_attributes: Vec::new(),
            source_range: None,
        });
        let tree = build_scene_node_tree(&m);
        assert!(tree.root.is_some());
        assert!(tree.nodes_by_path.contains_key("."));
    }
}
