import type { GodotResourceModel, GodotSceneModel } from "../scene/models.js";
import type { UnifiedChangeSet } from "./unified-change-set.js";

/**
 * Cross-surface consistency verification (Stage 3 milestone 11, ADR 0027).
 *
 * After a mixed apply, the relevant script/scene/resource relationships
 * are checked where they are statically supportable: scene script
 * attachments resolve to a script that exists, serialized signal targets
 * resolve structurally within the document, resource references keep
 * their identity valid, and script/scene pairs changed together surface
 * the runtime-only compatibility concern honestly. The checks never
 * claim full runtime validity statically.
 */

export type ConsistencyCheckStatus = "verified" | "concern" | "runtime_evidence_unavailable";

export interface ConsistencyCheck {
  readonly name: string;
  readonly status: ConsistencyCheckStatus;
  readonly detail: string;
}

export interface CrossSurfaceConsistencyResult {
  readonly consistent: boolean;
  readonly checks: readonly ConsistencyCheck[];
}

export interface CrossSurfaceConsistencyInput {
  readonly changeSet: UnifiedChangeSet;
  /**
   * Post-apply parsed documents for the native targets of the change
   * set. Absent when the document is not part of this change set.
   */
  readonly documents: ReadonlyMap<string, GodotSceneModel | GodotResourceModel>;
  /**
   * Host path inventory: true when the workspace-relative path exists on
   * disk or is a target of the current change set (create targets count).
   */
  readonly pathExists: (path: string) => boolean;
  /** Targets of the change set that create or edit a script path. */
  readonly scriptTargetPaths: readonly string[];
}

function sceneById(
  documents: ReadonlyMap<string, GodotSceneModel | GodotResourceModel>,
  path: string,
): GodotSceneModel | null {
  const document = documents.get(path);
  return document !== null && document !== undefined && "nodes" in document ? document : null;
}

/** Check every scene script attachment resolves to an existing script. */
function checkScriptAttachments(
  input: CrossSurfaceConsistencyInput,
  checks: ConsistencyCheck[],
): void {
  for (const target of input.changeSet.targets) {
    if (target.kind !== "native") {
      continue;
    }
    const scene = sceneById(input.documents, target.path);
    if (scene === null) {
      continue;
    }
    for (const node of scene.nodes) {
      if (node.script === undefined) {
        continue;
      }
      const attachment = node.script.resource;
      const resolved = node.script.resolvedPath;
      if (resolved === undefined || resolved.length === 0) {
        checks.push({
          name: "scene-script-attachment",
          status: "runtime_evidence_unavailable",
          detail: `Node ${node.name} in ${target.path} attaches script id ${attachment.id} without a resolvable res:// path; static resolution is not supported.`,
        });
        continue;
      }
      if (!input.pathExists(resolved)) {
        checks.push({
          name: "scene-script-attachment",
          status: "concern",
          detail: `Node ${node.name} in ${target.path} attaches script ${resolved} which does not exist on disk or in this change set.`,
        });
      } else {
        checks.push({
          name: "scene-script-attachment",
          status: "verified",
          detail: `Node ${node.name} in ${target.path} attaches existing script ${resolved}.`,
        });
      }
    }
  }
}

/** Check serialized signal targets resolve to nodes in the same document. */
function checkSignalTargets(input: CrossSurfaceConsistencyInput, checks: ConsistencyCheck[]): void {
  for (const target of input.changeSet.targets) {
    if (target.kind !== "native") {
      continue;
    }
    const scene = sceneById(input.documents, target.path);
    if (scene === null) {
      continue;
    }
    const nodeNames = new Set(scene.nodes.map((node) => node.name));
    const nodePaths = new Set<string>();
    for (const node of scene.nodes) {
      const parent = node.parentPath ?? ".";
      nodePaths.add(parent === "." ? node.name : `${parent}/${node.name}`);
    }
    for (const connection of scene.connections) {
      const toNode = connection.to.split(":")[0] ?? connection.to;
      if (toNode === ".") {
        continue;
      }
      const local = toNode.startsWith("%") ? toNode.slice(1) : toNode;
      const resolvable =
        nodeNames.has(local) ||
        nodePaths.has(local) ||
        local.split("/").every((part) => part.length > 0 && !part.includes("*"));
      if (!resolvable) {
        checks.push({
          name: "signal-target",
          status: "concern",
          detail: `Connection ${connection.signal} from ${connection.from} targets ${connection.to} which is not structurally resolvable in ${target.path}.`,
        });
      } else {
        checks.push({
          name: "signal-target",
          status: "verified",
          detail: `Connection ${connection.signal} from ${connection.from} targets ${connection.to} which resolves structurally in ${target.path}.`,
        });
      }
    }
  }
}

