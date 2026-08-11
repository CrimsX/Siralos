import { deepFreeze } from "../../domain/deep-freeze.js";
import type { GodotVariantValue } from "../scene/models.js";

/**
 * Structured scene/resource mutation operations (Stage 3 milestone 10,
 * ADR 0026).
 *
 * Native mutation is STRUCTURED, never arbitrary text replacement: every
 * operation is a typed intent over the parsed semantic model, validated
 * against the exact source revision before it becomes a prepared
 * mutation. The provider-facing surface is prepare-only; approval,
 * checkpointing, and apply are application-owned.
 */

export type MutationValue = GodotVariantValue;

export type SceneMutationOperation =
  | {
      readonly op: "set_property";
      readonly nodePath: string;
      readonly property: string;
      readonly value: MutationValue;
    }
  | {
      readonly op: "remove_property";
      readonly nodePath: string;
      readonly property: string;
    }
  | {
      readonly op: "add_node";
      readonly name: string;
      readonly type: string;
      readonly parentPath?: string;
      readonly properties?: readonly { readonly name: string; readonly value: MutationValue }[];
      readonly groups?: readonly string[];
    }
  | { readonly op: "remove_node"; readonly nodePath: string }
  | {
      readonly op: "set_script_attachment";
      readonly nodePath: string;
      /** Document-local ext_resource id, or null to remove the attachment. */
      readonly extResourceId: string | null;
    }
  | {
      readonly op: "change_resource_reference";
      readonly resourceId: string;
      readonly newPath?: string;
      readonly newUid?: string;
    }
  | {
      readonly op: "add_signal_connection";
      readonly signal: string;
      readonly from: string;
      readonly to: string;
      readonly method: string;
      readonly flags?: number;
      readonly binds?: readonly MutationValue[];
    }
  | {
      readonly op: "remove_signal_connection";
      readonly signal: string;
      readonly from: string;
      readonly to: string;
      readonly method: string;
    };

export type ResourceMutationOperation =
  | { readonly op: "set_property"; readonly property: string; readonly value: MutationValue }
  | { readonly op: "remove_property"; readonly property: string }
  | {
      readonly op: "change_resource_reference";
      readonly resourceId: string;
      readonly newPath?: string;
      readonly newUid?: string;
    }
  | {
      readonly op: "create_subresource";
      readonly id: string;
      readonly type: string;
      readonly properties?: readonly { readonly name: string; readonly value: MutationValue }[];
    }
  | {
      readonly op: "update_subresource";
      readonly id: string;
      readonly properties: readonly { readonly name: string; readonly value: MutationValue }[];
    }
  | { readonly op: "remove_subresource"; readonly id: string };

export type MutationOperation = SceneMutationOperation | ResourceMutationOperation;

/** Host-owned hard bounds for mutation operations (never raised by input). */
export const MUTATION_LIMITS = Object.freeze({
  maxOperations: 32,
  maxPropertiesPerOperation: 32,
  maxPathBytes: 1024,
  maxNameBytes: 256,
  maxValueBytes: 4096,
});

/** Post-apply semantic expectations derived from the operation set. */
export type SemanticExpectation =
  | { readonly kind: "node_exists"; readonly nodePath: string }
  | { readonly kind: "node_absent"; readonly nodePath: string }
  | {
      readonly kind: "property_equals";
      readonly nodePath: string | null;
      readonly property: string;
      readonly value: MutationValue;
    }
  | {
      readonly kind: "property_absent";
      readonly nodePath: string | null;
      readonly property: string;
    }
  | {
      readonly kind: "connection_exists";
      readonly signal: string;
      readonly from: string;
      readonly to: string;
      readonly method: string;
    }
  | {
      readonly kind: "connection_absent";
      readonly signal: string;
      readonly from: string;
      readonly to: string;
      readonly method: string;
    }
  | {
      readonly kind: "script_attachment";
      readonly nodePath: string;
      readonly extResourceId: string | null;
    }
  | { readonly kind: "subresource_exists"; readonly id: string }
  | { readonly kind: "subresource_absent"; readonly id: string }
  | {
      readonly kind: "resource_reference";
      readonly resourceId: string;
      readonly newPath?: string;
      readonly newUid?: string;
    }
  | { readonly kind: "resource_type"; readonly type: string };

const textEncoder = new TextEncoder();

function requireBounded(text: string, maxBytes: number, field: string): string {
  const value = text.trim();
  if (value.length === 0) {
    throw new Error(`${field} must not be empty.`);
  }
  if (textEncoder.encode(value).length > maxBytes) {
    throw new Error(`${field} exceeds ${maxBytes} UTF-8 bytes.`);
  }
  return value;
}

