import { randomUUID, createHash } from "node:crypto";
import { constants as fsConstants } from "node:fs";
import { mkdir, open, readFile, realpath, rename, lstat } from "node:fs/promises";
import type { FileHandle } from "node:fs/promises";
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
} from "@siralos/core";
import { validateRelativeWorkspacePath } from "../../tools/workspace/mutations/mutation-paths.js";
import { enumerateDirectoryBounded } from "../../fs/directory-enumeration.js";
import { createErrorDescriber } from "../../support/error-message.js";

const describeError = createErrorDescriber("Unknown checkpoint store failure.");

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
  /**
   * Test-visible observation point invoked by retention capacity
   * verification between the preimage lstat and the open, so the
   * changed-during-inspection fail-closed path is exercised
   * deterministically. Never invoked by `loadPreimage()` or any other
   * store path.
   */
  readonly retentionPreimageInspectionHook?: (preimagePath: string) => Promise<void> | void;
  /**
   * Test-visible seam for the bounded read loop inside preimage
   * verification (retention capacity and `loadPreimage()` alike), so
   * short reads, read errors, and mid-verification filesystem changes are
   * exercised deterministically. The production default calls the real
   * handle reader.
   */
  readonly preimageReadSeam?: PreimageReadFunction | undefined;
}

export const DEFAULT_CHECKPOINT_ROOT = join(homedir(), ".siralos", "checkpoints");
export const DEFAULT_MAX_CHECKPOINTS = 100;
export const DEFAULT_MAX_CHECKPOINT_STORAGE_BYTES = 100 * 1024 * 1024;
export const DEFAULT_MAX_PREIMAGE_BYTES = 1024 * 1024;
/**
 * Hard implementation cap for configured preimage limits. The bounded
 * verifier materializes at most `maxBytes + 1` bytes in a single buffer, so
 * an unchecked configuration value must never be able to drive a
 * proportional allocation: this cap keeps `maxBytes + 1` representable and
 * allocatable within a reasonable resource envelope, safely below Node's
 * maximum buffer length, and within the 100 MiB per-workspace storage
 * envelope. Store creation rejects larger configured values (never silently
 * clamped); the default limit sits at or below the cap.
 */
export const MAX_SUPPORTED_PREIMAGE_BYTES = 64 * 1024 * 1024;
const MAX_METADATA_BYTES = 64 * 1024;
const MAX_TOOL_NAME_LENGTH = 256;
const MAX_CREATED_AT_LENGTH = 64;
const SHA256_PATTERN = /^[0-9a-f]{64}$/;
const CHECKPOINT_ID_PATTERN = /^cp_[0-9a-f-]{10,}$/;
const CHECKPOINT_STATES: readonly string[] = [
  "prepared",
  "applied",
  "undone",
  "abandoned",
  "conflicted",
  "uncertain",
];
const CHECKPOINT_OPERATIONS: readonly string[] = ["create", "update", "delete"];

/**
 * The canonical operation-state invariant: an operation's meaning IS its
 * before/after existence transition. `create` records the absence of a
 * file that will appear; `update` records an existing file replaced by
 * another state; `delete` records an existing file that will be removed.
 */
const OPERATION_STATE_MATRIX: Record<
  CheckpointOperation,
  { readonly beforeExists: boolean; readonly afterExists: boolean }
> = {
  create: { beforeExists: false, afterExists: true },
  update: { beforeExists: true, afterExists: true },
  delete: { beforeExists: true, afterExists: false },
};

/**
 * Known per-checkpoint layout: `metadata.json` and `preimage.bin` only.
 * A checkpoint directory is enumerated with this cap, so a fourth entry
 * (junk, nested directories, leftovers) is provably unexpected.
 */
const CHECKPOINT_DIR_MAX_ENTRIES = 3;

const STORED_RECORD_KEYS = [
  "version",
  "id",
  "workspaceFingerprint",
  "relativePath",
  "operation",
  "toolName",
  "createdAt",
  "state",
  "before",
  "after",
  "preview",
] as const;
const FILE_STATE_KEYS = ["exists", "sha256", "byteLength"] as const;
const PREVIEW_KEYS = ["addedLines", "removedLines"] as const;
const PREPARED_RECORD_KEYS = [
  "relativePath",
  "operation",
  "toolName",
  "before",
  "after",
  "preview",
] as const;
const PREPARED_BEFORE_KEYS = ["exists", "sha256", "byteLength", "bytes"] as const;

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

/**
 * Overflow-safe addition of two safe non-negative byte totals. Throws a
 * `RangeError` when either operand is not a safe non-negative integer or
 * the sum exceeds `Number.MAX_SAFE_INTEGER`. Exported from this module so
 * the arithmetic boundary is directly testable; it is not part of the
 * package's public API.
 */
export function checkedByteTotal(a: number, b: number): number {
  if (!isSafeNonNegativeInteger(a) || !isSafeNonNegativeInteger(b)) {
    throw new RangeError("Byte totals must be safe non-negative integers.");
  }
  const total = a + b;
  if (total > Number.MAX_SAFE_INTEGER) {
    throw new RangeError("Byte totals exceed the safe integer range.");
  }
  return total;
}

/**
 * Immutable verification snapshot captured from the opened handle before
 * any byte is read. Same object identity (`dev`/`ino`) proves the pathname
 * and handle address the same object; the size and mutation timestamps
 * prove the object's CONTENT state did not change in place while it was
 * being read. Stable `dev`/`ino` alone never proves content stability: an
 * in-place rewrite preserves them, and only a size/mtime/ctime change
 * reveals it.
 */
export interface FileVerificationSnapshot {
  readonly dev: bigint;
  readonly ino: bigint;
  readonly size: bigint;
  readonly mtimeNs: bigint;
  readonly ctimeNs: bigint;
}

export type VerifiedPreimageOutcome =
  | { readonly status: "missing" }
  | { readonly status: "verified"; readonly bytes: Buffer }
  | { readonly status: "invalid"; readonly message: string }
  | { readonly status: "aborted" }
  | { readonly status: "expired" };

/**
 * Signature of the bounded read-loop operation inside preimage
 * verification: reads up to `length` bytes into `buffer` at `offset` from
 * the explicit `position` and resolves with the actual byte count
 * (0 means EOF). Mirrors `FileHandle.read`; the production default calls
 * the real handle reader, tests may substitute a seam.
 */
export type PreimageReadFunction = (
  handle: FileHandle,
  buffer: Buffer,
  offset: number,
  length: number,
  position: number,
) => Promise<number>;