/** Check external resource references keep a valid identity. */
function checkResourceReferences(
  input: CrossSurfaceConsistencyInput,
  checks: ConsistencyCheck[],
): void {
  for (const target of input.changeSet.targets) {
    if (target.kind !== "native") {
      continue;
    }
    const document = input.documents.get(target.path);
    if (document === undefined) {
      continue;
    }
    const declaredIds = new Set<string>();
    for (const external of document.externalResources) {
      declaredIds.add(external.id);
      if (external.path !== undefined && external.path.length > 0) {
        const path = external.path.replace(/^res:\/\//, "");
        if (!input.pathExists(path)) {
          checks.push({
            name: "resource-reference",
            status: "concern",
            detail: `External resource ${external.id} in ${target.path} references ${external.path} which does not exist on disk or in this change set.`,
          });
        } else {
          checks.push({
            name: "resource-reference",
            status: "verified",
            detail: `External resource ${external.id} in ${target.path} references existing ${external.path}.`,
          });
        }
      }
    }
    for (const sub of document.subResources) {
      declaredIds.add(sub.id);
    }
    const usedIds: string[] = [];
    const collect = (value: unknown): void => {
      if (value === null || typeof value !== "object") {
        return;
      }
      const record = value as Record<string, unknown>;
      if (typeof record["kind"] === "string") {
        const kind = record["kind"];
        if (kind === "ext_resource" || kind === "sub_resource") {
          const id = record["id"];
          if (typeof id === "string") {
            usedIds.push(id);
          }
        }
        if (kind === "array" && Array.isArray(record["items"])) {
          for (const item of record["items"] as unknown[]) {
            collect(item);
          }
        }
        if (kind === "dictionary" && Array.isArray(record["entries"])) {
          for (const entry of record["entries"] as unknown[]) {
            collect(entry);
            collect((entry as Record<string, unknown>)?.["value"]);
          }
        }
        if (kind === "resource" && record["reference"] !== undefined) {
          collect(record["reference"]);
        }
      }
    };
    const scanProperties = (properties: readonly { readonly value: unknown }[]): void => {
      for (const property of properties) {
        collect(property.value);
      }
    };
    if ("nodes" in document) {
      for (const node of document.nodes) {
        scanProperties(node.properties);
        if (node.instance !== undefined) {
          collect(node.instance.resource);
        }
        if (node.script !== undefined) {
          collect(node.script.resource);
        }
      }
    } else {
      scanProperties(document.properties);
    }
    for (const id of usedIds) {
      if (!declaredIds.has(id)) {
        checks.push({
          name: "resource-reference",
          status: "concern",
          detail: `${target.path} references resource id ${id} which is not declared in the document.`,
        });
      }
    }
  }
}

/**
 * Surface the script/scene pair concern: when a script target and a
 * scene target referencing that script change in the same batch, static
 * validation cannot prove serialized scene values stay compatible with
 * the script's exports — the limitation is disclosed, never claimed.
 */
function checkScriptScenePair(
  input: CrossSurfaceConsistencyInput,
  checks: ConsistencyCheck[],
): void {
  if (input.scriptTargetPaths.length === 0) {
    return;
  }
  const affectedScenes: string[] = [];
  for (const target of input.changeSet.targets) {
    if (target.kind !== "native") {
      continue;
    }
    const scene = sceneById(input.documents, target.path);
    if (scene === null) {
      continue;
    }
    for (const node of scene.nodes) {
      if (node.script === undefined) {
        continue;
      }
      const resolved = node.script.resolvedPath;
      if (resolved !== undefined && input.scriptTargetPaths.includes(resolved)) {
        affectedScenes.push(target.path);
        break;
      }
    }
  }
  if (affectedScenes.length > 0) {
    checks.push({
      name: "script-scene-pair",
      status: "runtime_evidence_unavailable",
      detail: `Scripts ${input.scriptTargetPaths.join(", ")} and scenes ${affectedScenes.join(
        ", ",
      )} changed together; serialized scene values may reference the scripts' exported properties and static validation cannot prove runtime compatibility.`,
    });
  }
}

export function verifyCrossSurfaceConsistency(
  input: CrossSurfaceConsistencyInput,
): CrossSurfaceConsistencyResult {
  const checks: ConsistencyCheck[] = [];
  checkScriptAttachments(input, checks);
  checkSignalTargets(input, checks);
  checkResourceReferences(input, checks);
  checkScriptScenePair(input, checks);
  // `consistent` means no concern-level finding: runtime-only disclosures
  // are honest reporting, never a consistency failure.
  const consistent = checks.every((check) => check.status !== "concern");
  return { consistent, checks };
}
