import { canonicalizeJson } from "../digest.js";
import type {
  GodotResourceModel,
  GodotSceneModel,
  GodotSceneNode,
  GodotVariantValue,
} from "../scene/models.js";
import { buildNodePathIndex, findNodeByPath } from "./model-apply.js";
import type { SemanticExpectation } from "./operations.js";

/**
 * Post-apply semantic verification (Stage 3 milestone 10, ADR 0026).
 *
 * Success is NEVER a successful file write: after apply, the target is
 * reparsed at its new revision and the derived semantic model is checked
 * against the expectedSemanticEffect of the prepared mutation. A failed
 * check makes the outcome failed/uncertain — never success.
 */

export type VerificationStatus = "verified" | "failed" | "uncertain";

export interface VerificationCheck {
  readonly expectation: SemanticExpectation;
  readonly status: "verified" | "failed" | "uncertain";
  readonly detail: string;
}

export interface SemanticVerification {
  readonly status: VerificationStatus;
  readonly checks: readonly VerificationCheck[];
}

function valuesEqual(a: GodotVariantValue, b: GodotVariantValue): boolean {
  return canonicalizeJson(a) === canonicalizeJson(b);
}

function nodeProperty(node: GodotSceneNode, property: string): GodotVariantValue | null {
  const found = node.properties.find((entry) => entry.name === property);
  return found === null || found === undefined ? null : found.value;
}

/** Verify the expected semantic effect against the reparsed scene model. */
export function verifySceneSemanticEffect(
  model: GodotSceneModel,
  expectations: readonly SemanticExpectation[],
): SemanticVerification {
  const index = buildNodePathIndex(model.nodes);
  const checks: VerificationCheck[] = [];
  let status: VerificationStatus = "verified";
  const fail = (check: Omit<VerificationCheck, "status">, failed: boolean): void => {
    checks.push({ ...check, status: failed ? "failed" : "verified" });
    if (failed) {
      status = "failed";
    }
  };
  for (const expectation of expectations) {
    switch (expectation.kind) {
      case "node_exists": {
        const node = findNodeByPath(index, expectation.nodePath);
        fail(
          {
            expectation,
            detail:
              node === null
                ? `node ${expectation.nodePath} missing`
                : `node ${expectation.nodePath} present`,
          },
          node === null,
        );
        break;
      }
      case "node_absent": {
        const node = findNodeByPath(index, expectation.nodePath);
        fail(
          {
            expectation,
            detail:
              node === null
                ? `node ${expectation.nodePath} absent`
                : `node ${expectation.nodePath} still present`,
          },
          node !== null,
        );
        break;
      }
      case "property_equals": {
        const node =
          expectation.nodePath === null ? null : findNodeByPath(index, expectation.nodePath);
        if (expectation.nodePath !== null && node === null) {
          fail({ expectation, detail: `node ${expectation.nodePath} missing` }, true);
          break;
        }
        const actual = node === null ? null : nodeProperty(node, expectation.property);
        if (node === null) {
          // Resource-style expectation on a scene: resource properties are
          // not on nodes; treat as uncertain, never success.
          checks.push({
            expectation,
            status: "uncertain",
            detail: "scene model has no resource property surface",
          });
          if (status === "verified") {
            status = "uncertain";
          }
          break;
        }
        const matches = actual !== null && valuesEqual(actual, expectation.value);
        fail(
          {
            expectation,
            detail: matches
              ? `${expectation.nodePath}.${expectation.property} equals expected value`
              : `${expectation.nodePath}.${expectation.property} does not match expected value`,
          },
          !matches,
        );
        break;
      }
      case "property_absent": {
        const node =
          expectation.nodePath === null ? null : findNodeByPath(index, expectation.nodePath);
        if (expectation.nodePath !== null && node === null) {
          fail({ expectation, detail: `node ${expectation.nodePath} missing` }, true);
          break;
        }
        if (node === null) {
          checks.push({
            expectation,
            status: "uncertain",
            detail: "scene model has no resource property surface",
          });
          if (status === "verified") {
            status = "uncertain";
          }
          break;
        }
        const absent = nodeProperty(node, expectation.property) === null;
        fail(
          {
            expectation,
            detail: absent
              ? `property ${expectation.property} absent`
              : `property ${expectation.property} still present`,
          },
          !absent,
        );
        break;
      }
      case "connection_exists": {
        const exists = model.connections.some(
          (connection) =>
            connection.signal === expectation.signal &&
            connection.from === expectation.from &&
            connection.to === expectation.to &&
            connection.method === expectation.method,
        );
        fail(
          { expectation, detail: exists ? "connection present" : "connection missing" },
          !exists,
        );
        break;
      }
      case "connection_absent": {
        const exists = model.connections.some(
          (connection) =>
            connection.signal === expectation.signal &&
            connection.from === expectation.from &&
            connection.to === expectation.to &&
            connection.method === expectation.method,
        );
        fail(
          { expectation, detail: exists ? "connection still present" : "connection absent" },
          exists,
        );
        break;
      }
      case "script_attachment": {
        const node = findNodeByPath(index, expectation.nodePath);
        if (node === null) {
          fail({ expectation, detail: `node ${expectation.nodePath} missing` }, true);
          break;
        }
        const attached = node.script?.resource.id ?? null;
        const matches = attached === expectation.extResourceId;
        fail(
          {
            expectation,
            detail: matches
              ? `script attachment ${String(attached)} matches`
              : `script attachment ${String(attached)} does not match expected ${String(expectation.extResourceId)}`,
          },
          !matches,
        );
        break;
      }
      case "subresource_exists": {
        const exists = model.subResources.some((resource) => resource.id === expectation.id);
        fail(
          { expectation, detail: exists ? "subresource present" : "subresource missing" },
          !exists,
        );
        break;
      }
      case "subresource_absent": {
        const exists = model.subResources.some((resource) => resource.id === expectation.id);
        fail(
          { expectation, detail: exists ? "subresource still present" : "subresource absent" },
          exists,
        );
        break;
      }
      case "resource_reference": {
        const resource = model.externalResources.find(
          (entry) => entry.id === expectation.resourceId,
        );
        if (resource === undefined) {
          fail({ expectation, detail: `ext_resource ${expectation.resourceId} missing` }, true);
          break;
        }
        const pathMatches =
          expectation.newPath === undefined || resource.path === expectation.newPath;
        const uidMatches = expectation.newUid === undefined || resource.uid === expectation.newUid;
        fail(
          {
            expectation,
            detail:
              pathMatches && uidMatches
                ? "resource reference updated"
                : "resource reference does not match expectation",
          },
          !(pathMatches && uidMatches),
        );
        break;
      }
      case "resource_type": {
        checks.push({
          expectation,
          status: "uncertain",
          detail: "resource_type is not a scene-model expectation",
        });
        if (status === "verified") {
          status = "uncertain";
        }
        break;
      }
    }
  }
  return { status, checks };
}