async function defaultPreimageRead(
  handle: FileHandle,
  buffer: Buffer,
  offset: number,
  length: number,
  position: number,
): Promise<number> {
  const { bytesRead } = await handle.read(buffer, offset, length, position);
  return bytesRead;
}

/**
 * Read-only open flags for preimage verification. `O_NOFOLLOW` is applied
 * on POSIX (where Node exposes it) so a symlink inserted at the leaf
 * between inspection and opening makes the open itself fail; on Windows the
 * constant is unavailable, so the open may follow a reparse point and the
 * handle/path identity comparison rejects it instead. No stronger Windows
 * no-follow guarantee is claimed.
 */
const PREIMAGE_READ_FLAGS =
  process.platform === "win32" || typeof fsConstants.O_NOFOLLOW !== "number"
    ? fsConstants.O_RDONLY
    : fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW;

/**
 * Shared bounded preimage reading and hash verification. This is the single
 * content-verification primitive for BOTH retention capacity verification
 * and `loadPreimage()`.
 *
 * Identity binding: the pathname is lstat'd and its stable identity fields
 * (`dev`/`ino`, bigint) are captured before opening; the path must be a
 * non-link regular file of an acceptable size. The file is then opened
 * read-only (`O_NOFOLLOW` on POSIX) and the opened handle must be a regular
 * file whose identity equals the inspected snapshot, while a fresh path
 * lstat must still identify the same object. Content equality is never a
 * substitute for identity equality: a same-content replacement (hard link,
 * rename, symlink, junction, reparse point) is refused.
 *
 * Stability binding: immediately after opening (before any read) an
 * immutable snapshot is captured from the opened handle — `dev`, `ino`,
 * `size`, `mtimeNs`, `ctimeNs` (bigint nanoseconds). After the bounded read
 * loop, the opened handle must still be a regular file whose identity, size,
 * and mutation timestamps all equal that snapshot, and a fresh path lstat
 * must still be a non-link regular file matching the final handle snapshot.
 * A same-inode in-place rewrite during verification changes size and/or the
 * mutation timestamps and is refused; restoring `mtime` cannot hide a
 * changed `ctime`, which user space cannot set. After the final stability
 * verification the preimage's bytes are re-read once more from the same
 * handle and must still match the expected byte length and SHA-256: a
 * same-size in-place rewrite performed by another handle is not reliably
 * visible through metadata observed on a handle that has not read since the
 * rewrite (platform-dependent), so this second bounded read pass — which
 * runs after every mutation window, including EOF and the final-verification
 * hook — makes the fail-closed guarantee platform-independent. The guarantee
 * is detection of mutation between the pre-read snapshot and the final
 * content re-verification — never atomic protection against a modification
 * occurring after final verification.
 * If the platform's identity or stability fields are unusable (zero
 * `dev`/`ino`, or unrepresentable timestamps), verification fails closed
 * rather than claiming a binding it cannot prove.
 *
 * Bounded read loop: one short read is never treated as EOF. Reading
 * continues with an explicit offset until EOF, `maxBytes + 1` collected
 * bytes, cancellation, deadline expiry, a read error, or an identity
 * mismatch; at most `maxBytes + 1` bytes are ever materialized in a single
 * buffer (the cap enforced here is `MAX_SUPPORTED_PREIMAGE_BYTES`).
 * Cancellation and deadline are checked before every read and after the
 * loop. The handle stays open through the read and the final stability
 * verification, and is closed on every outcome.
 */
