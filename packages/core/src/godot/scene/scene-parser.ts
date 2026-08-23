import { GODOT_SCENE_LIMITS } from "./limits.js";
import type {
  ExternalResourceRef,
  GodotDiagnosticCode,
  GodotProperty,
  GodotSceneModel,
  GodotSceneNode,
  GodotSignalConnection,
  GodotTextDiagnostic,
  GodotTextDocument,
  ResourceReference,
  SceneReference,
  SubResourceRef,
} from "./models.js";
import { resolveResPath } from "./resolution.js";
import { isBalancedText, isCommentLine, parseHeaderAttributes, splitKeyValue } from "./text.js";
import { parseGodotVariant, parseQuotedString } from "./variant.js";

/**
 * Deterministic `.tscn` parser (Stage 3 milestone 8).
 *
 * Parses the subset of Godot 4 text-scene syntax needed to inspect
 * ordinary projects: scene header (`load_steps`, `format`, `uid`),
 * `ext_resource`, `sub_resource`, `node`, `connection`, `[editable]`
 * metadata, and ordinary property assignments. Expressions are never
 * executed and Variant values are parsed conservatively (see variant.ts).
 *
 * Malformed input produces structured diagnostics and partial results;
 * it never crashes and never fabricates structure. Every result binds to
 * the exact workspace revision passed by the host.
 */

export interface ParseSceneOptions {
  /** Exact workspace revision of the parsed source state (host-bound). */
  readonly revision?: string | null;
}

const SECTION_NAME_PATTERN = /^([A-Za-z_][A-Za-z0-9_]*)(?:\s+(.*))?$/s;

type DiagnosticSink = (
  code: GodotDiagnosticCode,
  severity: GodotTextDiagnostic["severity"],
  message: string,
  line?: number,
) => void;

/** Mutable build-time node shape; frozen into GodotSceneNode on return. */
interface MutableSceneNode extends Omit<GodotSceneNode, "properties"> {
  readonly properties: GodotProperty[];
}

/** Mutable build-time subresource shape. */
interface MutableSubResource {
  readonly id: string;
  readonly type: string;
  readonly line: number;
  readonly properties: GodotProperty[];
}

