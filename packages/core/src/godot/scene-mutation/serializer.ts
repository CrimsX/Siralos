import type {
  ExternalResourceRef,
  GodotProperty,
  GodotResourceModel,
  GodotSceneModel,
  GodotSceneNode,
  SubResourceRef,
  GodotVariantValue,
} from "../scene/models.js";

/**
 * Deterministic Godot text-resource serializer (Stage 3 milestone 10,
 * ADR 0026).
 *
 * Serializes the parsed semantic model back to valid `.tscn`/`.tres`
 * syntax: deterministic section ordering, stable document-local ids,
 * preserved UIDs/paths, node paths, and untouched property raw text
 * (`GodotProperty.rawValue`) so unrelated formatting is not churned.
 * New/changed values are serialized from the structured variant model.
 *
 * Serialization discipline (ADR 0026): correct deterministic output
 * outranks fragile formatting preservation, and stable Godot identities
 * are never invented or renumbered without need.
 */

function escapeString(value: string): string {
  let escaped = "";
  for (const character of value) {
    switch (character) {
      case '"':
        escaped += '\\"';
        break;
      case "\\":
        escaped += "\\\\";
        break;
      case "\n":
        escaped += "\\n";
        break;
      case "\t":
        escaped += "\\t";
        break;
      case "\r":
        escaped += "\\r";
        break;
      default:
        if (character.charCodeAt(0) < 0x20) {
          escaped += `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`;
        } else {
          escaped += character;
        }
    }
  }
  return `"${escaped}"`;
}

function formatNumber(value: number): string {
  if (Number.isInteger(value)) {
    return String(value);
  }
  return String(value);
}

/** Deterministic serialization of a structured variant value. */
export function serializeVariantValue(value: GodotVariantValue): string {
  switch (value.kind) {
    case "null":
      return "null";
    case "boolean":
      return value.value ? "true" : "false";
    case "integer":
    case "float":
      return formatNumber(value.value);
    case "string":
      return escapeString(value.value);
    case "string_name":
      return `&${escapeString(value.value)}`;
    case "node_path":
      return `NodePath(${escapeString(value.value)})`;
    case "array":
      return `[${value.items.map((item) => serializeVariantValue(item)).join(", ")}]`;
    case "dictionary": {
      const entries = value.entries.map(
        (entry) => `${serializeVariantValue(entry.key)}: ${serializeVariantValue(entry.value)}`,
      );
      return `{ ${entries.join(", ")} }`;
    }
    case "vector":
      return `${value.typeName}(${value.components.map(formatNumber).join(", ")})`;
    case "color":
      return `Color(${value.components.map(formatNumber).join(", ")})`;
    case "packed_array":
      return `${value.typeName}(${value.items.map((item) => serializeVariantValue(item)).join(", ")})`;
    case "ext_resource":
      return `ExtResource(${escapeString(value.id)})`;
    case "sub_resource":
      return `SubResource(${escapeString(value.id)})`;
    case "resource":
      if (value.uid !== undefined) {
        return `Resource(${escapeString(value.uid)})`;
      }
      if (value.path !== undefined) {
        return `Resource(${escapeString(value.path)})`;
      }
      return `Resource(${escapeString(value.type ?? "Resource")})`;
    case "opaque":
      throw new Error("Opaque variant values cannot be serialized; use structured values.");
  }
}

function serializeProperty(property: GodotProperty): string {
  // Untouched properties keep their exact scanned raw text; new/changed
  // values (rawValue empty) are serialized structurally.
  const raw = property.rawValue ?? "";
  return `${property.name} = ${raw.length > 0 ? raw : serializeVariantValue(property.value)}`;
}

function serializeExternalResource(resource: ExternalResourceRef): string {
  const parts: string[] = [];
  if (resource.type !== undefined) {
    parts.push(`type=${escapeString(resource.type)}`);
  }
  if (resource.uid !== undefined) {
    parts.push(`uid=${escapeString(resource.uid)}`);
  }
  if (resource.path !== undefined) {
    parts.push(`path=${escapeString(resource.path)}`);
  }
  parts.push(`id=${escapeString(resource.id)}`);
  return `[ext_resource ${parts.join(" ")}]`;
}

function serializeSubResource(resource: SubResourceRef): string {
  const lines = [
    `[sub_resource type=${escapeString(resource.type)} id=${escapeString(resource.id)}]`,
  ];
  for (const property of resource.properties) {
    lines.push(serializeProperty(property));
  }
  return lines.join("\n");
}

