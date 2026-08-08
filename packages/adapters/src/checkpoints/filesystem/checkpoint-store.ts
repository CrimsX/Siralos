import { randomUUID, createHash } from "node:crypto";
import { mkdir, open, readFile, realpath, rename, lstat } from "node:fs/promises";
import { homedir } from "node:os";
import { basename, dirname, join, relative as pathRelative, sep } from "node:path";
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
import { enumerateDirectoryBounded } from "../../fs/directory-enumeration.js";

export interface FilesystemCheckpointStoreOptions {
  readonly workspaceRoot: string;
  readonly rootDirectory?: string;
  readonly maxCheckpoints?: number;
  readonly maxStorageBytes?: number;
  readonly maxPreimageBytes?: number;
  /** Wall-clock budget for one retention pass in ms (default 5000); a
   * negative value expires the pass immediately. Test-visible so the
   * expired-deadline fail-closed path is exercised deterministically. */
  readonly retentionDeadlineMs?: number;
}

export const DEFAULT_CHECKPOINT_ROOT = join(homedir(), ".solaris", "checkpoints");
export const DEFAULT_MAX_CHECKPOINTS = 100;
export const DEFAULT_MAX_CHECKPOINT_STORAGE_BYTES = 100 * 1024 * 1024;
export const DEFAULT_MAX_PREIMAGE_BYTES = 1024 * 1024;
const MAX_METADATA_BYTES = 64 * 1024;

/** Wall-clock budget for one list or retention pass. */
const CHECKPOINT_PASS_DEADLINE_MS = 5_000;

/**
 * Typed storage-limit failure. Automatic checkpoint-retention deletion is
 * disabled: reaching the checkpoint count or byte limit never deletes any
 * checkpoint or filesystem entry, and every existing checkpoint is
 * preserved for manual inspection. The caller must surface this as an
 * explicit storage-limit refusal.
 */
export class CheckpointStorageLimitError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CheckpointStorageLimitError";
  }
}