export function parseGodotScene(
  content: string,
  path: string,
  options: ParseSceneOptions = {},
): GodotTextDocument<GodotSceneModel> {
  const revision = options.revision ?? null;
  const diagnostics: GodotTextDiagnostic[] = [];
  const addDiagnostic: DiagnosticSink = (code, severity, message, line) => {
    if (diagnostics.length < GODOT_SCENE_LIMITS.maxDiagnostics) {
      diagnostics.push({ code, severity, message, ...(line === undefined ? {} : { line }) });
    }
  };
  // Bounded-truncation diagnostics are emitted ONCE per limit so repeated
  // excess records cannot exhaust the diagnostic budget and mask real
  // errors later in the document.
  const reportedLimits = new Set<string>();
  const reportLimit = (reason: string, message: string, line?: number): void => {
    if (reportedLimits.has(reason)) {
      return;
    }
    reportedLimits.add(reason);
    addDiagnostic("scene.document_truncated", "error", message, line);
  };

  const externalResources: ExternalResourceRef[] = [];
  const subResources: MutableSubResource[] = [];
  const nodes: MutableSceneNode[] = [];
  const connections: GodotSignalConnection[] = [];
  const editableInstances: string[] = [];
  const extIds = new Set<string>();
  const subIds = new Set<string>();
  let header: { format?: number; loadSteps?: number; uid?: string } | null = null;
  let currentSection:
    "header" | "ext_resource" | "sub_resource" | "node" | "connection" | "editable" | "body" =
    "header";
  let currentSubResource: MutableSubResource | null = null;
  let currentNode: MutableSceneNode | null = null;
  let seenSceneHeader = false;
  let truncated = false;
  let sectionCount = 0;
  let resourceCount = 0;
  let propertyCount = 0;

  const lines = content.split(/\r?\n/);
  const lineCount = Math.min(lines.length, GODOT_SCENE_LIMITS.maxLines);
  if (lines.length > GODOT_SCENE_LIMITS.maxLines) {
    truncated = true;
    addDiagnostic(
      "scene.document_truncated",
      "error",
      "The document exceeds the line bound; parsing stopped.",
    );
  }

  for (let index = 0; index < lineCount; index += 1) {
    const rawLine = lines[index] ?? "";
    const trimmed = rawLine.trim();
    if (trimmed.length === 0 || isCommentLine(trimmed)) {
      continue;
    }
    if (trimmed.startsWith("[")) {
      sectionCount += 1;
      if (sectionCount > GODOT_SCENE_LIMITS.maxSections) {
        truncated = true;
        reportLimit(
          "sections",
          `The section count exceeded the bound (${GODOT_SCENE_LIMITS.maxSections}); parsing stopped.`,
        );
        break;
      }
      // The section's closing bracket is the LAST "]" on the line: header
      // values may themselves contain "]" (e.g. `binds=[1, "x"]` or
      // `groups=["a"]`), so the first "]" is never a safe split point.
      const closeIndex = trimmed.lastIndexOf("]");
      if (closeIndex < 0) {
        addDiagnostic(
          "scene.malformed_section",
          "error",
          `Malformed section header at line ${index + 1}: missing closing bracket.`,
          index + 1,
        );
        currentSection = "body";
        currentSubResource = null;
        currentNode = null;
        continue;
      }
      const inner = trimmed.slice(1, closeIndex);
      const match = SECTION_NAME_PATTERN.exec(inner);
      if (match === null) {
        addDiagnostic(
          "scene.malformed_section",
          "error",
          `Malformed section header at line ${index + 1}.`,
          index + 1,
        );
        currentSection = "body";
        continue;
      }
      const sectionName = match[1] as string;
      const attributes = parseHeaderAttributes(
        match[2] ?? "",
        GODOT_SCENE_LIMITS.maxHeaderAttributes,
      ).attributes;
      switch (sectionName) {
        case "gd_scene": {
          if (seenSceneHeader) {
            addDiagnostic(
              "scene.unexpected_header",
              "error",
              `Unexpected duplicate scene header at line ${index + 1}.`,
              index + 1,
            );
          }
          seenSceneHeader = true;
          header = parseSceneHeader(attributes, addDiagnostic, index + 1);
          currentSection = "header";
          currentSubResource = null;
          currentNode = null;
          break;
        }
        case "gd_resource":
          addDiagnostic(
            "scene.unexpected_header",
            "error",
            `A resource header ([gd_resource]) is not valid inside a .tscn document (line ${index + 1}).`,
            index + 1,
          );
          currentSection = "header";
          break;
        case "ext_resource": {
          currentSection = "ext_resource";
          currentSubResource = null;
          currentNode = null;
          if (resourceCount >= GODOT_SCENE_LIMITS.maxResources) {
            truncated = true;
            reportLimit(
              "resources",
              `The resource count exceeded the bound (${GODOT_SCENE_LIMITS.maxResources}); remaining resources are ignored.`,
              index + 1,
            );
            break;
          }
          const ref = parseExtResource(attributes, index + 1, addDiagnostic);
          if (ref !== null) {
            resourceCount += 1;
            if (extIds.has(ref.id)) {
              addDiagnostic(
                "scene.duplicate_resource_id",
                "error",
                `Duplicate ext_resource id "${ref.id}" at line ${index + 1}; the later declaration is ignored.`,
                index + 1,
              );
            } else {
              extIds.add(ref.id);
              externalResources.push(ref);
            }
          }
          break;
        }
        case "sub_resource": {
          currentSection = "sub_resource";
          currentNode = null;
          if (resourceCount >= GODOT_SCENE_LIMITS.maxResources) {
            truncated = true;
            reportLimit(
              "resources",
              `The resource count exceeded the bound (${GODOT_SCENE_LIMITS.maxResources}); remaining resources are ignored.`,
              index + 1,
            );
            currentSubResource = null;
            break;
          }
          resourceCount += 1;
          const type = readStringAttribute(attributes, "type") ?? "";
          const id = readStringAttribute(attributes, "id");
          if (id === null) {
            addDiagnostic(
              "scene.missing_resource_id",
              "error",
              `sub_resource at line ${index + 1} is missing its id attribute.`,
              index + 1,
            );
            currentSubResource = null;
            break;
          }
          if (subIds.has(id)) {
            addDiagnostic(
              "scene.duplicate_resource_id",
              "error",
              `Duplicate sub_resource id "${id}" at line ${index + 1}; the later declaration is ignored.`,
              index + 1,
            );
            currentSubResource = null;
            break;
          }
          subIds.add(id);
          currentSubResource = { id, type, properties: [], line: index + 1 };
          subResources.push(currentSubResource);
          break;
        }
        case "node": {
          currentSection = "node";
          currentSubResource = null;
          if (nodes.length >= GODOT_SCENE_LIMITS.maxNodes) {
            truncated = true;
            reportLimit(
              "nodes",
              `The node count exceeded the bound (${GODOT_SCENE_LIMITS.maxNodes}); remaining nodes are ignored.`,
              index + 1,
            );
            currentNode = null;
            break;
          }
          currentNode = parseNode(attributes, index + 1, addDiagnostic);
          nodes.push(currentNode);
          break;
        }
        case "connection": {
          currentSection = "connection";
          currentSubResource = null;
          currentNode = null;
          if (connections.length >= GODOT_SCENE_LIMITS.maxConnections) {
            truncated = true;
            reportLimit(
              "connections",
              `The connection count exceeded the bound (${GODOT_SCENE_LIMITS.maxConnections}); remaining connections are ignored.`,
              index + 1,
            );
            break;
          }
          const connection = parseConnection(attributes, index + 1, addDiagnostic);
          if (connection !== null) {
            connections.push(connection);
          }
          break;
        }
        case "editable": {
          currentSection = "editable";
          currentSubResource = null;
          currentNode = null;
          const pathAttribute = readStringAttribute(attributes, "path");
          if (pathAttribute === null) {
            addDiagnostic(
              "scene.malformed_section",
              "warning",
              `[editable] at line ${index + 1} is missing its path attribute.`,
              index + 1,
            );
          } else if (editableInstances.length < GODOT_SCENE_LIMITS.maxEditableInstances) {
            editableInstances.push(pathAttribute);
          } else {
            truncated = true;
          }
          break;
        }
        default:
          addDiagnostic(
            "scene.malformed_section",
            "error",
            `Unknown section header "[${sectionName}]" at line ${index + 1}.`,
            index + 1,
          );
          currentSection = "body";
          currentNode = null;
          currentSubResource = null;
          break;
      }
      continue;
    }

    // Ordinary `key = value` record (possibly multiline).
    const record = readRecord(lines, index, lineCount, addDiagnostic, "scene");
    index = record.endIndex;
    if (record.key === null) {
      continue;
    }
    if (currentSection === "node" && currentNode !== null) {
      if (propertyCount >= GODOT_SCENE_LIMITS.maxProperties) {
        truncated = true;
        reportLimit(
          "properties",
          `The property count exceeded the bound (${GODOT_SCENE_LIMITS.maxProperties}); remaining properties are ignored.`,
          record.line,
        );
        continue;
      }
      propertyCount += 1;
      currentNode.properties.push(
        makeProperty(record.key, record.valueText, record.line, addDiagnostic, "scene"),
      );
    } else if (currentSection === "sub_resource" && currentSubResource !== null) {
      if (propertyCount >= GODOT_SCENE_LIMITS.maxProperties) {
        truncated = true;
        reportLimit(
          "properties",
          `The property count exceeded the bound (${GODOT_SCENE_LIMITS.maxProperties}); remaining properties are ignored.`,
          record.line,
        );
        continue;
      }
      propertyCount += 1;
      currentSubResource.properties.push(
        makeProperty(record.key, record.valueText, record.line, addDiagnostic, "scene"),
      );
    } else {
      addDiagnostic(
        "scene.unknown_property",
        "warning",
        `Property "${record.key}" at line ${record.line} is not valid in the current section and was ignored.`,
        record.line,
      );
    }
  }

  const model = buildSceneModel(
    path,
    revision,
    header,
    externalResources,
    subResources,
    nodes,
    connections,
    editableInstances,
    addDiagnostic,
  );

  const errorCount = diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  const status =
    !seenSceneHeader || model === null ? "invalid" : errorCount === 0 ? "complete" : "partial";
  return {
    path,
    revision,
    kind: "scene",
    status,
    document: model,
    diagnostics,
    truncated,
  };
}

