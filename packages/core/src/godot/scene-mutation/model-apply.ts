import { deepFreeze } from "../../domain/deep-freeze.js";
import type {
  GodotProperty,
  GodotResourceModel,
  GodotSceneModel,
  GodotSceneNode,
  GodotVariantValue,
  SubResourceRef,
} from "../scene/models.js";
import {
  validateMutationOperation,
  type MutationOperation,
  type SceneMutationOperation,
} from "./operations.js";

/**
 * Applies validated mutation operations to a COPY of the parsed semantic
 * model (Stage 3 milestone 10, ADR 0026). The original model is never
 * mutated; the result is a new immutable model ready for deterministic
 * serialization. Node paths are resolved through the parent chain;
 * removing a node also removes its descendants and the serialized
 * connections that reference them. Stable identities (ext/sub ids, UIDs,
 * node paths) are preserved.
 */

export interface NodePathIndex {
  readonly pathToNode: ReadonlyMap<string, GodotSceneNode>;
  readonly nodeToPath: ReadonlyMap<GodotSceneNode, string>;
}

/** Absolute node path = root name + parent-chain segments. */
export function buildNodePathIndex(nodes: readonly GodotSceneNode[]): NodePathIndex {
  const pathToNode = new Map<string, GodotSceneNode>();
  const nodeToPath = new Map<GodotSceneNode, string>();
  // Parents always serialize before children in the parser model, so a
  // single pass in model order resolves every absolute path (the map is
  // populated as we go, so parent lookups hit).
  for (const node of nodes) {
    if (node.parentPath === undefined || node.parentPath === ".") {
      pathToNode.set(node.name, node);
      nodeToPath.set(node, node.name);
      continue;
    }
    const parentPath = node.parentPath
      .split("/")
      .filter((segment) => segment.length > 0)
      .join("/");
    const parent = pathToNode.get(parentPath);
    const path = parent === undefined ? node.name : `${parentPath}/${node.name}`;
    pathToNode.set(path, node);
    nodeToPath.set(node, path);
  }
  return { pathToNode, nodeToPath };
}

export function findNodeByPath(index: NodePathIndex, path: string): GodotSceneNode | null {
  return index.pathToNode.get(path) ?? null;
}

export function isDescendantOf(nodePath: string, ancestorPath: string): boolean {
  return nodePath === ancestorPath || nodePath.startsWith(`${ancestorPath}/`);
}

function copyProperties(properties: readonly GodotProperty[]): GodotProperty[] {
  return properties.map((property) => ({ ...property }));
}

function upsertProperty(
  properties: readonly GodotProperty[],
  name: string,
  value: GodotVariantValue,
): GodotProperty[] {
  const copied = copyProperties(properties);
  const index = copied.findIndex((property) => property.name === name);
  const entry: GodotProperty = { name, value, rawValue: "" };
  if (index >= 0) {
    copied[index] = entry;
  } else {
    copied.push(entry);
  }
  return copied;
}

function removeProperty(properties: readonly GodotProperty[], name: string): GodotProperty[] {
  return copyProperties(properties).filter((property) => property.name !== name);
}

type SubresourceOperation =
  | {
      readonly op: "create_subresource";
      readonly id: string;
      readonly type: string;
      readonly properties?: readonly { readonly name: string; readonly value: GodotVariantValue }[];
    }
  | {
      readonly op: "update_subresource";
      readonly id: string;
      readonly properties: readonly { readonly name: string; readonly value: GodotVariantValue }[];
    }
  | { readonly op: "remove_subresource"; readonly id: string };

/** Shared subresource handling for scene and resource documents. */
function applySubresourceOperation(
  subResources: SubResourceRef[],
  operation: SubresourceOperation,
): SubResourceRef[] {
  if (operation.op === "remove_subresource") {
    return subResources.filter((resource) => resource.id !== operation.id);
  }
  if (operation.op === "create_subresource") {
    const id = operation.id;
    if (subResources.some((resource) => resource.id === id)) {
      throw new Error(`Cannot create subresource: id ${id} already exists.`);
    }
    return [
      ...subResources,
      {
        id,
        type: operation.type,
        properties: (operation.properties ?? []).map((property) => ({
          name: property.name,
          value: property.value,
          rawValue: "",
        })),
      },
    ];
  }
  const target = subResources.find((resource) => resource.id === operation.id);
  if (target === undefined) {
    throw new Error(`Cannot update subresource: id ${operation.id} does not exist.`);
  }
  // Merge: the given properties are upserted; untouched properties keep
  // their serialized raw text.
  let properties = target.properties.map((property) => ({ ...property }));
  for (const property of operation.properties) {
    properties = upsertProperty(properties, property.name, property.value);
  }
  return subResources.map((resource) =>
    resource === target ? { ...resource, properties } : resource,
  );
}