export async function createFilesystemCheckpointStore(
  options: FilesystemCheckpointStoreOptions,
): Promise<CheckpointStore> {
  const maxCheckpoints = options.maxCheckpoints ?? DEFAULT_MAX_CHECKPOINTS;
  const maxStorageBytes = options.maxStorageBytes ?? DEFAULT_MAX_CHECKPOINT_STORAGE_BYTES;
  const maxPreimageBytes = options.maxPreimageBytes ?? DEFAULT_MAX_PREIMAGE_BYTES;
  const retentionDeadlineMs = options.retentionDeadlineMs ?? CHECKPOINT_PASS_DEADLINE_MS;
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
  const rootStats = await lstat(canonicalRoot).catch(() => null);
  if (rootStats === null || rootStats.isSymbolicLink()) {
    throw new Error("The checkpoint root must not be a symbolic link.");
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

  /**
   * No-follow validation of a path under the Solaris-owned checkpoint root.
   * Every component is lstat-checked (rejecting symlinks), the leaf must
   * resolve canonically to its logical location (rejecting junctions and
   * reparse points on Windows and any inserted link on POSIX), and the root
   * itself is re-validated as a non-link directory outside the workspace on
   * every operation so a link inserted after startup is detected.
   */
  async function assertCheckpointPathSecure(directory: string): Promise<void> {
    const rootStats = await lstat(rootDirectory).catch(() => null);
    if (rootStats === null || rootStats.isSymbolicLink() || !rootStats.isDirectory()) {
      throw new Error("The checkpoint root is missing or a symbolic link.");
    }
    const canonicalRootNow = await realpath(rootDirectory);
    if (
      canonicalRootNow !== rootDirectory ||
      isInside(canonicalWorkspace, canonicalRootNow) ||
      canonicalRootNow === canonicalWorkspace
    ) {
      throw new Error("The checkpoint root moved or resolves inside the workspace.");
    }
    const relative = pathRelative(rootDirectory, directory);
    const components = relative.split(sep).filter((component) => component.length > 0);
    let current = rootDirectory;
    for (const component of components) {
      current = join(current, component);
      const stats = await lstat(current).catch(() => null);
      if (stats === null) {
        throw new Error(`Checkpoint path is missing: ${current}`);
      }
      if (stats.isSymbolicLink()) {
        throw new Error(`Checkpoint path contains a symbolic link: ${current}`);
      }
    }
    const parentCanonical = await realpath(dirname(directory));
    const leafCanonical = await realpath(directory);
    if (leafCanonical !== join(parentCanonical, basename(directory))) {
      throw new Error(`Checkpoint path is a junction or reparse point: ${directory}`);
    }
    if (!isInside(rootDirectory, leafCanonical)) {
      throw new Error(`Checkpoint path escapes the checkpoint root: ${directory}`);
    }
  }

  async function loadMetadata(id: string): Promise<FileCheckpoint> {
    const directory = checkpointPath(id);
    await assertCheckpointPathSecure(directory);
    const dirStats = await lstat(directory).catch(() => null);
    if (dirStats === null) {
      throw new Error(`Unknown checkpoint: ${id}.`);
    }
    if (dirStats.isSymbolicLink()) {
      throw new Error(`Checkpoint path is a symbolic link: ${id}.`);
    }
    const metadataPath = join(directory, "metadata.json");
    await assertCheckpointPathSecure(metadataPath);
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
    // Post-read byte re-check: a metadata file grown or swapped after the
    // lstat is rejected even though it was already read.
    if (Buffer.byteLength(raw, "utf8") > MAX_METADATA_BYTES) {
      throw new Error(`Checkpoint metadata is oversized: ${id}.`);
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
    await assertCheckpointPathSecure(directory);
    const temporaryPath = join(directory, `metadata.json.tmp-${randomUUID()}`);
    const metadataPath = join(directory, "metadata.json");
    const handle = await open(temporaryPath, "wx", 0o600);
    try {
      await handle.writeFile(JSON.stringify(checkpoint, null, 2), "utf8");
      await handle.sync();
    } finally {
      await handle.close();
    }
    await rename(temporaryPath, metadataPath);
  }

  /**
   * Fail-closed retention enforcement. Automatic retention deletion is
   * disabled: this pass only MEASURES whether the checkpoint count and byte
   * limits can accommodate one more checkpoint. When a limit would be
   * exceeded, it throws {@link CheckpointStorageLimitError} and deletes
   * nothing — every existing checkpoint is preserved for manual inspection.
   * A truncated enumeration (the checkpoint directory holds more entries
   * than the retention cap plus one, or the pass deadline expired) also
   * fails closed: capacity cannot be proven, so no new checkpoint is
   * created. Refusal always happens before any write.
   */
  async function assertRetentionCapacity(addedBytes: number, signal?: AbortSignal): Promise<void> {
    if (signal?.aborted) {
      throw new DOMException("Checkpoint preparation was aborted.", "AbortError");
    }
    const names: string[] = [];
    const deadline = Date.now() + retentionDeadlineMs;
    // The enumeration is incremental and capped at the retention maximum
    // plus one, so a hostile directory can never be materialized.
    const outcome = await enumerateDirectoryBounded({
      directory: checkpointDirectory,
      maxEntries: maxCheckpoints + 1,
      signal,
      deadline,
      onEntry: (entry) => {
        if (entry.isDirectory() && entry.name.startsWith("cp_")) {
          names.push(entry.name);
        }
      },
    });
    if (outcome.truncated) {
      throw new CheckpointStorageLimitError(
        `Checkpoint storage limits cannot be proven (the checkpoint directory enumeration was truncated); refusing to create a checkpoint. No checkpoint or filesystem entry was deleted.`,
      );
    }
    const loaded: FileCheckpoint[] = [];
    for (const name of names) {
      if (signal?.aborted) {
        throw new DOMException("Checkpoint preparation was aborted.", "AbortError");
      }
      if (Date.now() >= deadline) {
        throw new CheckpointStorageLimitError(
          `Checkpoint storage limits could not be verified within the pass deadline; refusing to create a checkpoint. No checkpoint or filesystem entry was deleted.`,
        );
      }
      try {
        loaded.push(await loadMetadata(name));
      } catch {
        // An unreadable checkpoint still occupies storage; it counts toward
        // the retention cap through `names` below, and capacity cannot be
        // proven for it, so the count check fails closed.
      }
    }
    let totalBytes = 0;
    for (const checkpoint of loaded) {
      if (checkpoint.before.exists && checkpoint.before.byteLength !== null) {
        totalBytes += checkpoint.before.byteLength;
      }
    }
    const wouldExceed =
      names.length + 1 > maxCheckpoints || totalBytes + addedBytes > maxStorageBytes;
    if (wouldExceed) {
      throw new CheckpointStorageLimitError(
        `Checkpoint storage limits would be exceeded (${names.length + 1} checkpoints against a ${maxCheckpoints}-checkpoint limit, ${totalBytes + addedBytes} bytes against a ${maxStorageBytes}-byte limit); refusing to create a checkpoint. No checkpoint or filesystem entry was deleted; existing checkpoints are preserved for manual inspection.`,
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
    await assertRetentionCapacity(checkpoint.before.byteLength ?? 0, signal);
    const directory = checkpointPath(id);
    await mkdir(directory, { recursive: true, mode: 0o700 });
    await assertCheckpointPathSecure(directory);
    if (checkpoint.before.bytes !== null) {
      const preimagePath = join(directory, "preimage.bin");
      const handle = await open(preimagePath, "wx", 0o600);
      try {
        await handle.writeFile(checkpoint.before.bytes);
        await handle.sync();
      } finally {
        await handle.close();
      }
      await assertCheckpointPathSecure(preimagePath);
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
    const names: string[] = [];
    const deadline = Date.now() + CHECKPOINT_PASS_DEADLINE_MS;
    // The enumeration is incremental and capped; unreadable or non-checkpoint
    // entries are counted but never materialized beyond the retention cap.
    await enumerateDirectoryBounded({
      directory: checkpointDirectory,
      maxEntries: maxCheckpoints + 1,
      deadline,
      onEntry: (entry) => {
        if (entry.isDirectory() && entry.name.startsWith("cp_")) {
          names.push(entry.name);
        }
      },
    });
    const checkpoints: FileCheckpoint[] = [];
    for (const name of names) {
      if (Date.now() >= deadline) {
        break;
      }
      try {
        checkpoints.push(await loadMetadata(name));
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
    await assertCheckpointPathSecure(preimagePath);
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
    // Post-read byte re-check: a preimage grown or swapped after the lstat
    // is rejected even though it was already read.
    if (bytes.length > maxPreimageBytes) {
      throw new Error(`Checkpoint preimage is oversized: ${checkpointId}.`);
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