function parseSceneHeader(
  attributes: readonly HeaderAttribute[],
  addDiagnostic: DiagnosticSink,
  line: number,
): { format?: number; loadSteps?: number; uid?: string } {
  const header: { format?: number; loadSteps?: number; uid?: string } = {};
  for (const attribute of attributes) {
    if (attribute.name === "format") {
      const parsed = Number(attribute.valueText);
      if (Number.isInteger(parsed) && parsed >= 0) {
        header.format = parsed;
      } else {
        addDiagnostic(
          "scene.unknown_header_attribute",
          "warning",
          `Unsupported format value at line ${line}.`,
          line,
        );
      }
    } else if (attribute.name === "load_steps") {
      const parsed = Number(attribute.valueText);
      if (Number.isInteger(parsed) && parsed >= 0) {
        header.loadSteps = parsed;
      }
    } else if (attribute.name === "uid") {
      const uid = unquoteValue(attribute.valueText);
      if (uid !== null && uid.startsWith("uid://")) {
        header.uid = uid;
      }
    }
  }
  return header;
}

function parseExtResource(
  attributes: readonly HeaderAttribute[],
  line: number,
  addDiagnostic: DiagnosticSink,
): ExternalResourceRef | null {
  const id = readStringAttribute(attributes, "id");
  if (id === null) {
    addDiagnostic(
      "scene.missing_resource_id",
      "error",
      `ext_resource at line ${line} is missing its id attribute.`,
      line,
    );
    return null;
  }
  const type = readStringAttribute(attributes, "type");
  const path = readStringAttribute(attributes, "path");
  const uid = readStringAttribute(attributes, "uid");
  return {
    id,
    ...(type === null ? {} : { type }),
    ...(path === null ? {} : { path }),
    ...(uid === null ? {} : { uid }),
    line,
  };
}