function serializeResourceReference(
  kind: "instance" | "script",
  reference: { readonly resource: ExternalResourceRef },
): string {
  return `${kind}=ExtResource(${escapeString(reference.resource.id)})`;
}

function serializeNode(node: GodotSceneNode): string {
  const attributes: string[] = [`name=${escapeString(node.name)}`];
  if (node.type !== undefined) {
    attributes.push(`type=${escapeString(node.type)}`);
  }
  if (node.parentPath !== undefined) {
    attributes.push(`parent=${escapeString(node.parentPath)}`);
  }
  if (node.ownerPath !== undefined) {
    attributes.push(`owner=${escapeString(node.ownerPath)}`);
  }
  if (node.groups.length > 0) {
    attributes.push(`groups=[${node.groups.map((group) => escapeString(group)).join(", ")}]`);
  }
  if (node.instance !== undefined) {
    attributes.push(serializeResourceReference("instance", node.instance));
  }
  if (node.script !== undefined) {
    attributes.push(serializeResourceReference("script", node.script));
  }
  for (const attribute of node.rawAttributes) {
    attributes.push(`${attribute.name}=${attribute.rawValue}`);
  }
  const lines = [`[node ${attributes.join(" ")}]`];
  for (const property of node.properties) {
    lines.push(serializeProperty(property));
  }
  return lines.join("\n");
}

function serializeConnection(connection: {
  readonly signal: string;
  readonly from: string;
  readonly to: string;
  readonly method: string;
  readonly flags?: number;
  readonly binds?: readonly GodotVariantValue[];
}): string {
  const parts = [
    `signal=${escapeString(connection.signal)}`,
    `from=${escapeString(connection.from)}`,
    `to=${escapeString(connection.to)}`,
    `method=${escapeString(connection.method)}`,
  ];
  if (connection.flags !== undefined) {
    parts.push(`flags=${connection.flags}`);
  }
  if (connection.binds !== undefined && connection.binds.length > 0) {
    parts.push(
      `binds=[${connection.binds.map((value) => serializeVariantValue(value)).join(", ")}]`,
    );
  }
  return `[connection ${parts.join(" ")}]`;
}

function sceneLoadSteps(model: GodotSceneModel): number {
  return 1 + model.externalResources.length + model.subResources.length;
}

function resourceLoadSteps(model: GodotResourceModel): number {
  return 1 + model.externalResources.length + model.subResources.length;
}

/** Deterministic full-file serialization of a parsed scene model. */
export function serializeScene(model: GodotSceneModel): string {
  const headerParts: string[] = [];
  headerParts.push(`load_steps=${sceneLoadSteps(model)}`);
  if (model.format !== undefined) {
    headerParts.push(`format=${model.format}`);
  } else {
    headerParts.push("format=3");
  }
  if (model.uid !== undefined) {
    headerParts.push(`uid=${escapeString(model.uid)}`);
  }
  const lines = [`[gd_scene ${headerParts.join(" ")}]`, ""];
  for (const resource of model.externalResources) {
    lines.push(serializeExternalResource(resource), "");
  }
  for (const resource of model.subResources) {
    lines.push(serializeSubResource(resource), "");
  }
  for (const node of model.nodes) {
    lines.push(serializeNode(node), "");
  }
  for (const connection of model.connections) {
    lines.push(serializeConnection(connection), "");
  }
  for (const path of model.editableInstances) {
    lines.push(`[editable path=${escapeString(path)}]`, "");
  }
  return lines.join("\n").trimEnd() + "\n";
}

/** Deterministic full-file serialization of a parsed resource model. */
export function serializeResource(model: GodotResourceModel): string {
  const headerParts: string[] = [`type=${escapeString(model.type)}`];
  headerParts.push(`load_steps=${resourceLoadSteps(model)}`);
  if (model.format !== undefined) {
    headerParts.push(`format=${model.format}`);
  } else {
    headerParts.push("format=3");
  }
  if (model.uid !== undefined) {
    headerParts.push(`uid=${escapeString(model.uid)}`);
  }
  const lines = [`[gd_resource ${headerParts.join(" ")}]`, ""];
  for (const resource of model.externalResources) {
    lines.push(serializeExternalResource(resource), "");
  }
  for (const resource of model.subResources) {
    lines.push(serializeSubResource(resource), "");
  }
  lines.push("[resource]");
  if (model.script !== undefined) {
    lines.push(serializeResourceReference("script", model.script));
  }
  for (const property of model.properties) {
    lines.push(serializeProperty(property));
  }
  return lines.join("\n").trimEnd() + "\n";
}
