/**
 * Revision-oracle probe (differential harness, ADR 0033, Stage 3R R4).
 *
 * Executes workspace-revision scenarios against the REAL TypeScript
 * reference revision registry (packages/core/src/workspace/workspace-
 * revision.ts) and prints the canonical R4 observation object.
 * Deterministic: fingerprints and op scripts come from the scenario
 * input; the observation clock is the registry's own counter.
 */
import { readFileSync } from "node:fs";
import {
  computeWorkspaceRevisionHandle,
  createWorkspaceRevisionRegistry,
} from "../../../packages/core/src/workspace/workspace-revision.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

function identityJson(identity) {
  if (identity === null || identity === undefined) {
    return null;
  }
  return {
    workspaceFingerprint: identity.workspaceFingerprint,
    path: identity.path,
    sha256: identity.sha256,
  };
}

function run(input) {
  const registry = createWorkspaceRevisionRegistry({
    workspaceFingerprint: input.fingerprint,
    ...(input.limit === undefined ? {} : { maxEntries: input.limit }),
  });
  const other = createWorkspaceRevisionRegistry({
    workspaceFingerprint: "fixture-other-workspace",
  });
  const ops = [];
  for (const op of input.ops ?? []) {
    let result = null;
    switch (op.op) {
      case "issue":
        result = registry.issue(op.path, op.sha256);
        break;
      case "resolve":
        result = identityJson(registry.resolve(op.handle));
        break;
      case "current":
        result = registry.currentRevision(op.path);
        break;
      case "state":
        result = registry.revisionForState(op.path, op.sha256);
        break;
      case "invalidate":
        registry.invalidatePath(op.path);
        break;
      case "observe": {
        const handle = registry.currentRevision(op.path);
        if (handle !== null) {
          registry.observeRead(op.path, handle, op.mode);
        }
        break;
      }
      case "observed":
        result = registry.observedReads().map((read) => ({
          path: read.path,
          revision: read.revision,
          mode: read.mode,
          atMs: read.atMs,
        }));
        break;
      case "size":
        result = registry.size;
        break;
      case "clear":
        registry.clear();
        break;
      case "foreign-resolve":
        result = identityJson(other.resolve(op.handle));
        break;
      case "foreign-issue":
        result = other.issue(op.path, op.sha256);
        break;
      case "compute":
        result = computeWorkspaceRevisionHandle(op.workspace, op.path, op.sha256);
        break;
      default:
        throw new Error(`unsupported revision op ${op.op}`);
    }
    ops.push({ op: op.op, result });
  }
  return { ops };
}

const input = readStdinBounded();
process.stdout.write(`${JSON.stringify(run(input))}\n`);
