import type { GodotSceneModel, GodotSceneNode } from "./models.js";

/**
 * Deterministic scene node tree (Stage 3 milestone 8).
 *
 * A tree-query representation derived from node declarations: root node,
 * children in declaration order, node paths, and parent relationships.
 * Malformed parent relationships are surfaced as orphans (plus parser
 * diagnostics) — never fabricated structure. The tree is a derived
 * projection of `GodotSceneModel`, not a separate source of truth.
 */

export interface GodotSceneTreeNode {
  readonly node: GodotSceneNode;
  /** Scene node path, e.g. `Player/Weapon` (root is `"."`). */
  readonly path: string;
  readonly children: readonly GodotSceneTreeNode[];
}

export interface GodotSceneNodeTree {
  /** The scene root (first node); null when the scene declares no nodes. */
  readonly root: GodotSceneTreeNode | null;
  /** Nodes whose parent could not be resolved (malformed parent paths). */
  readonly orphans: readonly GodotSceneNode[];
  /** Lookup by node path (declared nodes only). */
  readonly nodesByPath: ReadonlyMap<string, GodotSceneTreeNode>;
  /** All declared node paths in declaration order. */
  readonly paths: readonly string[];
}

/** Build the deterministic node tree for a parsed scene model. */
export function buildSceneNodeTree(model: GodotSceneModel): GodotSceneNodeTree {
  const nodes = model.nodes;
  if (nodes.length === 0) {
    return { root: null, orphans: [], nodesByPath: new Map(), paths: [] };
  }
  // Effective parent: explicit attribute; the FIRST node is the root (the
  // serialized root carries no parent attribute, or `parent="."`).
  const effectiveParent = (node: GodotSceneNode, index: number): string | null =>
    node.parentPath !== undefined ? node.parentPath : index === 0 ? "." : null;
  const root = nodes[0] as GodotSceneNode;

  // Children of the root are serialized with `parent="."`; deeper children
  // reference their parent's node path. The ROOT_KEY sentinel keeps root
  // children distinct from a malformed `parent="."` on the root itself.
  const ROOT_KEY = "\u0000root";
  const childrenByParent = new Map<string, GodotSceneNode[]>();
  const orphans: GodotSceneNode[] = [];
  for (let index = 0; index < nodes.length; index += 1) {
    const node = nodes[index] as GodotSceneNode;
    if (node === root) {
      continue;
    }
    const parent = effectiveParent(node, index);
    if (parent === null) {
      orphans.push(node);
      continue;
    }
    const key = parent === "." ? ROOT_KEY : parent;
    const list = childrenByParent.get(key) ?? [];
    list.push(node);
    childrenByParent.set(key, list);
  }

  const byPath = new Map<string, GodotSceneTreeNode>();
  const order: string[] = [];
  const visitedNodes = new Set<GodotSceneNode>();
  const build = (parentKey: string, prefix: string): GodotSceneTreeNode[] => {
    const children: GodotSceneTreeNode[] = [];
    for (const node of childrenByParent.get(parentKey) ?? []) {
      visitedNodes.add(node);
      const path = prefix.length === 0 ? node.name : `${prefix}/${node.name}`;
      const childNodes = build(path, path);
      const treeNode: GodotSceneTreeNode = { node, path, children: childNodes };
      byPath.set(path, treeNode);
      order.push(path);
      children.push(treeNode);
    }
    return children;
  };

  const rootChildren = build(ROOT_KEY, "");
  const treeRoot: GodotSceneTreeNode = { node: root, path: ".", children: rootChildren };
  byPath.set(".", treeRoot);
  // Nodes whose parent path never resolves inside the scene are orphans —
  // never fabricated structure (malformed parent relationships surface
  // both here and as parser diagnostics).
  for (const list of childrenByParent.values()) {
    for (const node of list) {
      if (!visitedNodes.has(node)) {
        orphans.push(node);
      }
    }
  }
  return { root: treeRoot, orphans, nodesByPath: byPath, paths: [".", ...order] };
}

/** Query which nodes belong to a group (serialized data only). */
export function nodesInGroup(tree: GodotSceneNodeTree, group: string): readonly GodotSceneNode[] {
  const matches: GodotSceneNode[] = [];
  const visit = (node: GodotSceneTreeNode): void => {
    if (node.node.groups.includes(group)) {
      matches.push(node.node);
    }
    for (const child of node.children) {
      visit(child);
    }
  };
  if (tree.root !== null) {
    visit(tree.root);
  }
  for (const orphan of tree.orphans) {
    if (orphan.groups.includes(group)) {
      matches.push(orphan);
    }
  }
  return matches;
}

/** Bounded compact tree text projection (`view: tree`), explicit truncation. */
export function renderSceneTreeText(
  tree: GodotSceneNodeTree,
  options: { readonly maxLines?: number } = {},
): { readonly text: string; readonly truncated: boolean } {
  const maxLines = options.maxLines ?? 200;
  const lines: string[] = [];
  const visit = (node: GodotSceneTreeNode, depth: number): void => {
    if (lines.length >= maxLines) {
      return;
    }
    const kind = node.node.instance !== undefined ? " (instance)" : "";
    lines.push(`${"  ".repeat(depth)}${node.node.name}: ${node.node.type ?? "<external>"}${kind}`);
    for (const child of node.children) {
      visit(child, depth + 1);
    }
  };
  if (tree.root !== null) {
    visit(tree.root, 0);
  }
  const truncated = lines.length >= maxLines;
  return { text: lines.join("\n"), truncated };
}