function parseNode(
  attributes: readonly HeaderAttribute[],
  line: number,
  addDiagnostic: DiagnosticSink,
): MutableSceneNode {
  const name = readStringAttribute(attributes, "name");
  if (name === null) {
    addDiagnostic(
      "scene.malformed_section",
      "error",
      `[node] at line ${line} is missing its name attribute.`,
      line,
    );
  }
  const type = readStringAttribute(attributes, "type");
  const parentPath = readStringAttribute(attributes, "parent");
  const ownerPath = readStringAttribute(attributes, "owner");
  const instanceAttribute = readAttribute(attributes, "instance");
  const instance =
    instanceAttribute === null ? undefined : parseInstanceReference(instanceAttribute.valueText);
  const groups = parseGroupList(attributes);
  const rawAttributes: { readonly name: string; readonly rawValue: string }[] = [];
  for (const attribute of attributes) {
    if (
      attribute.name === "name" ||
      attribute.name === "type" ||
      attribute.name === "parent" ||
      attribute.name === "owner" ||
      attribute.name === "instance" ||
      attribute.name === "groups"
    ) {
      continue;
    }
    rawAttributes.push({ name: attribute.name, rawValue: attribute.valueText });
  }
  return {
    name: name ?? "",
    ...(type === null ? {} : { type }),
    ...(parentPath === null ? {} : { parentPath }),
    ...(ownerPath === null ? {} : { ownerPath }),
    ...(instance === undefined ? {} : { instance }),
    groups,
    properties: [],
    rawAttributes,
    sourceRange: { startLine: line, startColumn: 1, endLine: line, endColumn: 1 },
  };
}

function parseInstanceReference(valueText: string): SceneReference | undefined {
  const parsed = parseGodotVariant(valueText);
  if (parsed.value.kind !== "ext_resource") {
    return undefined;
  }
  // Resolution against declared ext_resources happens in buildSceneModel.
  return { kind: "scene", resource: { id: parsed.value.id } };
}

function parseGroupList(attributes: readonly HeaderAttribute[]): readonly string[] {
  const attribute = readAttribute(attributes, "groups");
  if (attribute === null) {
    return [];
  }
  const parsed = parseGodotVariant(attribute.valueText);
  if (parsed.value.kind !== "array") {
    return [];
  }
  const groups: string[] = [];
  for (const item of parsed.value.items) {
    if (groups.length >= GODOT_SCENE_LIMITS.maxGroupsPerNode) {
      break;
    }
    if (item.kind === "string" || item.kind === "string_name") {
      groups.push(item.value);
    }
  }
  return groups;
}