function isSubresourceOperation(operation: MutationOperation): operation is SubresourceOperation {
  return (
    operation.op === "create_subresource" ||
    operation.op === "update_subresource" ||
    operation.op === "remove_subresource"
  );
}

function mutateSceneModel(
  model: GodotSceneModel,
  operations: readonly MutationOperation[],
): GodotSceneModel {
  let nodes: GodotSceneNode[] = model.nodes.map((node) => ({
    ...node,
    properties: copyProperties(node.properties),
    groups: [...node.groups],
  }));
  let connections = model.connections.map((connection) => ({ ...connection }));
  let externalResources = model.externalResources.map((resource) => ({ ...resource }));
  let subResources: SubResourceRef[] = model.subResources.map((resource) => ({
    ...resource,
    properties: copyProperties(resource.properties),
  }));

  for (const rawOperation of operations) {
    const raw = validateMutationOperation(rawOperation);
    if (isSubresourceOperation(raw)) {
      subResources = applySubresourceOperation(subResources, raw);
      continue;
    }
    const operation = raw as SceneMutationOperation;
    const index = buildNodePathIndex(nodes);
    switch (operation.op) {
      case "set_property": {
        const node = findNodeByPath(index, operation.nodePath);
        if (node === null) {
          throw new Error(`Cannot set property: node not found at ${operation.nodePath}.`);
        }
        nodes = nodes.map((candidate) =>
          candidate === node
            ? {
                ...candidate,
                properties: upsertProperty(
                  candidate.properties,
                  operation.property,
                  operation.value,
                ),
              }
            : candidate,
        );
        break;
      }
      case "remove_property": {
        const node = findNodeByPath(index, operation.nodePath);
        if (node === null) {
          throw new Error(`Cannot remove property: node not found at ${operation.nodePath}.`);
        }
        nodes = nodes.map((candidate) =>
          candidate === node
            ? { ...candidate, properties: removeProperty(candidate.properties, operation.property) }
            : candidate,
        );
        break;
      }
      case "add_node": {
        const parentPath =
          operation.parentPath ??
          (nodes.some((node) => node.parentPath === undefined || node.parentPath === ".")
            ? "."
            : undefined);
        const added: GodotSceneNode = {
          name: operation.name,
          type: operation.type,
          ...(parentPath === undefined ? {} : { parentPath }),
          groups: [...(operation.groups ?? [])],
          properties: (operation.properties ?? []).map((property) => ({
            name: property.name,
            value: property.value,
            rawValue: "",
          })),
          rawAttributes: [],
        };
        nodes = [...nodes, added];
        break;
      }
      case "remove_node": {
        // Remove the target and every descendant by RESOLVED PATH (node
        // names are unique only among siblings; filtering by name would
        // over-prune same-named nodes elsewhere in the tree).
        const removedPaths = new Set<string>();
        for (const path of index.pathToNode.keys()) {
          if (isDescendantOf(path, operation.nodePath)) {
            removedPaths.add(path);
          }
        }
        nodes = nodes.filter((node) => {
          const path = index.nodeToPath.get(node);
          return path === undefined || !removedPaths.has(path);
        });
        connections = connections.filter(
          (connection) =>
            !isDescendantOf(connection.from, operation.nodePath) &&
            !isDescendantOf(connection.to, operation.nodePath),
        );
        break;
      }
      case "set_script_attachment": {
        const node = findNodeByPath(index, operation.nodePath);
        if (node === null) {
          throw new Error(`Cannot set script attachment: node not found at ${operation.nodePath}.`);
        }
        nodes = nodes.map((candidate) => {
          if (candidate !== node) {
            return candidate;
          }
          if (operation.extResourceId === null) {
            // Remove the attachment: drop the script key AND the serialized
            // `script` property (the parser represents the attachment as a
            // property assignment).
            const copy: GodotSceneNode = { ...candidate };
            const { script: _removed, ...rest } = copy;
            void _removed;
            return {
              ...rest,
              properties: removeProperty(copy.properties, "script"),
            };
          }
          const resource = externalResources.find((entry) => entry.id === operation.extResourceId);
          if (resource === undefined) {
            throw new Error(
              `Cannot attach script: ext_resource ${operation.extResourceId} does not exist.`,
            );
          }
          return {
            ...candidate,
            script: { resource },
            properties: upsertProperty(candidate.properties, "script", {
              kind: "ext_resource",
              id: operation.extResourceId,
            }),
          };
        });
        break;
      }
      case "change_resource_reference": {
        const resource = externalResources.find((entry) => entry.id === operation.resourceId);
        if (resource === undefined) {
          throw new Error(
            `Cannot change reference: ext_resource ${operation.resourceId} does not exist.`,
          );
        }
        externalResources = externalResources.map((candidate) =>
          candidate === resource
            ? {
                ...candidate,
                ...(operation.newPath === undefined ? {} : { path: operation.newPath }),
                ...(operation.newUid === undefined ? {} : { uid: operation.newUid }),
              }
            : candidate,
        );
        break;
      }
      case "add_signal_connection":
        connections = [
          ...connections,
          {
            signal: operation.signal,
            from: operation.from,
            to: operation.to,
            method: operation.method,
            ...(operation.flags === undefined ? {} : { flags: operation.flags }),
            ...(operation.binds === undefined ? {} : { binds: operation.binds }),
          },
        ];
        break;
      case "remove_signal_connection":
        connections = connections.filter(
          (connection) =>
            !(
              connection.signal === operation.signal &&
              connection.from === operation.from &&
              connection.to === operation.to &&
              connection.method === operation.method
            ),
        );
        break;
    }
  }
  return deepFreeze({ ...model, nodes, connections, externalResources, subResources });
}