export async function verifyPreimageBounded(options: {
  readonly path: string;
  readonly expected: { readonly sha256: string | null; readonly byteLength: number | null };
  readonly maxBytes: number;
  readonly context: string;
  readonly signal?: AbortSignal | undefined;
  readonly deadline?: number | undefined;
  readonly onInspect?: ((path: string) => Promise<void> | void) | undefined;
  readonly read?: PreimageReadFunction | undefined;
  /**
   * Test-visible observation point invoked after the bounded read loop and
   * before the final stability verification, so an in-place rewrite after
   * EOF is exercised deterministically.
   */
  readonly finalVerificationHook?: ((path: string) => Promise<void> | void) | undefined;
  /**
   * Test-visible seam over the pre-read stability snapshot, so unusable or
   * mutated snapshot fields exercise the fail-closed path deterministically.
   */
  readonly snapshotSeam?:
    ((snapshot: FileVerificationSnapshot) => FileVerificationSnapshot | undefined) | undefined;
}): Promise<VerifiedPreimageOutcome> {
  if (
    !isSafeNonNegativeInteger(options.maxBytes) ||
    options.maxBytes > MAX_SUPPORTED_PREIMAGE_BYTES
  ) {
    return {
      status: "invalid",
      message: `Checkpoint ${options.context} preimage bound is unsupported (${String(options.maxBytes)}).`,
    };
  }
  let inspected;
  try {
    inspected = await lstat(options.path, { bigint: true });
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return { status: "missing" };
    }
    return {
      status: "invalid",
      message: `Checkpoint ${options.context} preimage cannot be inspected: ${describeError(error)}`,
    };
  }
  const beforeOpen = verificationInterruption(options.signal, options.deadline);
  if (beforeOpen !== null) {
    return beforeOpen;
  }
  if (inspected.isSymbolicLink() || !inspected.isFile()) {
    return {
      status: "invalid",
      message: `Checkpoint ${options.context} preimage is not a regular file.`,
    };
  }
  if (inspected.size > options.maxBytes) {
    return {
      status: "invalid",
      message: `Checkpoint ${options.context} preimage is oversized.`,
    };
  }
  if (options.onInspect !== undefined) {
    await options.onInspect(options.path);
  }
  const afterHook = verificationInterruption(options.signal, options.deadline);
  if (afterHook !== null) {
    return afterHook;
  }
  let handle;
  try {
    handle = await open(options.path, PREIMAGE_READ_FLAGS);
  } catch (error: unknown) {
    return {
      status: "invalid",
      message: `Checkpoint ${options.context} preimage cannot be opened: ${describeError(error)}`,
    };
  }
  try {
    // Post-open identity binding: the opened handle and a fresh path lstat
    // must both be the exact object that was inspected.
    const afterOpen = await verifyOpenedIdentity(
      handle,
      options.path,
      inspected.dev,
      inspected.ino,
      options.context,
    );
    if (afterOpen !== null) {
      return afterOpen;
    }
    // The immutable stability snapshot is captured from the opened handle
    // before any byte is read. Identity fields bind the object; size and
    // mutation timestamps bind its content state, so an in-place rewrite of
    // the same inode during reading is detected by the final stability
    // verification.
    let handleStats;
    try {
      handleStats = await handle.stat({ bigint: true });
    } catch (error: unknown) {
      return {
        status: "invalid",
        message: `Checkpoint ${options.context} opened preimage handle cannot be inspected: ${describeError(error)}`,
      };
    }
    if (handleStats.isSymbolicLink() || !handleStats.isFile()) {
      return {
        status: "invalid",
        message: `Checkpoint ${options.context} opened preimage object is not a regular file.`,
      };
    }
    let snapshot: FileVerificationSnapshot = {
      dev: handleStats.dev,
      ino: handleStats.ino,
      size: handleStats.size,
      mtimeNs: handleStats.mtimeNs,
      ctimeNs: handleStats.ctimeNs,
    };
    if (options.snapshotSeam !== undefined) {
      const altered = options.snapshotSeam(snapshot);
      if (altered !== undefined) {
        snapshot = altered;
      }
    }
    if (!isUsableVerificationSnapshot(snapshot)) {
      return {
        status: "invalid",
        message: `Checkpoint ${options.context} preimage stability fields are unusable on this filesystem; verification fails closed.`,
      };
    }
    const readFn = options.read ?? defaultPreimageRead;
    const buffer = Buffer.allocUnsafe(options.maxBytes + 1);
    let total = 0;
    while (true) {
      const interruption = verificationInterruption(options.signal, options.deadline);
      if (interruption !== null) {
        return interruption;
      }
      const remaining = buffer.length - total;
      if (remaining === 0) {
        break;
      }
      let bytesRead: number;
      try {
        bytesRead = await readFn(handle, buffer, total, remaining, total);
      } catch (error: unknown) {
        return {
          status: "invalid",
          message: `Checkpoint ${options.context} preimage cannot be read: ${describeError(error)}`,
        };
      }
      if (bytesRead === 0) {
        break; // EOF
      }
      if (!isSafeNonNegativeInteger(bytesRead) || bytesRead > remaining) {
        return {
          status: "invalid",
          message: `Checkpoint ${options.context} preimage read returned an invalid byte count.`,
        };
      }
      total += bytesRead;
    }
    if (total > options.maxBytes) {
      return {
        status: "invalid",
        message: `Checkpoint ${options.context} preimage is oversized.`,
      };
    }
    const afterRead = verificationInterruption(options.signal, options.deadline);
    if (afterRead !== null) {
      return afterRead;
    }
    if (options.finalVerificationHook !== undefined) {
      await options.finalVerificationHook(options.path);
    }
    const afterFinalHook = verificationInterruption(options.signal, options.deadline);
    if (afterFinalHook !== null) {
      return afterFinalHook;
    }
    // Final stability verification: the opened handle must still be the
    // exact object captured before reading, with identical size and
    // mutation timestamps (an in-place rewrite of the same inode changes
    // size and/or mtimeNs/ctimeNs and is refused), and the pathname must
    // still resolve to that exact object.
    const finalStability = await verifyOpenedStability(
      handle,
      options.path,
      snapshot,
      options.context,
    );
    if (finalStability !== null) {
      return finalStability;
    }
    const bytes = buffer.subarray(0, total);
    if (options.expected.byteLength !== null && bytes.length !== options.expected.byteLength) {
      return {
        status: "invalid",
        message: `Checkpoint ${options.context} preimage size (${bytes.length} bytes) disagrees with its metadata (${options.expected.byteLength} bytes).`,
      };
    }
    const hash = createHash("sha256").update(bytes).digest("hex");
    if (options.expected.sha256 !== null && hash !== options.expected.sha256) {
      return {
        status: "invalid",
        message: `Checkpoint ${options.context} preimage hash does not match its metadata.`,
      };
    }
    // Final content re-verification: the preimage's bytes are re-read once
    // more from the same handle and must still match the expected byte
    // length and SHA-256. A same-size in-place rewrite performed by
    // another handle is not reliably visible through metadata observed on
    // a handle that has not read since the rewrite, so the stability
    // comparison alone cannot close every mutation window; the second
    // bounded read pass runs after every mutation window (the final
    // data-bearing read, EOF, and the final-verification hook) and makes
    // the fail-closed guarantee independent of platform metadata caching.
    if (options.expected.sha256 !== null || options.expected.byteLength !== null) {
      const reverifyBuffer = Buffer.allocUnsafe(options.maxBytes + 1);
      let reverifiedTotal = 0;
      while (true) {
        const reverifyInterruption = verificationInterruption(options.signal, options.deadline);
        if (reverifyInterruption !== null) {
          return reverifyInterruption;
        }
        const reverifyRemaining = reverifyBuffer.length - reverifiedTotal;
        if (reverifyRemaining === 0) {
          break;
        }
        let reverifiedRead: number;
        try {
          reverifiedRead = await readFn(
            handle,
            reverifyBuffer,
            reverifiedTotal,
            reverifyRemaining,
            reverifiedTotal,
          );
        } catch (error: unknown) {
          return {
            status: "invalid",
            message: `Checkpoint ${options.context} preimage cannot be re-read: ${describeError(error)}`,
          };
        }
        if (reverifiedRead === 0) {
          break; // EOF
        }
        if (!isSafeNonNegativeInteger(reverifiedRead) || reverifiedRead > reverifyRemaining) {
          return {
            status: "invalid",
            message: `Checkpoint ${options.context} preimage re-read returned an invalid byte count.`,
          };
        }
        reverifiedTotal += reverifiedRead;
      }
      if (reverifiedTotal > options.maxBytes) {
        return {
          status: "invalid",
          message: `Checkpoint ${options.context} preimage is oversized.`,
        };
      }
      const reverifiedBytes = reverifyBuffer.subarray(0, reverifiedTotal);
      const reverifiedSizeMatches =
        options.expected.byteLength === null ||
        reverifiedBytes.length === options.expected.byteLength;
      const reverifiedHashMatches =
        options.expected.sha256 === null ||
        createHash("sha256").update(reverifiedBytes).digest("hex") === options.expected.sha256;
      if (!reverifiedSizeMatches || !reverifiedHashMatches) {
        return {
          status: "invalid",
          message: `Checkpoint ${options.context} preimage changed during verification (in-place mutation).`,
        };
      }
    }
    return { status: "verified", bytes };
  } finally {
    await handle.close().catch(() => undefined);
  }
}

