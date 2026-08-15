/**
 * Checkpoint-oracle probe (differential harness, ADR 0033, Stage 3R R4).
 *
 * Executes checkpoint scenarios against the REAL TypeScript reference:
 * the filesystem checkpoint store inspection (get/list), startup
 * reconciliation, and the core undo planner. Fixture checkpoint
 * directories are written in the exact stored layout with the
 * store-computed workspace fingerprint; records redact the fingerprint
 * (a machine identity derived from the absolute workspace path) and
 * report fingerprint validity instead.
 */
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, rmSync, readFileSync, writeFileSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createFilesystemCheckpointStore } from "../../../packages/adapters/src/checkpoints/filesystem/checkpoint-store.js";
import { reconcileWorkspaceCheckpoints } from "../../../packages/adapters/src/checkpoints/filesystem/reconciliation.js";
import { planUndo } from "../../../packages/core/src/checkpoints/undo-plan.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

function checkpointJson(checkpoint, fingerprint) {
  if (checkpoint === null || checkpoint === undefined) {
    return null;
  }
  return {
    id: checkpoint.id,
    operation: checkpoint.operation,
    state: checkpoint.state,
    relativePath: checkpoint.relativePath,
    before: {
      exists: checkpoint.before.exists,
      sha256: checkpoint.before.sha256,
      byteLength: checkpoint.before.byteLength,
    },
    after: {
      exists: checkpoint.after.exists,
      sha256: checkpoint.after.sha256,
      byteLength: checkpoint.after.byteLength,
    },
    preview: {
      addedLines: checkpoint.preview.addedLines,
      removedLines: checkpoint.preview.removedLines,
    },
    fingerprintValid: checkpoint.workspaceFingerprint === fingerprint,
  };
}

function writeFixtureCheckpoint(root, fingerprint, spec) {
  const id = spec.id;
  const directory = join(root, fingerprint, id);
  mkdirSync(directory, { recursive: true });
  if (spec.raw !== undefined) {
    writeFileSync(join(directory, "metadata.json"), spec.raw, "utf8");
    return;
  }
  if (spec.recordJson !== undefined) {
    const resolved = spec.recordJson.replace("__FINGERPRINT__", fingerprint);
    writeFileSync(join(directory, "metadata.json"), resolved, "utf8");
    return;
  }
  const record = spec.record;
  const stored = {
    version: 1,
    id,
    workspaceFingerprint: spec.foreignFingerprint === true ? "0".repeat(64) : fingerprint,
    relativePath: record.relativePath,
    operation: record.operation,
    toolName: record.toolName,
    createdAt: record.createdAt,
    state: record.state,
    before: record.before,
    after: record.after,
    preview: record.preview,
  };
  writeFileSync(join(directory, "metadata.json"), `${JSON.stringify(stored, null, 2)}\n`, "utf8");
}

function decisionJson(decision) {
  if (decision.decision === "ready") {
    return `ready_${decision.action}`;
  }
  return "conflict";
}

async function run(input) {
  const workspace = mkdtempSync(join(tmpdir(), "siralos-oracle-cpws-"));
  const storeRoot = mkdtempSync(join(tmpdir(), "siralos-oracle-cproot-"));
  for (const [path, content] of Object.entries(input.workspaceFiles ?? {})) {
    mkdirSync(join(workspace, path.split("/").slice(0, -1).join("/")), { recursive: true });
    writeFileSync(join(workspace, path), content, "utf8");
  }
  const store = await createFilesystemCheckpointStore({
    workspaceRoot: workspace,
    rootDirectory: storeRoot,
  });
  const canonicalWorkspace = realpathSync(workspace);
  const fingerprint = createHash("sha256").update(canonicalWorkspace).digest("hex");
  for (const spec of input.checkpoints ?? []) {
    writeFixtureCheckpoint(storeRoot, fingerprint, spec);
  }
  const ops = [];
  for (const op of input.ops ?? []) {
    if (op.op === "list" || op.op === "list-after") {
      const checkpoints = await store.list(
        op.states === undefined ? undefined : { states: op.states },
      );
      ops.push({
        op: op.op,
        checkpoints: checkpoints.map((checkpoint) => checkpointJson(checkpoint, fingerprint)),
      });
      continue;
    }
    if (op.op === "get") {
      ops.push({ op: "get", checkpoint: checkpointJson(await store.get(op.id), fingerprint) });
      continue;
    }
    if (op.op === "reconcile") {
      const report = await reconcileWorkspaceCheckpoints({ workspaceRoot: workspace, store });
      ops.push({
        op: "reconcile",
        checked: report.checked,
        abandoned: report.abandoned,
        applied: report.applied,
        uncertain: report.uncertain,
        undoneAfterRestore: report.undoneAfterRestore,
      });
      continue;
    }
    if (op.op === "undo-plan") {
      const checkpoint = await store.get(op.id);
      if (checkpoint === null) {
        throw new Error(`undo-plan requires a valid checkpoint ${op.id}`);
      }
      ops.push({
        op: "undo-plan",
        decision: decisionJson(
          planUndo(checkpoint, { exists: op.current.exists, sha256: op.current.sha256 }),
        ),
      });
      continue;
    }
    throw new Error(`unsupported checkpoint op ${op.op}`);
  }
  rmSync(workspace, { recursive: true, force: true });
  rmSync(storeRoot, { recursive: true, force: true });
  return { ops };
}

const input = readStdinBounded();
process.stdout.write(`${JSON.stringify(await run(input))}\n`);