/** Absolute scene node paths (root-relative, no traversal, no backslashes). */
export function validateNodePath(path: string): string {
  const value = requireBounded(path, MUTATION_LIMITS.maxPathBytes, "A node path");
  if (value.includes("\\") || value.includes("\0") || value.split("/").includes("..")) {
    throw new Error(`Invalid node path: ${value}`);
  }
  return value;
}

function validateName(name: string, field: string): string {
  const value = requireBounded(name, MUTATION_LIMITS.maxNameBytes, field);
  if (value.includes("/") || value.includes("\0")) {
    throw new Error(`${field} must not contain slashes or NUL: ${value}`);
  }
  return value;
}

function validateValue(value: MutationValue, field: string): MutationValue {
  const serialized = textEncoder.encode(JSON.stringify(value));
  if (serialized.length > MUTATION_LIMITS.maxValueBytes) {
    throw new Error(`${field} exceeds ${MUTATION_LIMITS.maxValueBytes} UTF-8 bytes.`);
  }
  if (value.kind === "opaque") {
    throw new Error(`${field} must not carry opaque/unknown constructors; use structured values.`);
  }
  return value;
}

function validateProperties(
  properties: readonly { readonly name: string; readonly value: MutationValue }[] | undefined,
  field: string,
): { readonly name: string; readonly value: MutationValue }[] {
  const values = properties ?? [];
  if (values.length > MUTATION_LIMITS.maxPropertiesPerOperation) {
    throw new Error(
      `${field} accepts at most ${MUTATION_LIMITS.maxPropertiesPerOperation} properties.`,
    );
  }
  const seen = new Set<string>();
  return values.map((property) => {
    const name = validateName(property.name, `${field} property name`);
    if (seen.has(name)) {
      throw new Error(`${field} contains a duplicate property: ${name}`);
    }
    seen.add(name);
    return { name, value: validateValue(property.value, `${field} property ${name}`) };
  });
}

function validateReferenceChange(operation: {
  readonly resourceId: string;
  readonly newPath?: string;
  readonly newUid?: string;
}): void {
  validateName(operation.resourceId, "A resource id");
  if (operation.newPath !== undefined) {
    requireBounded(operation.newPath, MUTATION_LIMITS.maxPathBytes, "A resource path");
  }
  if (operation.newUid !== undefined) {
    requireBounded(operation.newUid, MUTATION_LIMITS.maxPathBytes, "A resource uid");
  }
  if (operation.newPath === undefined && operation.newUid === undefined) {
    throw new Error("change_resource_reference requires newPath and/or newUid.");
  }
}

/**
 * Validate and detach one mutation operation. Paths are workspace-safe
 * and node paths are absolute; values are structured (opaque constructors
 * are rejected); counts are bounded.
 */
export function validateMutationOperation(operation: MutationOperation): MutationOperation {
  switch (operation.op) {
    case "set_property": {
      const nodePath = "nodePath" in operation ? operation.nodePath : null;
      return {
        op: "set_property",
        ...(nodePath === null ? {} : { nodePath: validateNodePath(nodePath) }),
        property: validateName(operation.property, "A property name"),
        value: validateValue(operation.value, "A property value"),
      };
    }
    case "remove_property": {
      const nodePath = "nodePath" in operation ? operation.nodePath : null;
      return {
        op: "remove_property",
        ...(nodePath === null ? {} : { nodePath: validateNodePath(nodePath) }),
        property: validateName(operation.property, "A property name"),
      };
    }
    case "add_node":
      return {
        op: "add_node",
        name: validateName(operation.name, "A node name"),
        type: validateName(operation.type, "A node type"),
        ...(operation.parentPath === undefined
          ? {}
          : { parentPath: validateNodePath(operation.parentPath) }),
        ...(operation.properties === undefined
          ? {}
          : { properties: validateProperties(operation.properties, "add_node") }),
        ...(operation.groups === undefined
          ? {}
          : {
              groups: operation.groups
                .slice(0, MUTATION_LIMITS.maxPropertiesPerOperation)
                .map((group) => validateName(group, "A group name")),
            }),
      };
    case "remove_node":
      return { op: "remove_node", nodePath: validateNodePath(operation.nodePath) };
    case "set_script_attachment":
      return {
        op: "set_script_attachment",
        nodePath: validateNodePath(operation.nodePath),
        extResourceId:
          operation.extResourceId === null
            ? null
            : validateName(operation.extResourceId, "A resource id"),
      };
    case "change_resource_reference":
      validateReferenceChange(operation);
      return {
        op: "change_resource_reference",
        resourceId: validateName(operation.resourceId, "A resource id"),
        ...(operation.newPath === undefined ? {} : { newPath: operation.newPath }),
        ...(operation.newUid === undefined ? {} : { newUid: operation.newUid }),
      };
    case "add_signal_connection":
      return {
        op: "add_signal_connection",
        signal: validateName(operation.signal, "A signal name"),
        from: validateNodePath(operation.from),
        to: validateNodePath(operation.to),
        method: validateName(operation.method, "A method name"),
        ...(operation.flags === undefined
          ? {}
          : {
              flags: operation.flags,
            }),
        ...(operation.binds === undefined
          ? {}
          : {
              binds: operation.binds
                .slice(0, MUTATION_LIMITS.maxPropertiesPerOperation)
                .map((value, index) => validateValue(value, `A bind value ${index}`)),
            }),
      };
    case "remove_signal_connection":
      return {
        op: "remove_signal_connection",
        signal: validateName(operation.signal, "A signal name"),
        from: validateNodePath(operation.from),
        to: validateNodePath(operation.to),
        method: validateName(operation.method, "A method name"),
      };
    case "create_subresource":
      return {
        op: "create_subresource",
        id: validateName(operation.id, "A subresource id"),
        type: validateName(operation.type, "A subresource type"),
        ...(operation.properties === undefined
          ? {}
          : { properties: validateProperties(operation.properties, "create_subresource") }),
      };
    case "update_subresource":
      return {
        op: "update_subresource",
        id: validateName(operation.id, "A subresource id"),
        properties: validateProperties(operation.properties, "update_subresource"),
      };
    case "remove_subresource":
      return { op: "remove_subresource", id: validateName(operation.id, "A subresource id") };
  }
}