function mutateResourceModel(
  model: GodotResourceModel,
  operations: readonly MutationOperation[],
): GodotResourceModel {
  let properties = copyProperties(model.properties);
  let externalResources = model.externalResources.map((resource) => ({ ...resource }));
  let subResources: SubResourceRef[] = model.subResources.map((resource) => ({
    ...resource,
    properties: copyProperties(resource.properties),
  }));

  for (const rawOperation of operations) {
    const raw = validateMutationOperation(rawOperation);
    if (isSubresourceOperation(raw)) {
      subResources = applySubresourceOperation(subResources, raw);
      continue;
    }
    const operation = raw as MutationOperation;
    switch (operation.op) {
      case "set_property":
        properties = upsertProperty(properties, operation.property, operation.value);
        break;
      case "remove_property":
        properties = removeProperty(properties, operation.property);
        break;
      case "change_resource_reference": {
        const resource = externalResources.find((entry) => entry.id === operation.resourceId);
        if (resource === undefined) {
          throw new Error(
            `Cannot change reference: ext_resource ${operation.resourceId} does not exist.`,
          );
        }
        externalResources = externalResources.map((candidate) =>
          candidate === resource
            ? {
                ...candidate,
                ...(operation.newPath === undefined ? {} : { path: operation.newPath }),
                ...(operation.newUid === undefined ? {} : { uid: operation.newUid }),
              }
            : candidate,
        );
        break;
      }
      case "add_node":
      case "remove_node":
      case "set_script_attachment":
      case "add_signal_connection":
      case "remove_signal_connection":
      case "create_subresource":
      case "update_subresource":
      case "remove_subresource":
        // Scene-only operations (and guarded subresource ops) are invalid
        // on resource documents.
        throw new Error(`Operation ${operation.op} is not valid on a resource document.`);
    }
  }
  return deepFreeze({ ...model, properties, externalResources, subResources });
}

/**
 * Apply validated operations to a copy of the scene model. Throws on
 * operations that contradict the parsed document (unknown node paths,
 * missing ext_resource ids, duplicate subresource ids).
 */
export function applySceneOperations(
  model: GodotSceneModel,
  operations: readonly MutationOperation[],
): GodotSceneModel {
  return mutateSceneModel(model, operations);
}

/** Apply validated operations to a copy of the resource model. */
export function applyResourceOperations(
  model: GodotResourceModel,
  operations: readonly MutationOperation[],
): GodotResourceModel {
  return mutateResourceModel(model, operations);
}