/**
 * Platform-aware identity binding check: the opened handle and a fresh
 * pathname lstat must both be the exact non-link regular file whose
 * `dev`/`ino` were captured before opening. Identity fields are compared as
 * bigints (never as case-folded path strings); when either side reports an
 * unusable identity (zero `dev`/`ino` — possible on filesystems without
 * stable file identifiers), the check fails closed instead of claiming a
 * binding that cannot be proven.
 */
async function verifyOpenedIdentity(
  handle: FileHandle,
  path: string,
  expectedDev: bigint,
  expectedIno: bigint,
  context: string,
): Promise<{ readonly status: "invalid"; readonly message: string } | null> {
  let handleStats;
  try {
    handleStats = await handle.stat({ bigint: true });
  } catch (error: unknown) {
    return {
      status: "invalid",
      message: `Checkpoint ${context} opened preimage handle cannot be inspected: ${describeError(error)}`,
    };
  }
  if (handleStats.isSymbolicLink() || !handleStats.isFile()) {
    return {
      status: "invalid",
      message: `Checkpoint ${context} opened preimage object is not a regular file.`,
    };
  }
  if (!sameFileIdentity(handleStats, expectedDev, expectedIno)) {
    return {
      status: "invalid",
      message: `Checkpoint ${context} opened preimage object differs from the inspected object (identity substitution).`,
    };
  }
  let pathStats;
  try {
    pathStats = await lstat(path, { bigint: true });
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return {
        status: "invalid",
        message: `Checkpoint ${context} preimage path disappeared after it was opened.`,
      };
    }
    return {
      status: "invalid",
      message: `Checkpoint ${context} preimage path cannot be re-inspected after opening: ${describeError(error)}`,
    };
  }
  if (pathStats.isSymbolicLink() || !pathStats.isFile()) {
    return {
      status: "invalid",
      message: `Checkpoint ${context} preimage path no longer resolves to a non-link regular file.`,
    };
  }
  if (!sameFileIdentity(pathStats, expectedDev, expectedIno)) {
    return {
      status: "invalid",
      message: `Checkpoint ${context} preimage path now resolves to a different object.`,
    };
  }
  return null;
}

function sameFileIdentity(
  stats: { readonly dev: bigint; readonly ino: bigint },
  expectedDev: bigint,
  expectedIno: bigint,
): boolean {
  if (stats.dev === 0n || stats.ino === 0n || expectedDev === 0n || expectedIno === 0n) {
    return false;
  }
  return stats.dev === expectedDev && stats.ino === expectedIno;
}

/**
 * Final stability verification: the opened handle must still be the exact
 * non-link regular file captured in the pre-read snapshot — identical
 * identity, size, and mutation timestamps — and the pathname must still
 * resolve to that exact object (identity, size, and mutation timestamps
 * matching the final handle state). A same-inode in-place rewrite during
 * verification changes size and/or `mtimeNs`/`ctimeNs` and is refused here;
 * restoring `mtime` cannot hide a changed `ctime`, which user space cannot
 * set. This is a detection guarantee for mutation between the pre-read and
 * final snapshots, never atomic protection against modification after final
 * verification.
 */
async function verifyOpenedStability(
  handle: FileHandle,
  path: string,
  expected: FileVerificationSnapshot,
  context: string,
): Promise<{ readonly status: "invalid"; readonly message: string } | null> {
  let handleStats;
  try {
    handleStats = await handle.stat({ bigint: true });
  } catch (error: unknown) {
    return {
      status: "invalid",
      message: `Checkpoint ${context} opened preimage handle cannot be re-inspected: ${describeError(error)}`,
    };
  }
  if (handleStats.isSymbolicLink() || !handleStats.isFile()) {
    return {
      status: "invalid",
      message: `Checkpoint ${context} opened preimage object is not a regular file.`,
    };
  }
  if (
    handleStats.dev !== expected.dev ||
    handleStats.ino !== expected.ino ||
    handleStats.size !== expected.size ||
    handleStats.mtimeNs !== expected.mtimeNs ||
    handleStats.ctimeNs !== expected.ctimeNs
  ) {
    return {
      status: "invalid",
      message: `Checkpoint ${context} preimage changed during verification (in-place mutation).`,
    };
  }
  let pathStats;
  try {
    pathStats = await lstat(path, { bigint: true });
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return {
        status: "invalid",
        message: `Checkpoint ${context} preimage path disappeared after it was opened.`,
      };
    }
    return {
      status: "invalid",
      message: `Checkpoint ${context} preimage path cannot be re-inspected after opening: ${describeError(error)}`,
    };
  }
  if (pathStats.isSymbolicLink() || !pathStats.isFile()) {
    return {
      status: "invalid",
      message: `Checkpoint ${context} preimage path no longer resolves to a non-link regular file.`,
    };
  }
  if (
    pathStats.dev !== handleStats.dev ||
    pathStats.ino !== handleStats.ino ||
    pathStats.size !== handleStats.size ||
    pathStats.mtimeNs !== handleStats.mtimeNs ||
    pathStats.ctimeNs !== handleStats.ctimeNs
  ) {
    return {
      status: "invalid",
      message: `Checkpoint ${context} preimage path now resolves to a different object.`,
    };
  }
  return null;
}

/**
 * A stability snapshot is usable only when every field is a bigint and the
 * identity fields are non-zero. Node's bigint stats always expose these
 * fields on supported filesystems; where they are missing, invalid, or
 * zero-identity (a filesystem without stable identifiers), verification
 * fails closed instead of silently degrading to a weaker check.
 */
function isUsableVerificationSnapshot(snapshot: FileVerificationSnapshot): boolean {
  return (
    typeof snapshot.dev === "bigint" &&
    typeof snapshot.ino === "bigint" &&
    typeof snapshot.size === "bigint" &&
    typeof snapshot.mtimeNs === "bigint" &&
    typeof snapshot.ctimeNs === "bigint" &&
    snapshot.dev !== 0n &&
    snapshot.ino !== 0n
  );
}

function verificationInterruption(
  signal: AbortSignal | undefined,
  deadline: number | undefined,
): VerifiedPreimageOutcome | null {
  if (signal?.aborted) {
    return { status: "aborted" };
  }
  if (deadline !== undefined && Date.now() >= deadline) {
    return { status: "expired" };
  }
  return null;
}