function parseConnection(
  attributes: readonly HeaderAttribute[],
  line: number,
  addDiagnostic: DiagnosticSink,
): GodotSignalConnection | null {
  const signal = readStringAttribute(attributes, "signal");
  const from = readStringAttribute(attributes, "from");
  const to = readStringAttribute(attributes, "to");
  const method = readStringAttribute(attributes, "method");
  if (signal === null || from === null || to === null || method === null) {
    addDiagnostic(
      "scene.malformed_section",
      "error",
      `Connection at line ${line} requires signal, from, to, and method attributes.`,
      line,
    );
    return null;
  }
  let flags: number | undefined;
  const flagsAttribute = readAttribute(attributes, "flags");
  if (flagsAttribute !== null) {
    const parsed = Number(flagsAttribute.valueText);
    if (Number.isInteger(parsed)) {
      flags = parsed;
    }
  }
  let binds: readonly import("./models.js").GodotVariantValue[] | undefined;
  const bindsAttribute = readAttribute(attributes, "binds");
  if (bindsAttribute !== null) {
    const parsed = parseGodotVariant(bindsAttribute.valueText);
    if (parsed.value.kind === "array") {
      binds = parsed.value.items;
    }
  }
  return {
    signal,
    from,
    to,
    method,
    ...(flags === undefined ? {} : { flags }),
    ...(binds === undefined ? {} : { binds }),
    line,
  };
}

function buildSceneModel(
  path: string,
  revision: string | null,
  header: { format?: number; loadSteps?: number; uid?: string } | null,
  externalResources: readonly ExternalResourceRef[],
  subResources: readonly SubResourceRef[],
  nodes: readonly GodotSceneNode[],
  connections: readonly GodotSignalConnection[],
  editableInstances: readonly string[],
  addDiagnostic: DiagnosticSink,
): GodotSceneModel | null {
  if (header === null) {
    return null;
  }
  // Resolve node instance/script references against declared ext/sub ids.
  const resolvedNodes: GodotSceneNode[] = nodes.map((node) => {
    const script = resolveScriptReference(
      node.properties,
      externalResources,
      subResources,
      addDiagnostic,
    );
    return {
      ...node,
      ...(node.instance === undefined
        ? {}
        : {
            instance: resolveSceneReference(
              node.instance,
              externalResources,
              addDiagnostic,
              "error",
            ),
          }),
      ...(script === undefined ? {} : { script }),
    };
  });
  // The root node's instance reference is the inherited base scene,
  // distinct from ordinary child scene instances.
  const root = resolvedNodes.find(
    (node) => node.parentPath === undefined || node.parentPath === ".",
  );
  const baseScene = root?.instance;
  // Structural endpoint validation: parents and connection endpoints.
  validateParentsAndConnections(resolvedNodes, connections, addDiagnostic);
  return {
    path,
    revision,
    ...(header.uid === undefined ? {} : { uid: header.uid }),
    ...(header.format === undefined ? {} : { format: header.format }),
    ...(header.loadSteps === undefined ? {} : { loadSteps: header.loadSteps }),
    ...(baseScene === undefined ? {} : { baseScene }),
    externalResources,
    subResources,
    nodes: resolvedNodes,
    connections,
    editableInstances,
  };
}

function resolveSceneReference(
  reference: SceneReference,
  externalResources: readonly ExternalResourceRef[],
  addDiagnostic: DiagnosticSink,
  severity: GodotTextDiagnostic["severity"],
): SceneReference {
  const declared = externalResources.find((resource) => resource.id === reference.resource.id);
  if (declared === undefined) {
    addDiagnostic(
      "scene.unknown_resource_reference",
      severity,
      `Unknown resource reference ExtResource("${reference.resource.id}") — no matching ext_resource declaration.`,
    );
    return reference;
  }
  const resolvedPath = declared.path === undefined ? undefined : resolveResPath(declared.path);
  return {
    kind: "scene",
    resource: declared,
    ...(resolvedPath?.ok === true ? { resolvedPath: resolvedPath.relativePath } : {}),
  };
}

