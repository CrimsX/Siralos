import { randomUUID, createHash } from "node:crypto";
import { mkdir, open, readFile, readdir, realpath, rename, rm, lstat } from "node:fs/promises";
import { homedir } from "node:os";
import { join, sep } from "node:path";
import type {
  AppliedCheckpointResult,
  CheckpointListQuery,
  CheckpointState,
  CheckpointStore,
  CheckpointTerminalState,
  FileCheckpoint,
  PreparedCheckpoint,
} from "@solaris/core";
import { validateRelativeWorkspacePath } from "../../tools/workspace/mutations/mutation-paths.js";

export interface FilesystemCheckpointStoreOptions {
  readonly workspaceRoot: string;
  readonly rootDirectory?: string;
  readonly maxCheckpoints?: number;
  readonly maxStorageBytes?: number;
  readonly maxPreimageBytes?: number;
}

export const DEFAULT_CHECKPOINT_ROOT = join(homedir(), ".solaris", "checkpoints");
export const DEFAULT_MAX_CHECKPOINTS = 100;
export const DEFAULT_MAX_CHECKPOINT_STORAGE_BYTES = 100 * 1024 * 1024;
export const DEFAULT_MAX_PREIMAGE_BYTES = 1024 * 1024;
const MAX_METADATA_BYTES = 64 * 1024;

const PRUNE_ORDER: readonly CheckpointState[] = ["undone", "abandoned", "applied"];