export async function createFilesystemCheckpointStore(
  options: FilesystemCheckpointStoreOptions,
): Promise<CheckpointStore> {
  const maxCheckpoints = options.maxCheckpoints ?? DEFAULT_MAX_CHECKPOINTS;
  const maxStorageBytes = options.maxStorageBytes ?? DEFAULT_MAX_CHECKPOINT_STORAGE_BYTES;
  const maxPreimageBytes = options.maxPreimageBytes ?? DEFAULT_MAX_PREIMAGE_BYTES;
  const retentionDeadlineMs = options.retentionDeadlineMs ?? CHECKPOINT_PASS_DEADLINE_MS;
  requireSafeNonNegativeIntegerOption(maxCheckpoints, "maxCheckpoints");
  requireSafeNonNegativeIntegerOption(maxStorageBytes, "maxStorageBytes");
  requireSafeNonNegativeIntegerOption(maxPreimageBytes, "maxPreimageBytes");
  if (maxPreimageBytes > MAX_SUPPORTED_PREIMAGE_BYTES) {
    throw new Error(
      `Checkpoint store option maxPreimageBytes (${maxPreimageBytes}) exceeds the supported maximum of ${MAX_SUPPORTED_PREIMAGE_BYTES} bytes.`,
    );
  }
  if (!Number.isFinite(retentionDeadlineMs)) {
    throw new Error("Checkpoint store option retentionDeadlineMs must be a finite number.");
  }
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
   * No-follow validation of a path under the Siralos-owned checkpoint root.
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
    return (await loadMetadataWithByteCount(id)).checkpoint;
  }

  /**
   * Reads and fully validates one checkpoint's metadata, returning the
   * validated record together with the exact UTF-8 byte count of the raw
   * metadata that was read. The byte count is used as the ACTUAL disk
   * accounting for the metadata file in the storage-byte limit; it is never
   * taken from the metadata's own declared lengths.
   */
  async function loadMetadataWithByteCount(
    id: string,
  ): Promise<{ readonly checkpoint: FileCheckpoint; readonly metadataBytes: number }> {
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
    return {
      checkpoint: validateMetadata(parsed, id, workspaceFingerprint, maxPreimageBytes),
      metadataBytes: Buffer.byteLength(raw, "utf8"),
    };
  }

  async function writeMetadata(checkpoint: FileCheckpoint): Promise<void> {
    await writeMetadataSerialized(checkpoint, serializeCheckpoint(checkpoint));
  }

  async function writeMetadataSerialized(
    checkpoint: FileCheckpoint,
    serialized: string,
  ): Promise<void> {
    const directory = checkpointPath(checkpoint.id);
    await assertCheckpointPathSecure(directory);
    const temporaryPath = join(directory, `metadata.json.tmp-${randomUUID()}`);
    const metadataPath = join(directory, "metadata.json");
    const handle = await open(temporaryPath, "wx", 0o600);
    try {
      await handle.writeFile(serialized, "utf8");
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
   * than the retention cap plus one, a checkpoint directory holds more
   * entries than the known layout, or the pass deadline expired) also
   * fails closed: capacity cannot be proven, so no new checkpoint is
   * created. Refusal always happens before any write.
   *
   * The byte limit measures the total ACTUAL regular-file bytes stored
   * beneath the workspace checkpoint directory, including metadata and
   * preimages: every `metadata.json` byte (the exact UTF-8 length read from
   * disk) and every `preimage.bin` byte (the actual observed size,
   * cross-checked against the metadata-declared length). Filesystem
   * allocation overhead is outside this logical byte limit. The proposed
   * checkpoint's exact serialized metadata bytes and preimage bytes are
   * added before any write. The serialized metadata used for accounting is
   * exactly the content subsequently written.
   *
   * Every existing checkpoint must be fully inspectable and match the
   * exact `cp_<valid-id>/{metadata.json,preimage.bin}` layout: any entry
   * whose metadata is invalid, unreadable, oversized, or linked — or whose
   * preimage presence, actual size, or actual SHA-256 disagrees with its
   * validated metadata — as well as unknown files, nested directories,
   * temporary files, case-variant duplicate names, links, special files,
   * or entries that cannot be inspected, makes capacity unverifiable and
   * refuses the new checkpoint. A declared preimage is read through the
   * shared bounded verifier, which materializes at most
   * `maxPreimageBytes + 1` bytes and requires the exact bytes read to
   * match the metadata byte length and SHA-256: a same-size corrupted
   * preimage is a refusal, and any change that is detectable between the
   * lstat and the read (growth, shrinkage, substitution, disappearance,
   * unreadability) is a refusal. The store never deletes, renames, repairs,
   * truncates, or quarantines unexpected entries: the entire tree is
   * preserved for manual inspection. Inspection is incremental (per-directory entry
   * caps), bounded (a global inspected-entry cap, pass deadline,
   * cancellation), and link-free (dirent and lstat types are trusted;
   * nothing is followed). Byte arithmetic is overflow-safe.
   *
   * Race note: the check-to-write sequence cannot be atomic against a
   * same-user process modifying the checkpoint tree between accounting and
   * the new checkpoint's writes (Node offers no directory-relative
   * primitive). This is a fail-closed measurement of what could be
   * verified at pass time, not an atomic global quota enforcement.
   */
  async function assertRetentionCapacity(
    proposed: { readonly metadataBytes: number; readonly preimageBytes: number },
    signal?: AbortSignal,
  ): Promise<void> {
    if (signal?.aborted) {
      throw new DOMException("Checkpoint preparation was aborted.", "AbortError");
    }
    const deadline = Date.now() + retentionDeadlineMs;
    const maxTotalInspected = 4 * (maxCheckpoints + 1);
    let totalInspected = 0;
    const checkpointNames: string[] = [];
    const refuse = (reason: string): never => {
      throw new CheckpointStorageLimitError(
        `Checkpoint storage capacity is unverifiable (${reason}); refusing to create a checkpoint. No checkpoint or filesystem entry was written or deleted; existing checkpoints are preserved for manual inspection.`,
      );
    };
    // The enumeration is incremental and capped at the retention maximum
    // plus one, so a hostile directory can never be materialized. Only the
    // exact checkpoint layout is accepted: any other entry — files,
    // directories with unknown names, links, special files — makes capacity
    // unverifiable.
    const outcome = await enumerateDirectoryBounded({
      directory: checkpointDirectory,
      maxEntries: maxCheckpoints + 1,
      signal,
      deadline,
      onEntry: (entry) => {
        totalInspected += 1;
        if (totalInspected > maxTotalInspected) {
          refuse("more entries were inspected than the global inspection bound allows");
        }
        if (!entry.isDirectory() || !CHECKPOINT_ID_PATTERN.test(entry.name)) {
          refuse(
            `unexpected entry "${entry.name}" in the checkpoint directory (only checkpoint directories named "cp_<valid-id>" are permitted)`,
          );
        }
        checkpointNames.push(entry.name);
      },
    });
    if (outcome.truncated) {
      refuse("the checkpoint directory enumeration was truncated");
    }
    let totalBytes = 0;
    for (const name of checkpointNames) {
      if (signal?.aborted) {
        throw new DOMException("Checkpoint preparation was aborted.", "AbortError");
      }
      if (Date.now() >= deadline) {
        refuse("the pass deadline expired before capacity could be verified");
      }
      try {
        const directory = checkpointPath(name);
        // The checkpoint directory itself is enumerated entry by entry with
        // a per-directory cap: the known layout is metadata.json and
        // preimage.bin only, so a truncated enumeration proves unexpected
        // content without ever materializing it.
        const dirOutcome = await enumerateDirectoryBounded({
          directory,
          maxEntries: CHECKPOINT_DIR_MAX_ENTRIES,
          signal,
          deadline,
          onEntry: (entry) => {
            totalInspected += 1;
            if (totalInspected > maxTotalInspected) {
              refuse("more entries were inspected than the global inspection bound allows");
            }
            if (entry.name !== "metadata.json" && entry.name !== "preimage.bin") {
              refuse(`unexpected entry "${entry.name}" inside checkpoint ${name}`);
            }
            if (entry.isSymbolicLink()) {
              refuse(`entry "${entry.name}" inside checkpoint ${name} is a symbolic link`);
            }
            if (!entry.isFile()) {
              refuse(`entry "${entry.name}" inside checkpoint ${name} is not a regular file`);
            }
          },
        });
        if (dirOutcome.truncated) {
          refuse(`checkpoint ${name} holds more entries than the known checkpoint layout`);
        }
        const { checkpoint, metadataBytes } = await loadMetadataWithByteCount(name);
        const expectsPreimage = checkpoint.before.exists;
        const preimagePath = join(checkpointPath(checkpoint.id), "preimage.bin");
        let preimageSize = 0;
        if (!expectsPreimage) {
          // The metadata declares no preimage: only absence is acceptable.
          // ENOENT means "no preimage"; ANY other inspection outcome makes
          // the entry's storage contribution unknowable and must fail
          // closed rather than be assumed to consume zero bytes.
          const preimageStats = await lstat(preimagePath).catch((error: unknown) => {
            if (isNotFoundError(error)) {
              return null;
            }
            throw error;
          });
          if (preimageStats !== null) {
            throw new Error(
              `Checkpoint ${name} metadata declares no preimage but a preimage.bin exists.`,
            );
          }
        } else {
          await assertCheckpointPathSecure(preimagePath);
          // The actual bytes on disk are read and hash-verified against the
          // metadata: a same-size corrupted preimage is detected, and the
          // exact bytes read are the disk accounting (never a size guess).
          const outcome = await verifyPreimageBounded({
            path: preimagePath,
            expected: checkpoint.before,
            maxBytes: maxPreimageBytes,
            context: name,
            signal,
            deadline,
            onInspect: options.retentionPreimageInspectionHook,
            read: options.preimageReadSeam,
          });
          if (outcome.status === "aborted") {
            throw new DOMException("Checkpoint preparation was aborted.", "AbortError");
          }
          if (outcome.status === "expired") {
            refuse("the pass deadline expired before capacity could be verified");
          }
          if (outcome.status === "missing") {
            throw new Error(
              `Checkpoint ${name} metadata declares a preimage but its preimage.bin is missing.`,
            );
          }
          if (outcome.status === "invalid") {
            throw new Error(outcome.message);
          }
          if (outcome.status !== "verified") {
            throw new Error(
              `Checkpoint ${name} preimage verification returned an unexpected outcome.`,
            );
          }
          preimageSize = outcome.bytes.length;
        }
        totalBytes = checkedByteTotal(totalBytes, checkedByteTotal(metadataBytes, preimageSize));
      } catch (error: unknown) {
        if (error instanceof CheckpointStorageLimitError) {
          throw error;
        }
        if (error instanceof DOMException && error.name === "AbortError") {
          throw error;
        }
        refuse(describeError(error));
      }
    }
    let proposedTotal: number;
    try {
      proposedTotal = checkedByteTotal(proposed.metadataBytes, proposed.preimageBytes);
    } catch (error: unknown) {
      throw new CheckpointStorageLimitError(
        `Checkpoint storage capacity is unverifiable (the proposed checkpoint contribution cannot be represented as a safe byte total (${describeError(error)})); refusing to create a checkpoint. No checkpoint or filesystem entry was written or deleted; existing checkpoints are preserved for manual inspection.`,
      );
    }
    let combined: number;
    try {
      combined = checkedByteTotal(totalBytes, proposedTotal);
    } catch (error: unknown) {
      throw new CheckpointStorageLimitError(
        `Checkpoint storage capacity is unverifiable (byte totals exceed the safe integer range (${describeError(error)})); refusing to create a checkpoint. No checkpoint or filesystem entry was written or deleted; existing checkpoints are preserved for manual inspection.`,
      );
    }
    if (checkpointNames.length + 1 > maxCheckpoints || combined > maxStorageBytes) {
      throw new CheckpointStorageLimitError(
        `Checkpoint storage limits would be exceeded (${checkpointNames.length + 1} checkpoints against a ${maxCheckpoints}-checkpoint limit, ${combined} bytes against a ${maxStorageBytes}-byte limit); refusing to create a checkpoint. No checkpoint or filesystem entry was deleted; existing checkpoints are preserved for manual inspection.`,
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
    // The complete prepared record is validated at runtime BEFORE any
    // capacity inspection or filesystem activity: an out-of-contract caller
    // must not be able to prepare a checkpoint the store's own loader would
    // reject (for example `exists: true` with `bytes: null`), nor one whose
    // presence, hash, length, and bytes disagree with each other.
    const validated = validatePreparedCheckpoint(checkpoint, maxPreimageBytes);
    const id = `cp_${randomUUID()}`;
    const stored: FileCheckpoint = {
      version: 1,
      id,
      workspaceFingerprint,
      relativePath: validated.relativePath,
      operation: validated.operation,
      toolName: validated.toolName,
      createdAt: new Date().toISOString(),
      state: "prepared",
      before: {
        exists: validated.before.exists,
        sha256: validated.before.sha256,
        byteLength: validated.before.byteLength,
      },
      after: validated.after,
      preview: validated.preview,
    };
    // The exact record that would be written must satisfy the same
    // structural rules used when loading metadata: the store never writes a
    // record its own loader would reject.
    validateMetadata(stored, id, workspaceFingerprint, maxPreimageBytes);
    const serializedMetadata = serializeCheckpoint(stored);
    const metadataBytes = Buffer.byteLength(serializedMetadata, "utf8");
    if (metadataBytes > MAX_METADATA_BYTES) {
      throw new Error("Checkpoint metadata would exceed the serialized metadata size limit.");
    }
    await assertRetentionCapacity(
      {
        metadataBytes,
        preimageBytes: validated.before.exists ? validated.before.byteLength : 0,
      },
      signal,
    );
    if (signal?.aborted) {
      throw new DOMException("Checkpoint preparation was aborted.", "AbortError");
    }
    const directory = checkpointPath(id);
    await mkdir(directory, { recursive: true, mode: 0o700 });
    await assertCheckpointPathSecure(directory);
    if (validated.before.exists) {
      const preimagePath = join(directory, "preimage.bin");
      const handle = await open(preimagePath, "wx", 0o600);
      try {
        await handle.writeFile(validated.before.bytes);
        await handle.sync();
      } finally {
        await handle.close();
      }
      await assertCheckpointPathSecure(preimagePath);
    }
    await writeMetadataSerialized(stored, serializedMetadata);
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
    const outcome = await verifyPreimageBounded({
      path: preimagePath,
      expected: checkpoint.before,
      maxBytes: maxPreimageBytes,
      context: checkpointId,
      read: options.preimageReadSeam,
    });
    if (outcome.status === "missing") {
      throw new Error(`Checkpoint preimage is missing: ${checkpointId}.`);
    }
    if (outcome.status === "invalid") {
      throw new Error(outcome.message);
    }
    if (outcome.status === "aborted" || outcome.status === "expired") {
      throw new Error(
        `Checkpoint preimage verification was interrupted: ${checkpointId}. (Unreachable without a signal or deadline.)`,
      );
    }
    return new Uint8Array(outcome.bytes);
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
 * not exist carries neither a hash nor a byte length). The record's key set
 * is checked exactly: extra or substituted fields are rejected. This is the
 * single stored-record validator: `prepare()` validates its constructed
 * record with this same function, so the store can never write a record
 * that `loadMetadata()` would reject.
 */
function validateMetadata(
  parsed: unknown,
  id: string,
  workspaceFingerprint: string,
  maxPreimageBytes: number,
): FileCheckpoint {
  const record = asRecordStrict(parsed, "metadata", id, STORED_RECORD_KEYS);
  if (record["version"] !== 1 || record["id"] !== id) {
    throw new Error(checkpointMessage(id, "Checkpoint metadata version or id mismatch"));
  }
  if (record["workspaceFingerprint"] !== workspaceFingerprint) {
    throw new Error(checkpointMessage(id, "Checkpoint belongs to a different workspace"));
  }
  const relativePath = record["relativePath"];
  if (typeof relativePath !== "string" || validateRelativeWorkspacePath(relativePath) !== null) {
    throw new Error(checkpointMessage(id, "Checkpoint relative path is invalid"));
  }
  const state = record["state"];
  if (!isCheckpointState(state)) {
    throw new Error(checkpointMessage(id, "Checkpoint state is invalid"));
  }
  const toolName = record["toolName"];
  if (
    typeof toolName !== "string" ||
    toolName.length === 0 ||
    toolName.length > MAX_TOOL_NAME_LENGTH
  ) {
    throw new Error(checkpointMessage(id, "Checkpoint tool name is invalid"));
  }
  const createdAt = record["createdAt"];
  if (
    typeof createdAt !== "string" ||
    createdAt.length === 0 ||
    createdAt.length > MAX_CREATED_AT_LENGTH ||
    Number.isNaN(Date.parse(createdAt))
  ) {
    throw new Error(checkpointMessage(id, "Checkpoint creation timestamp is invalid"));
  }
  const before = validateFileState(record["before"], id, "before", maxPreimageBytes);
  const after = validateFileState(record["after"], id, "after", maxPreimageBytes);
  const operation = validateOperationFileStates(
    record["operation"],
    before.exists,
    after.exists,
    id,
  );
  const preview = asRecordStrict(record["preview"], "preview", id, PREVIEW_KEYS);
  const addedLines = preview["addedLines"];
  const removedLines = preview["removedLines"];
  if (!isSafeNonNegativeInteger(addedLines) || !isSafeNonNegativeInteger(removedLines)) {
    throw new Error(checkpointMessage(id, "Checkpoint preview counts are invalid"));
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

interface ValidatedPreparedCheckpoint {
  readonly relativePath: string;
  readonly operation: CheckpointOperation;
  readonly toolName: string;
  readonly before:
    | {
        readonly exists: true;
        readonly sha256: string;
        readonly byteLength: number;
        readonly bytes: Uint8Array;
      }
    | {
        readonly exists: false;
        readonly sha256: null;
        readonly byteLength: null;
        readonly bytes: null;
      };
  readonly after: { exists: boolean; sha256: string | null; byteLength: number | null };
  readonly preview: { readonly addedLines: number; readonly removedLines: number };
}

/**
 * Runtime validation of a complete prepared checkpoint before any capacity
 * inspection or filesystem activity. Shares its structural primitives with
 * the stored-record validator (file states, preview counts, tool name,
 * operation, relative path), and additionally binds the preimage presence,
 * bytes, hash, and length together: `exists: true` requires real bytes
 * whose length and SHA-256 match the declared values within the configured
 * bound; `exists: false` requires every field to be `null`. The record's
 * key set is checked exactly; extra or substituted fields are rejected.
 */
function validatePreparedCheckpoint(
  input: unknown,
  maxPreimageBytes: number,
): ValidatedPreparedCheckpoint {
  const record = asRecordStrict(input, "prepared checkpoint", "", PREPARED_RECORD_KEYS);
  const relativePath = record["relativePath"];
  if (typeof relativePath !== "string" || validateRelativeWorkspacePath(relativePath) !== null) {
    throw new Error("Checkpoint relative path is invalid.");
  }
  const toolName = record["toolName"];
  if (
    typeof toolName !== "string" ||
    toolName.length === 0 ||
    toolName.length > MAX_TOOL_NAME_LENGTH
  ) {
    throw new Error("Checkpoint tool name is invalid.");
  }
  const before = validatePreparedFileState(record["before"], maxPreimageBytes);
  const after = validateFileState(record["after"], "", "after", maxPreimageBytes);
  const operation = validateOperationFileStates(
    record["operation"],
    before.exists,
    after.exists,
    "",
  );
  const preview = asRecordStrict(record["preview"], "preview", "", PREVIEW_KEYS);
  const addedLines = preview["addedLines"];
  const removedLines = preview["removedLines"];
  if (!isSafeNonNegativeInteger(addedLines) || !isSafeNonNegativeInteger(removedLines)) {
    throw new Error("Checkpoint preview counts are invalid.");
  }
  return {
    relativePath,
    operation,
    toolName,
    before,
    after,
    preview: { addedLines, removedLines },
  };
}

function validatePreparedFileState(
  value: unknown,
  maxPreimageBytes: number,
): ValidatedPreparedCheckpoint["before"] {
  const record = asRecordStrict(value, "before file state", "", PREPARED_BEFORE_KEYS);
  const exists = record["exists"];
  if (typeof exists !== "boolean") {
    throw new Error("Checkpoint before existence flag is invalid.");
  }
  const sha256 = record["sha256"];
  const byteLength = record["byteLength"];
  const bytes = record["bytes"];
  if (exists) {
    if (typeof sha256 !== "string" || !SHA256_PATTERN.test(sha256)) {
      throw new Error("Checkpoint before sha256 is invalid.");
    }
    if (!isSafeNonNegativeInteger(byteLength)) {
      throw new Error("Checkpoint before byte length is invalid.");
    }
    if (byteLength > maxPreimageBytes) {
      throw new Error("Checkpoint before byte length exceeds the configured maximum.");
    }
    if (!(bytes instanceof Uint8Array)) {
      throw new Error("Checkpoint preimage bytes are missing or are not byte data.");
    }
    if (bytes.byteLength !== byteLength) {
      throw new Error("Checkpoint preimage size does not match the recorded byte length.");
    }
    const hash = createHash("sha256").update(bytes).digest("hex");
    if (hash !== sha256) {
      throw new Error("Checkpoint preimage hash does not match the recorded before hash.");
    }
    return { exists: true, sha256, byteLength, bytes };
  }
  if (bytes !== null) {
    throw new Error("Checkpoint before exists is false but preimage bytes are present.");
  }
  if (sha256 !== null || byteLength !== null) {
    throw new Error(
      "Checkpoint before carries a hash or byte length for a state that does not exist.",
    );
  }
  return { exists: false, sha256: null, byteLength: null, bytes: null };
}

function validateFileState(
  value: unknown,
  id: string,
  label: "before" | "after",
  maxPreimageBytes: number,
): { exists: boolean; sha256: string | null; byteLength: number | null } {
  const record = asRecordStrict(value, `${label} file state`, id, FILE_STATE_KEYS);
  const exists = record["exists"];
  if (typeof exists !== "boolean") {
    throw new Error(checkpointMessage(id, `Checkpoint ${label} existence flag is invalid`));
  }
  const sha256 = record["sha256"];
  const byteLength = record["byteLength"];
  if (exists) {
    if (typeof sha256 !== "string" || !SHA256_PATTERN.test(sha256)) {
      throw new Error(checkpointMessage(id, `Checkpoint ${label} sha256 is invalid`));
    }
    if (!isSafeNonNegativeInteger(byteLength)) {
      throw new Error(checkpointMessage(id, `Checkpoint ${label} byte length is invalid`));
    }
    if (label === "before" && byteLength > maxPreimageBytes) {
      throw new Error(
        checkpointMessage(id, `Checkpoint ${label} byte length exceeds the configured maximum`),
      );
    }
    return { exists, sha256, byteLength };
  }
  if (sha256 !== null || byteLength !== null) {
    throw new Error(
      checkpointMessage(
        id,
        `Checkpoint ${label} carries a hash or byte length for a state that does not exist`,
      ),
    );
  }
  return { exists: false, sha256: null, byteLength: null };
}

/**
 * Shared operation-state validator: `operation` must be one of the three
 * checkpoint operations AND its before/after existence flags must match the
 * canonical invariant matrix (create: absent->present; update:
 * present->present; delete: present->absent). Used by BOTH the prepared-
 * record validator and the stored-metadata validator so the two paths can
 * never diverge; the operation-state relationship is the invariant, never
 * an inference from tool names.
 */
function validateOperationFileStates(
  operation: unknown,
  beforeExists: boolean,
  afterExists: boolean,
  id: string,
): CheckpointOperation {
  if (!isCheckpointOperation(operation)) {
    throw new Error(checkpointMessage(id, "Checkpoint operation is invalid"));
  }
  const expected = OPERATION_STATE_MATRIX[operation];
  if (beforeExists !== expected.beforeExists || afterExists !== expected.afterExists) {
    throw new Error(
      checkpointMessage(
        id,
        `Checkpoint operation "${operation}" requires the before/after existence transition ${expected.beforeExists}->${expected.afterExists}, but the record declares ${beforeExists}->${afterExists}`,
      ),
    );
  }
  return operation;
}

function asRecordStrict(
  value: unknown,
  label: string,
  id: string,
  keys: readonly string[],
): Record<string, unknown> {
  const record = asRecord(value, label, id);
  const ownKeys = Object.keys(record);
  if (
    ownKeys.length !== keys.length ||
    keys.some((key) => !Object.prototype.hasOwnProperty.call(record, key))
  ) {
    throw new Error(checkpointMessage(id, `Checkpoint ${label} carries unexpected fields`));
  }
  return record;
}

function asRecord(value: unknown, label: string, id: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    throw new Error(checkpointMessage(id, `Checkpoint ${label} is malformed`));
  }
  return value as Record<string, unknown>;
}

function serializeCheckpoint(checkpoint: FileCheckpoint): string {
  return JSON.stringify(checkpoint, null, 2);
}

function checkpointMessage(id: string, message: string): string {
  return id.length === 0 ? `${message}.` : `${message}: ${id}.`;
}

function requireSafeNonNegativeIntegerOption(value: number, name: string): void {
  if (!isSafeNonNegativeInteger(value)) {
    throw new Error(`Checkpoint store option ${name} must be a safe non-negative integer.`);
  }
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
  if (!CHECKPOINT_ID_PATTERN.test(id)) {
    throw new Error(`Invalid checkpoint id: ${id}.`);
  }
}

function isInside(root: string, target: string): boolean {
  const rootPrefix = root.endsWith("/") || root.endsWith("\\") ? root : `${root}${sep}`;
  return target.startsWith(rootPrefix);
}

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}