function resolveScriptReference(
  properties: readonly GodotProperty[],
  externalResources: readonly ExternalResourceRef[],
  subResources: readonly SubResourceRef[],
  addDiagnostic: DiagnosticSink,
): ResourceReference | undefined {
  const scriptProperty = properties.find((property) => property.name === "script");
  if (scriptProperty === undefined) {
    return undefined;
  }
  const scriptValue = scriptProperty.value;
  if (scriptValue.kind === "ext_resource") {
    const declared = externalResources.find((resource) => resource.id === scriptValue.id);
    if (declared === undefined) {
      addDiagnostic(
        "scene.unknown_resource_reference",
        "warning",
        `Unknown script reference ExtResource("${scriptValue.id}") — no matching ext_resource declaration.`,
        scriptProperty.line,
      );
      return undefined;
    }
    const resolvedPath = declared.path === undefined ? undefined : resolveResPath(declared.path);
    return {
      resource: declared,
      ...(resolvedPath?.ok === true ? { resolvedPath: resolvedPath.relativePath } : {}),
    };
  }
  if (scriptValue.kind === "sub_resource") {
    const declared = subResources.find((resource) => resource.id === scriptValue.id);
    if (declared === undefined) {
      addDiagnostic(
        "scene.unknown_resource_reference",
        "warning",
        `Unknown script reference SubResource("${scriptValue.id}") — no matching sub_resource declaration.`,
        scriptProperty.line,
      );
      return undefined;
    }
    return {
      resource: {
        id: declared.id,
        type: declared.type,
        ...(declared.line === undefined ? {} : { line: declared.line }),
      },
    };
  }
  return undefined;
}

function validateParentsAndConnections(
  nodes: readonly GodotSceneNode[],
  connections: readonly GodotSignalConnection[],
  addDiagnostic: DiagnosticSink,
): void {
  if (nodes.length === 0) {
    return;
  }
  const paths = nodePaths(nodes);
  for (const node of nodes) {
    const effectiveParent =
      node.parentPath === undefined ? (node === nodes[0] ? "." : undefined) : node.parentPath;
    if (effectiveParent === undefined) {
      addDiagnostic(
        "scene.unresolved_parent",
        "warning",
        `Node "${node.name}" has no parent attribute and is not the root node; its parent relationship is unresolved.`,
        node.sourceRange?.startLine,
      );
      continue;
    }
    if (effectiveParent !== "." && !paths.includes(effectiveParent)) {
      addDiagnostic(
        "scene.unresolved_parent",
        "warning",
        `Node "${node.name}" declares parent "${effectiveParent}" which is not a declared node in this scene.`,
        node.sourceRange?.startLine,
      );
    }
  }
  for (const connection of connections) {
    if (!pathExists(connection.from, paths)) {
      addDiagnostic(
        "scene.missing_signal_source",
        "warning",
        `Connection "${connection.signal}" references source node "${connection.from}" which is not declared in this scene.`,
        connection.line,
      );
    }
    if (!pathExists(connection.to, paths)) {
      addDiagnostic(
        "scene.missing_signal_target",
        "warning",
        `Connection "${connection.signal}" references target node "${connection.to}" which is not declared in this scene.`,
        connection.line,
      );
    }
  }
}

/**
 * Deterministic addressable node paths: `"."` is the scene root, the root
 * node is addressable by its own name, root-level children by their bare
 * names, and deeper nodes by their full chain (`Parent/Child`). Paths are
 * returned in declaration order.
 */
export function nodePaths(nodes: readonly GodotSceneNode[]): readonly string[] {
  const byParent = new Map<string, string[]>();
  for (const node of nodes) {
    const parent = node.parentPath === undefined ? "." : node.parentPath;
    const list = byParent.get(parent) ?? [];
    list.push(node.name);
    byParent.set(parent, list);
  }
  const paths = new Map<string, string>();
  const walk = (parentPath: string): void => {
    for (const name of byParent.get(parentPath) ?? []) {
      const path = parentPath === "." ? name : `${parentPath}/${name}`;
      paths.set(path, path);
      walk(path);
    }
  };
  walk(".");
  const rootName = nodes[0]?.name;
  const ordered: string[] = ["."];
  if (rootName !== undefined && rootName.length > 0) {
    ordered.push(rootName);
  }
  ordered.push(...paths.keys());
  return ordered;
}