export async function createFilesystemCheckpointStore(
  options: FilesystemCheckpointStoreOptions,
): Promise<CheckpointStore> {
  const maxCheckpoints = options.maxCheckpoints ?? DEFAULT_MAX_CHECKPOINTS;
  const maxStorageBytes = options.maxStorageBytes ?? DEFAULT_MAX_CHECKPOINT_STORAGE_BYTES;
  const maxPreimageBytes = options.maxPreimageBytes ?? DEFAULT_MAX_PREIMAGE_BYTES;
  let canonicalWorkspace: string;
  let canonicalRoot: string;
  try {
    canonicalWorkspace = await realpath(options.workspaceRoot);
  } catch (error: unknown) {
    throw new Error(`Checkpoint store paths cannot be resolved: ${describeError(error)}`);
  }
  const requestedRoot = options.rootDirectory ?? DEFAULT_CHECKPOINT_ROOT;
  try {
    canonicalRoot = await realpath(requestedRoot);
  } catch {
    await mkdir(requestedRoot, { recursive: true, mode: 0o700 });
    canonicalRoot = await realpath(requestedRoot);
  }
  if (isInside(canonicalWorkspace, canonicalRoot) || canonicalRoot === canonicalWorkspace) {
    throw new Error("The checkpoint store must not resolve inside the active workspace.");
  }
  const workspaceFingerprint = createHash("sha256").update(canonicalWorkspace).digest("hex");
  const rootDirectory = canonicalRoot;
  const checkpointDirectory = join(rootDirectory, workspaceFingerprint);

  function checkpointPath(id: string): string {
    assertValidCheckpointId(id);
    return join(checkpointDirectory, id);
  }

  async function loadMetadata(id: string): Promise<FileCheckpoint> {
    const directory = checkpointPath(id);
    const dirStats = await lstat(directory).catch(() => null);
    if (dirStats === null) {
      throw new Error(`Unknown checkpoint: ${id}.`);
    }
    if (dirStats.isSymbolicLink()) {
      throw new Error(`Checkpoint path is a symbolic link: ${id}.`);
    }
    const metadataPath = join(directory, "metadata.json");
    const metadataStats = await lstat(metadataPath).catch(() => null);
    if (metadataStats === null || metadataStats.isSymbolicLink()) {
      throw new Error(`Checkpoint metadata is missing or a symbolic link: ${id}.`);
    }
    if (metadataStats.size > MAX_METADATA_BYTES) {
      throw new Error(`Checkpoint metadata is oversized: ${id}.`);
    }
    let raw: string;
    try {
      raw = await readFile(metadataPath, "utf8");
    } catch (error: unknown) {
      throw new Error(`Checkpoint metadata cannot be read: ${describeError(error)}`);
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      throw new Error(`Checkpoint metadata is not valid JSON: ${id}.`);
    }
    return validateMetadata(parsed, id, workspaceFingerprint);
  }

  async function writeMetadata(checkpoint: FileCheckpoint): Promise<void> {
    const directory = checkpointPath(checkpoint.id);
    const temporaryPath = join(directory, `metadata.json.tmp-${randomUUID()}`);
    const metadataPath = join(directory, "metadata.json");
    const handle = await open(temporaryPath, "wx");
    try {
      await handle.writeFile(JSON.stringify(checkpoint, null, 2), "utf8");
      await handle.sync();
    } finally {
      await handle.close();
    }
    await rename(temporaryPath, metadataPath);
  }

  async function pruneIfNeeded(addedBytes: number): Promise<void> {
    const entries = await readdir(checkpointDirectory).catch(() => []);
    const checkpoints: FileCheckpoint[] = [];
    for (const entry of entries) {
      try {
        checkpoints.push(await loadMetadata(entry));
      } catch {
        // skip unreadable entries during pruning
      }
    }
    let totalBytes = 0;
    for (const checkpoint of checkpoints) {
      if (checkpoint.before.exists && checkpoint.before.byteLength !== null) {
        totalBytes += checkpoint.before.byteLength;
      }
    }
    const wouldExceed =
      checkpoints.length + 1 > maxCheckpoints || totalBytes + addedBytes > maxStorageBytes;
    if (!wouldExceed) {
      return;
    }
    const terminal = PRUNE_ORDER.flatMap((state) =>
      checkpoints
        .filter((checkpoint) => checkpoint.state === state)
        .sort((a, b) => (a.createdAt < b.createdAt ? -1 : 1)),
    );
    for (const checkpoint of terminal) {
      if (checkpoints.length + 1 <= maxCheckpoints && totalBytes + addedBytes <= maxStorageBytes) {
        break;
      }
      await rm(checkpointPath(checkpoint.id), { recursive: true, force: true });
      totalBytes -= checkpoint.before.byteLength ?? 0;
      checkpoints.splice(checkpoints.indexOf(checkpoint), 1);
    }
    if (checkpoints.length + 1 > maxCheckpoints || totalBytes + addedBytes > maxStorageBytes) {
      throw new Error(
        "Checkpoint storage limits were reached and terminal checkpoints could not free enough space.",
      );
    }
  }

  async function prepare(
    checkpoint: PreparedCheckpoint,
    signal?: AbortSignal,
  ): Promise<FileCheckpoint> {
    if (signal?.aborted) {
      throw new DOMException("Checkpoint preparation was aborted.", "AbortError");
    }
    const pathValidation = validateRelativeWorkspacePath(checkpoint.relativePath);
    if (pathValidation !== null) {
      throw new Error(`Invalid checkpoint path: ${pathValidation}`);
    }
    if (checkpoint.before.bytes !== null) {
      if (checkpoint.before.bytes.byteLength > maxPreimageBytes) {
        throw new Error("Checkpoint preimage exceeds the size limit.");
      }
      const hash = createHash("sha256").update(checkpoint.before.bytes).digest("hex");
      if (hash !== checkpoint.before.sha256) {
        throw new Error("Checkpoint preimage hash does not match the recorded before hash.");
      }
    }
    const id = `cp_${randomUUID()}`;
    const createdAt = new Date().toISOString();
    const stored: FileCheckpoint = {
      version: 1,
      id,
      workspaceFingerprint,
      relativePath: checkpoint.relativePath,
      operation: checkpoint.operation,
      toolName: checkpoint.toolName,
      createdAt,
      state: "prepared",
      before: {
        exists: checkpoint.before.exists,
        sha256: checkpoint.before.sha256,
        byteLength: checkpoint.before.byteLength,
      },
      after: checkpoint.after,
      preview: checkpoint.preview,
    };
    await pruneIfNeeded(checkpoint.before.byteLength ?? 0);
    const directory = checkpointPath(id);
    await mkdir(directory, { recursive: true, mode: 0o700 });
    if (checkpoint.before.bytes !== null) {
      const handle = await open(join(directory, "preimage.bin"), "wx", 0o600);
      try {
        await handle.writeFile(checkpoint.before.bytes);
        await handle.sync();
      } finally {
        await handle.close();
      }
    }
    await writeMetadata(stored);
    return stored;
  }

  async function finalizeApplied(
    checkpointId: string,
    result: AppliedCheckpointResult,
  ): Promise<FileCheckpoint> {
    const checkpoint = await loadMetadata(checkpointId);
    assertState(checkpoint, ["prepared"]);
    const afterMatches =
      result.absent === !checkpoint.after.exists &&
      (checkpoint.after.exists ? result.afterSha256 === checkpoint.after.sha256 : true);
    if (!afterMatches) {
      throw new Error(
        `Checkpoint ${checkpointId} could not be finalized: the recorded after-state does not match the applied result.`,
      );
    }
    const updated: FileCheckpoint = { ...checkpoint, state: "applied" };
    await writeMetadata(updated);
    return updated;
  }

  async function markUndone(checkpointId: string): Promise<FileCheckpoint> {
    const checkpoint = await loadMetadata(checkpointId);
    assertState(checkpoint, ["prepared", "applied"]);
    const updated: FileCheckpoint = { ...checkpoint, state: "undone" };
    await writeMetadata(updated);
    return updated;
  }

  async function markState(
    checkpointId: string,
    state: CheckpointTerminalState,
  ): Promise<FileCheckpoint> {
    const checkpoint = await loadMetadata(checkpointId);
    const targetStates: readonly CheckpointTerminalState[] =
      checkpoint.state === "prepared"
        ? ["applied", "abandoned", "conflicted", "uncertain"]
        : checkpoint.state === "applied"
          ? ["uncertain"]
          : [];
    if (!targetStates.includes(state)) {
      throw new Error(
        `Checkpoint ${checkpointId} is in state ${checkpoint.state}, which cannot transition to ${state}.`,
      );
    }
    const updated: FileCheckpoint = { ...checkpoint, state };
    await writeMetadata(updated);
    return updated;
  }

  async function get(checkpointId: string): Promise<FileCheckpoint | null> {
    try {
      return await loadMetadata(checkpointId);
    } catch {
      return null;
    }
  }

  async function list(query?: CheckpointListQuery): Promise<readonly FileCheckpoint[]> {
    const entries = await readdir(checkpointDirectory).catch(() => []);
    const checkpoints: FileCheckpoint[] = [];
    for (const entry of entries) {
      try {
        checkpoints.push(await loadMetadata(entry));
      } catch {
        // skip unreadable entries
      }
    }
    const states = query?.states;
    const filtered =
      states === undefined
        ? checkpoints
        : checkpoints.filter((checkpoint) => states.includes(checkpoint.state));
    const sorted = filtered.sort((a, b) => (a.createdAt < b.createdAt ? 1 : -1));
    return query?.limit === undefined ? sorted : sorted.slice(0, query.limit);
  }

  async function loadPreimage(checkpointId: string): Promise<Uint8Array | null> {
    const checkpoint = await loadMetadata(checkpointId);
    if (!checkpoint.before.exists) {
      return null;
    }
    const preimagePath = join(checkpointPath(checkpointId), "preimage.bin");
    const stats = await lstat(preimagePath).catch(() => null);
    if (stats === null || stats.isSymbolicLink()) {
      throw new Error(`Checkpoint preimage is missing or a symbolic link: ${checkpointId}.`);
    }
    if (stats.size > maxPreimageBytes) {
      throw new Error(`Checkpoint preimage is oversized: ${checkpointId}.`);
    }
    let bytes: Buffer;
    try {
      bytes = await readFile(preimagePath);
    } catch (error: unknown) {
      throw new Error(`Checkpoint preimage cannot be read: ${describeError(error)}`);
    }
    const hash = createHash("sha256").update(bytes).digest("hex");
    if (checkpoint.before.sha256 !== null && hash !== checkpoint.before.sha256) {
      throw new Error(`Checkpoint preimage hash does not match its metadata: ${checkpointId}.`);
    }
    return new Uint8Array(bytes);
  }

  return { prepare, finalizeApplied, markUndone, markState, get, list, loadPreimage };
}

