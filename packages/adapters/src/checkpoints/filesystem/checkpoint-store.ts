import { randomUUID, createHash } from "node:crypto";
import { mkdir, open, readFile, realpath, rename, lstat } from "node:fs/promises";
import { homedir } from "node:os";
import { basename, dirname, join, relative as pathRelative, sep } from "node:path";
import type {
  AppliedCheckpointResult,
  CheckpointListQuery,
  CheckpointOperation,
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
const MAX_TOOL_NAME_LENGTH = 256;
const MAX_CREATED_AT_LENGTH = 64;
const SHA256_PATTERN = /^[0-9a-f]{64}$/;
const CHECKPOINT_STATES: readonly string[] = [
  "prepared",
  "applied",
  "undone",
  "abandoned",
  "conflicted",
  "uncertain",
];
const CHECKPOINT_OPERATIONS: readonly string[] = ["create", "update", "delete"];

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
    return validateMetadata(parsed, id, workspaceFingerprint, maxPreimageBytes);
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
   *
   * Every existing checkpoint must be fully inspectable: any entry whose
   * metadata is invalid, unreadable, oversized, or linked — or whose
   * preimage presence or actual size disagrees with its validated metadata
   * — makes capacity unverifiable and refuses the new checkpoint. Byte
   * accounting uses the ACTUAL preimage size observed on the filesystem,
   * never the metadata-declared byte count alone.
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
    let totalBytes = 0;
    for (const name of names) {
      if (signal?.aborted) {
        throw new DOMException("Checkpoint preparation was aborted.", "AbortError");
      }
      if (Date.now() >= deadline) {
        throw new CheckpointStorageLimitError(
          `Checkpoint storage limits could not be verified within the pass deadline; refusing to create a checkpoint. No checkpoint or filesystem entry was deleted.`,
        );
      }
      // One unreadable or inconsistent checkpoint makes the whole capacity
      // unverifiable: nothing may be skipped or assumed to consume zero
      // bytes.
      let preimageSize: number;
      try {
        const checkpoint = await loadMetadata(name);
        const expectsPreimage = checkpoint.before.exists;
        const preimagePath = join(checkpointPath(checkpoint.id), "preimage.bin");
        // ENOENT means "no preimage"; ANY other inspection error makes the
        // entry's storage contribution unknowable and must fail closed
        // rather than be assumed to consume zero bytes.
        const preimageStats = await lstat(preimagePath).catch((error: unknown) => {
          if (isNotFoundError(error)) {
            return null;
          }
          throw error;
        });
        if (preimageStats === null) {
          if (expectsPreimage) {
            throw new Error(
              `Checkpoint ${name} metadata declares a preimage but its preimage.bin is missing.`,
            );
          }
          preimageSize = 0;
        } else {
          await assertCheckpointPathSecure(preimagePath);
          if (preimageStats.isSymbolicLink() || !preimageStats.isFile()) {
            throw new Error(`Checkpoint ${name} preimage is not a regular file.`);
          }
          if (!expectsPreimage) {
            throw new Error(
              `Checkpoint ${name} metadata declares no preimage but a preimage.bin exists.`,
            );
          }
          if (preimageStats.size > maxPreimageBytes) {
            throw new Error(`Checkpoint ${name} preimage is oversized.`);
          }
          if (preimageStats.size !== checkpoint.before.byteLength) {
            throw new Error(
              `Checkpoint ${name} preimage size (${preimageStats.size} bytes) disagrees with its metadata (${checkpoint.before.byteLength} bytes).`,
            );
          }
          preimageSize = preimageStats.size;
        }
      } catch (error: unknown) {
        throw new CheckpointStorageLimitError(
          `Checkpoint storage capacity is unverifiable (${describeError(error)}); refusing to create a checkpoint. No checkpoint or filesystem entry was written or deleted; existing checkpoints are preserved for manual inspection.`,
        );
      }
      totalBytes += preimageSize;
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
      if (
        checkpoint.before.byteLength === null ||
        checkpoint.before.bytes.byteLength !== checkpoint.before.byteLength
      ) {
        throw new Error("Checkpoint preimage size does not match the recorded byte length.");
      }
      if (checkpoint.before.bytes.byteLength > maxPreimageBytes) {
        throw new Error("Checkpoint preimage exceeds the size limit.");
      }
      const hash = createHash("sha256").update(checkpoint.before.bytes).digest("hex");
      if (hash !== checkpoint.before.sha256) {
        throw new Error("Checkpoint preimage hash does not match the recorded before hash.");
      }
    }
    // The written metadata must always satisfy validateMetadata: an
    // out-of-contract caller must not be able to poison retention with
    // metadata the store itself would refuse to read.
    if (
      typeof checkpoint.toolName !== "string" ||
      checkpoint.toolName.length === 0 ||
      checkpoint.toolName.length > MAX_TOOL_NAME_LENGTH
    ) {
      throw new Error("Checkpoint tool name is invalid.");
    }
    const { addedLines, removedLines } = checkpoint.preview;
    if (!isSafeNonNegativeInteger(addedLines) || !isSafeNonNegativeInteger(removedLines)) {
      throw new Error("Checkpoint preview counts are invalid.");
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

/**
 * Full structural validation of checkpoint metadata. Every field used by
 * retention and lifecycle logic is validated before the result is built;
 * nothing is cast from a partially checked generic record. Byte lengths
 * must be safe non-negative integers (the before-state additionally within
 * the configured preimage bound), SHA-256 fields must be 64 hex digits,
 * and `null` is allowed only where the model permits it (a state that does
 * not exist carries neither a hash nor a byte length).
 */
function validateMetadata(
  parsed: unknown,
  id: string,
  workspaceFingerprint: string,
  maxPreimageBytes: number,
): FileCheckpoint {
  const record = asRecord(parsed, "Checkpoint metadata", id);
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
  if (!isCheckpointState(state) || !isCheckpointOperation(operation)) {
    throw new Error(`Checkpoint state or operation is invalid: ${id}.`);
  }
  const toolName = record["toolName"];
  if (
    typeof toolName !== "string" ||
    toolName.length === 0 ||
    toolName.length > MAX_TOOL_NAME_LENGTH
  ) {
    throw new Error(`Checkpoint tool name is invalid: ${id}.`);
  }
  const createdAt = record["createdAt"];
  if (
    typeof createdAt !== "string" ||
    createdAt.length === 0 ||
    createdAt.length > MAX_CREATED_AT_LENGTH ||
    Number.isNaN(Date.parse(createdAt))
  ) {
    throw new Error(`Checkpoint creation timestamp is invalid: ${id}.`);
  }
  const before = validateFileState(record["before"], id, "before", maxPreimageBytes);
  const after = validateFileState(record["after"], id, "after", maxPreimageBytes);
  const preview = asRecord(record["preview"], "preview", id);
  const addedLines = preview["addedLines"];
  const removedLines = preview["removedLines"];
  if (!isSafeNonNegativeInteger(addedLines) || !isSafeNonNegativeInteger(removedLines)) {
    throw new Error(`Checkpoint preview counts are invalid: ${id}.`);
  }
  return {
    version: 1,
    id,
    workspaceFingerprint,
    relativePath,
    operation,
    toolName,
    createdAt,
    state,
    before,
    after,
    preview: { addedLines, removedLines },
  };
}

function validateFileState(
  value: unknown,
  id: string,
  label: "before" | "after",
  maxPreimageBytes: number,
): { exists: boolean; sha256: string | null; byteLength: number | null } {
  const record = asRecord(value, `${label} file state`, id);
  const exists = record["exists"];
  if (typeof exists !== "boolean") {
    throw new Error(`Checkpoint ${label} existence flag is invalid: ${id}.`);
  }
  const sha256 = record["sha256"];
  const byteLength = record["byteLength"];
  if (exists) {
    if (typeof sha256 !== "string" || !SHA256_PATTERN.test(sha256)) {
      throw new Error(`Checkpoint ${label} sha256 is invalid: ${id}.`);
    }
    if (!isSafeNonNegativeInteger(byteLength)) {
      throw new Error(`Checkpoint ${label} byte length is invalid: ${id}.`);
    }
    if (label === "before" && byteLength > maxPreimageBytes) {
      throw new Error(`Checkpoint ${label} byte length exceeds the configured maximum: ${id}.`);
    }
  } else if (sha256 !== null || byteLength !== null) {
    throw new Error(
      `Checkpoint ${label} carries a hash or byte length for a state that does not exist: ${id}.`,
    );
  }
  return {
    exists,
    sha256: exists ? sha256 : null,
    byteLength: exists ? byteLength : null,
  };
}

function asRecord(value: unknown, label: string, id: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    throw new Error(`Checkpoint ${label} is malformed: ${id}.`);
  }
  return value as Record<string, unknown>;
}

function isSafeNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isCheckpointState(value: unknown): value is CheckpointState {
  return typeof value === "string" && CHECKPOINT_STATES.includes(value);
}

function isCheckpointOperation(value: unknown): value is CheckpointOperation {
  return typeof value === "string" && CHECKPOINT_OPERATIONS.includes(value);
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

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}