function pathExists(connectionPath: string, paths: readonly string[]): boolean {
  if (connectionPath === ".") {
    return true;
  }
  if (paths.includes(connectionPath)) {
    return true;
  }
  // The path may live inside an instanced scene: a declared prefix is
  // structurally plausible and not reported as missing.
  const segments = connectionPath.split("/");
  for (let index = segments.length - 1; index >= 1; index -= 1) {
    const prefix = segments.slice(0, index).join("/");
    if (paths.includes(prefix)) {
      return true;
    }
  }
  return false;
}

interface Record {
  readonly key: string | null;
  readonly valueText: string;
  readonly line: number;
  readonly endIndex: number;
}

function readRecord(
  lines: readonly string[],
  startIndex: number,
  lineCount: number,
  addDiagnostic: DiagnosticSink,
  kind: "scene" | "resource",
): Record {
  const lineNumber = startIndex + 1;
  const firstLine = lines[startIndex] ?? "";
  const split = splitKeyValue(firstLine);
  if (split === null) {
    addDiagnostic(
      kind === "scene" ? "scene.unknown_property" : "resource.unknown_property",
      "warning",
      `Unrecognized record without a value at line ${lineNumber}.`,
      lineNumber,
    );
    return { key: null, valueText: "", line: lineNumber, endIndex: startIndex };
  }
  let valueText = firstLine.slice(split.valueStart).trim();
  let endIndex = startIndex;
  let continuation = 0;
  while (
    !isBalancedText(valueText) &&
    continuation < GODOT_SCENE_LIMITS.maxValueContinuationLines
  ) {
    endIndex += 1;
    continuation += 1;
    if (endIndex >= lineCount) {
      break;
    }
    const nextLine = lines[endIndex] ?? "";
    const nextTrimmed = nextLine.trim();
    if (nextTrimmed.length === 0 || isCommentLine(nextTrimmed)) {
      continue;
    }
    valueText = `${valueText}\n${nextLine.trim()}`;
  }
  if (!isBalancedText(valueText)) {
    addDiagnostic(
      kind === "scene" ? "scene.unbalanced_value" : "resource.unbalanced_value",
      "error",
      `The value of "${split.key}" at line ${lineNumber} is unbalanced; it was truncated at the continuation bound.`,
      lineNumber,
    );
  }
  return { key: split.key, valueText, line: lineNumber, endIndex };
}

function makeProperty(
  key: string,
  valueText: string,
  line: number,
  addDiagnostic: DiagnosticSink,
  kind: "scene" | "resource",
): GodotProperty {
  const parsed = parseGodotVariant(valueText);
  if (parsed.truncated) {
    addDiagnostic(
      kind === "scene" ? "scene.value_truncated" : "resource.value_truncated",
      "warning",
      `The value of "${key}" at line ${line} exceeds interpretation bounds and was preserved partially.`,
      line,
    );
  }
  return {
    name: unquoteKey(key),
    value: parsed.value,
    rawValue: valueText.slice(0, GODOT_SCENE_LIMITS.maxRawValueLength),
    line,
  };
}

function unquoteKey(key: string): string {
  if (key.length >= 2 && key.startsWith('"') && key.endsWith('"')) {
    return key.slice(1, -1);
  }
  return key;
}

interface HeaderAttribute {
  readonly name: string;
  readonly valueText: string;
  readonly quoted: boolean;
}

function readAttribute(
  attributes: readonly HeaderAttribute[],
  name: string,
): HeaderAttribute | null {
  return attributes.find((attribute) => attribute.name === name) ?? null;
}

function readStringAttribute(attributes: readonly HeaderAttribute[], name: string): string | null {
  const attribute = readAttribute(attributes, name);
  if (attribute === null) {
    return null;
  }
  return unquoteValue(attribute.valueText);
}

function unquoteValue(valueText: string): string | null {
  if (valueText.length >= 2 && valueText.startsWith('"') && valueText.endsWith('"')) {
    const parsed = parseQuotedString(valueText);
    return parsed.ok ? parsed.value : null;
  }
  return valueText.length > 0 ? valueText : null;
}
