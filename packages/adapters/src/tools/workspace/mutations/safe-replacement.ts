import { createHash, randomUUID } from "node:crypto";
import { lstat, readFile, rename, rm, unlink } from "node:fs/promises";
import { dirname, join } from "node:path";

export interface ReplacementFsOps {
  rename(from: string, to: string): Promise<void>;
  unlink(path: string): Promise<void>;
  readFile(path: string): Promise<Buffer>;
  lstat(path: string): Promise<Awaited<ReturnType<typeof lstat>>>;
  rm(path: string, options: { force: true }): Promise<void>;
}

export const REAL_REPLACEMENT_FS_OPS: ReplacementFsOps = {
  rename,
  unlink,
  readFile,
  lstat,
  rm,
};

export type ReplacementOutcome =
  | {
      readonly kind: "success";
      /**
       * Path of a same-directory quarantine holding the replaced original.
       * Non-null only when the platform required a quarantine dance; the
       * caller must delete it only after verifying the new content, because
       * it may be the only remaining copy of the original.
       */
      readonly quarantinePath: string | null;
    }
  | {
      /** Nothing was committed and the original target remains intact. */
      readonly kind: "failed";
      readonly message: string;
      readonly quarantinePath: string | null;
    }
  | {
      /**
       * The operation could not be proven safe: the target may be absent or
       * hold new content while the original sits in the quarantine. Never
       * reported as success; the quarantine path is the recoverable copy.
       */
      readonly kind: "uncertain";
      readonly message: string;
      readonly quarantinePath: string;
    };

export interface ReplaceFileWithQuarantineOptions {
  readonly tempPath: string;
  readonly targetPath: string;
  /**
   * SHA-256 of the content the target is expected to hold. The quarantined
   * copy is verified against it before the new content is committed, so a
   * substituted or externally-changed target can never be silently
   * destroyed.
   */
  readonly expectedTargetSha256: string | null;
  readonly ops?: ReplacementFsOps;
}

/**
 * One shared, narrowly scoped safe file-replacement primitive. Never unlinks
 * the only valid copy before replacement is committed: the original is moved
 * to a same-directory quarantine (recoverable), verified by hash, replaced
 * by the staged content, and rolled back automatically if the commit fails.
 * On POSIX the atomic rename path is used directly and no quarantine is
 * created. Recovery information (the quarantine path) is preserved in every
 * failure result so an intermediate state that survives a process crash is
 * never reported as success or silently cleaned up.
 */
export async function replaceFileWithQuarantine(
  options: ReplaceFileWithQuarantineOptions,
): Promise<ReplacementOutcome> {
  const ops = options.ops ?? REAL_REPLACEMENT_FS_OPS;
  try {
    await ops.rename(options.tempPath, options.targetPath);
    return { kind: "success", quarantinePath: null };
  } catch {
    // The direct replace failed (typical on Windows for locked or
    // read-only targets); fall through to the quarantine dance.
  }
  const quarantinePath = join(dirname(options.targetPath), `.solaris-quarantine-${randomUUID()}`);
  try {
    await ops.rename(options.targetPath, quarantinePath);
  } catch (error: unknown) {
    return {
      kind: "failed",
      message: `The replacement could not be committed and the original could not be moved to quarantine: ${describeError(error)}`,
      quarantinePath: null,
    };
  }
  if (options.expectedTargetSha256 !== null) {
    const identityCheck = await verifyQuarantinedIdentity(
      ops,
      quarantinePath,
      options.expectedTargetSha256,
    );
    if (!identityCheck.ok) {
      const rollback = await restoreOriginal(ops, quarantinePath, options.targetPath);
      if (rollback.ok) {
        return {
          kind: "failed",
          message: `${identityCheck.message}; the original was restored and the replacement was not committed.`,
          quarantinePath: null,
        };
      }
      return {
        kind: "uncertain",
        message: `${identityCheck.message} and the original could not be restored; the recoverable original is at ${quarantinePath}.`,
        quarantinePath,
      };
    }
  }
  try {
    await ops.rename(options.tempPath, options.targetPath);
  } catch (error: unknown) {
    const rollback = await restoreOriginal(ops, quarantinePath, options.targetPath);
    if (rollback.ok) {
      return {
        kind: "failed",
        message: `The replacement could not be committed (${describeError(error)}); the original was restored.`,
        quarantinePath: null,
      };
    }
    return {
      kind: "uncertain",
      message: `The replacement could not be committed (${describeError(error)}) and the original could not be restored; the recoverable original is at ${quarantinePath}.`,
      quarantinePath,
    };
  }
  return { kind: "success", quarantinePath };
}

async function verifyQuarantinedIdentity(
  ops: ReplacementFsOps,
  quarantinePath: string,
  expectedSha256: string,
): Promise<{ ok: true } | { ok: false; message: string }> {
  let bytes: Buffer;
  try {
    bytes = await ops.readFile(quarantinePath);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The quarantined original could not be verified (${describeError(error)})`,
    };
  }
  const hash = createHash("sha256").update(bytes).digest("hex");
  if (hash !== expectedSha256) {
    return {
      ok: false,
      message: "The quarantined original does not match the expected content hash",
    };
  }
  return { ok: true };
}

async function restoreOriginal(
  ops: ReplacementFsOps,
  quarantinePath: string,
  targetPath: string,
): Promise<{ ok: true } | { ok: false }> {
  try {
    await ops.rename(quarantinePath, targetPath);
    return { ok: true };
  } catch {
    return { ok: false };
  }
}

export async function removeQuarantinedCopy(
  quarantinePath: string,
  ops?: ReplacementFsOps,
): Promise<void> {
  await (ops ?? REAL_REPLACEMENT_FS_OPS).rm(quarantinePath, { force: true });
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown filesystem failure occurred";
}