export function validateMutationOperations(
  operations: readonly MutationOperation[],
): MutationOperation[] {
  if (operations.length > MUTATION_LIMITS.maxOperations) {
    throw new Error(`A mutation accepts at most ${MUTATION_LIMITS.maxOperations} operations.`);
  }
  if (operations.length === 0) {
    throw new Error("A mutation requires at least one operation.");
  }
  return deepFreeze(operations.map((operation) => validateMutationOperation(operation)));
}

/** Deterministic post-apply expectations derived from the operation set. */
export function expectedSemanticEffect(
  operations: readonly MutationOperation[],
): readonly SemanticExpectation[] {
  const expectations: SemanticExpectation[] = [];
  for (const operation of operations) {
    switch (operation.op) {
      case "set_property": {
        const nodePath = "nodePath" in operation ? operation.nodePath : null;
        expectations.push({
          kind: "property_equals",
          nodePath,
          property: operation.property,
          value: operation.value,
        });
        break;
      }
      case "remove_property": {
        const nodePath = "nodePath" in operation ? operation.nodePath : null;
        expectations.push({
          kind: "property_absent",
          nodePath,
          property: operation.property,
        });
        break;
      }
      case "add_node": {
        // The expectation binds the node's RESOLVED absolute path
        // (parentPath/name), not the bare name.
        const nodePath =
          operation.parentPath === undefined || operation.parentPath === "."
            ? operation.name
            : `${operation.parentPath}/${operation.name}`;
        expectations.push({ kind: "node_exists", nodePath });
        for (const property of operation.properties ?? []) {
          expectations.push({
            kind: "property_equals",
            nodePath,
            property: property.name,
            value: property.value,
          });
        }
        break;
      }
      case "remove_node":
        expectations.push({ kind: "node_absent", nodePath: operation.nodePath });
        break;
      case "set_script_attachment":
        expectations.push({
          kind: "script_attachment",
          nodePath: operation.nodePath,
          extResourceId: operation.extResourceId,
        });
        break;
      case "change_resource_reference":
        expectations.push({
          kind: "resource_reference",
          resourceId: operation.resourceId,
          ...(operation.newPath === undefined ? {} : { newPath: operation.newPath }),
          ...(operation.newUid === undefined ? {} : { newUid: operation.newUid }),
        });
        break;
      case "add_signal_connection":
        expectations.push({
          kind: "connection_exists",
          signal: operation.signal,
          from: operation.from,
          to: operation.to,
          method: operation.method,
        });
        break;
      case "remove_signal_connection":
        expectations.push({
          kind: "connection_absent",
          signal: operation.signal,
          from: operation.from,
          to: operation.to,
          method: operation.method,
        });
        break;
      case "create_subresource":
        expectations.push({ kind: "subresource_exists", id: operation.id });
        break;
      case "update_subresource":
        expectations.push({ kind: "subresource_exists", id: operation.id });
        break;
      case "remove_subresource":
        expectations.push({ kind: "subresource_absent", id: operation.id });
        break;
    }
  }
  return deepFreeze(expectations);
}