/** Verify the expected semantic effect against the reparsed resource model. */
export function verifyResourceSemanticEffect(
  model: GodotResourceModel,
  expectations: readonly SemanticExpectation[],
): SemanticVerification {
  const checks: VerificationCheck[] = [];
  let status: VerificationStatus = "verified";
  const fail = (check: Omit<VerificationCheck, "status">, failed: boolean): void => {
    checks.push({ ...check, status: failed ? "failed" : "verified" });
    if (failed) {
      status = "failed";
    }
  };
  for (const expectation of expectations) {
    switch (expectation.kind) {
      case "property_equals": {
        const actual =
          model.properties.find((property) => property.name === expectation.property)?.value ??
          null;
        const matches = actual !== null && valuesEqual(actual, expectation.value);
        fail(
          {
            expectation,
            detail: matches
              ? `resource property ${expectation.property} equals expected value`
              : `resource property ${expectation.property} does not match expected value`,
          },
          !matches,
        );
        break;
      }
      case "property_absent": {
        const absent = !model.properties.some((property) => property.name === expectation.property);
        fail(
          {
            expectation,
            detail: absent
              ? `property ${expectation.property} absent`
              : `property ${expectation.property} still present`,
          },
          !absent,
        );
        break;
      }
      case "subresource_exists": {
        const exists = model.subResources.some((resource) => resource.id === expectation.id);
        fail(
          { expectation, detail: exists ? "subresource present" : "subresource missing" },
          !exists,
        );
        break;
      }
      case "subresource_absent": {
        const exists = model.subResources.some((resource) => resource.id === expectation.id);
        fail(
          { expectation, detail: exists ? "subresource still present" : "subresource absent" },
          exists,
        );
        break;
      }
      case "resource_reference": {
        const resource = model.externalResources.find(
          (entry) => entry.id === expectation.resourceId,
        );
        if (resource === undefined) {
          fail({ expectation, detail: `ext_resource ${expectation.resourceId} missing` }, true);
          break;
        }
        const pathMatches =
          expectation.newPath === undefined || resource.path === expectation.newPath;
        const uidMatches = expectation.newUid === undefined || resource.uid === expectation.newUid;
        fail(
          {
            expectation,
            detail:
              pathMatches && uidMatches
                ? "resource reference updated"
                : "resource reference does not match expectation",
          },
          !(pathMatches && uidMatches),
        );
        break;
      }
      case "resource_type": {
        const matches = model.type === expectation.type;
        fail(
          {
            expectation,
            detail: matches
              ? `resource type ${model.type} matches`
              : `resource type ${model.type} does not match ${expectation.type}`,
          },
          !matches,
        );
        break;
      }
      case "node_exists":
      case "node_absent":
      case "connection_exists":
      case "connection_absent":
      case "script_attachment":
        // Scene-only expectations (nodes, connections, attachments) are
        // not verifiable on a resource model: uncertain, never success.
        checks.push({
          expectation,
          status: "uncertain",
          detail: "not verifiable on a resource document",
        });
        if (status === "verified") {
          status = "uncertain";
        }
    }
  }
  return { status, checks };
}