function validateMetadata(
  parsed: unknown,
  id: string,
  workspaceFingerprint: string,
): FileCheckpoint {
  if (typeof parsed !== "object" || parsed === null) {
    throw new Error(`Checkpoint metadata is malformed: ${id}.`);
  }
  const record = parsed as Record<string, unknown>;
  if (record["version"] !== 1 || record["id"] !== id) {
    throw new Error(`Checkpoint metadata version or id mismatch: ${id}.`);
  }
  if (record["workspaceFingerprint"] !== workspaceFingerprint) {
    throw new Error(`Checkpoint belongs to a different workspace: ${id}.`);
  }
  const relativePath = record["relativePath"];
  if (typeof relativePath !== "string" || validateRelativeWorkspacePath(relativePath) !== null) {
    throw new Error(`Checkpoint relative path is invalid: ${id}.`);
  }
  const state = record["state"];
  const operation = record["operation"];
  if (
    typeof state !== "string" ||
    !["prepared", "applied", "undone", "abandoned", "conflicted", "uncertain"].includes(state) ||
    typeof operation !== "string" ||
    !["create", "update", "delete"].includes(operation)
  ) {
    throw new Error(`Checkpoint state or operation is invalid: ${id}.`);
  }
  const before = record["before"];
  const after = record["after"];
  if (
    typeof before !== "object" ||
    before === null ||
    typeof after !== "object" ||
    after === null
  ) {
    throw new Error(`Checkpoint file states are malformed: ${id}.`);
  }
  const beforeRecord = before as Record<string, unknown>;
  const afterRecord = after as Record<string, unknown>;
  if (typeof beforeRecord["exists"] !== "boolean" || typeof afterRecord["exists"] !== "boolean") {
    throw new Error(`Checkpoint file states are malformed: ${id}.`);
  }
  return record as unknown as FileCheckpoint;
}

function assertState(checkpoint: FileCheckpoint, allowed: readonly CheckpointState[]): void {
  if (!allowed.includes(checkpoint.state)) {
    throw new Error(
      `Checkpoint ${checkpoint.id} is in state ${checkpoint.state}, which cannot transition here.`,
    );
  }
}

function assertValidCheckpointId(id: string): void {
  if (!/^cp_[0-9a-f-]{10,}$/.test(id)) {
    throw new Error(`Invalid checkpoint id: ${id}.`);
  }
}

function isInside(root: string, target: string): boolean {
  const rootPrefix = root.endsWith("/") || root.endsWith("\\") ? root : `${root}${sep}`;
  return target.startsWith(rootPrefix);
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "Unknown checkpoint store failure.";
}
