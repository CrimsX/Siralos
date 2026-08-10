import { createHash, randomUUID } from "node:crypto";
import { link, lstat, readFile, rename, rm, unlink } from "node:fs/promises";
import { dirname, join } from "node:path";
import { createErrorDescriber } from "../../../support/error-message.js";

const describeError = createErrorDescriber("an unknown filesystem failure occurred");

export interface ReplacementFsOps {
  rename(from: string, to: string): Promise<void>;
  /**
   * Creates `to` as a hard link to `from`. Must fail (EEXIST) when `to`
   * already exists and never overwrite, unlike rename. This is the commit
   * and restore primitive: it atomically requires the destination to stay
   * absent, so a target that reappears between displacement and commit can
   * never be silently overwritten.
   */
  link(from: string, to: string): Promise<void>;
  unlink(path: string): Promise<void>;
  readFile(path: string): Promise<Buffer>;
  lstat(path: string): Promise<Awaited<ReturnType<typeof lstat>>>;
  rm(path: string, options: { force: true }): Promise<void>;
}

export const REAL_REPLACEMENT_FS_OPS: ReplacementFsOps = {
  rename,
  link,
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
       * Always non-null: every commit displaces the target into quarantine
       * first and verifies it against the expected state. The caller must
       * delete it only after verifying the new content and durably
       * finalizing the checkpoint lifecycle, because it may be the only
       * remaining copy of the original.
       */
      readonly quarantinePath: string;
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
   * SHA-256 of the content the target is expected to hold. The target is
   * always moved to a same-directory quarantine before the replacement is
   * committed, and the displaced object is verified against this hash, so a
   * substituted or externally-changed target can never be silently
   * destroyed on any platform.
   */
  readonly expectedTargetSha256: string | null;
  /**
   * SHA-256 of the content staged at `tempPath`. When provided, the
   * committed target is verified after the commit: it must be a regular
   * non-symlink file whose bytes hash to exactly this value. A staged file
   * tampered between staging and commit is therefore detected before the
   * operation can be reported as success.
   */
  readonly expectedStagedSha256: string | null;
  /**
   * Optional final parent-chain identity verification, invoked immediately
   * before the displacement rename. It must throw when any parent
   * component of `targetPath` is a symbolic link or no longer resolves
   * canonically inside its verified root; the operation then fails closed
   * before modifying anything. Node exposes no dirfd-relative rename, so
   * this shrinks the parent-swap window to the single rename syscall, and
   * the displaced object's identity (dev+ino captured immediately before
   * the rename) is re-verified afterwards.
   */
  readonly verifyParentIdentity?: () => Promise<void>;
  readonly ops?: ReplacementFsOps;
}

/**
 * One shared, narrowly scoped safe file-replacement primitive. Never unlinks
 * the only valid copy before replacement is committed, and never trusts a
 * path-based check at the commit instant: the target is first moved to a
 * same-directory quarantine (an atomic displacement of exactly the object
 * that currently sits at the target), the displaced object is verified
 * against the expected hash, the staged content is committed, and the
 * original is rolled back automatically if the commit fails.
 *
 * The commit and every restore is an exclusive hard link: `link` fails with
 * EEXIST when the destination exists and never overwrites, so a new target
 * created by another process between displacement and commit is preserved
 * (outcome `uncertain`, original kept in quarantine) instead of being
 * silently destroyed by a rename. Filesystems without hard-link support fail
 * closed (uncertain or failed with the quarantine preserved) — an
 * overwrite-capable rename is never used as a fallback. The staged temp file
 * is kept until the outcome is known and is removed only after success, via
 * an unlink of the temp link, never of the target.
 *
 * The quarantine dance runs on every platform — a successful direct rename
 * is not treated as a compare-and-swap. Recovery information (the quarantine
 * path) is preserved in every failure result so an intermediate state that
 * survives a process crash is never reported as success or silently cleaned
 * up.
 */
export async function replaceFileWithQuarantine(
  options: ReplaceFileWithQuarantineOptions,
): Promise<ReplacementOutcome> {
  const ops = options.ops ?? REAL_REPLACEMENT_FS_OPS;
  if (options.verifyParentIdentity !== undefined) {
    try {
      await options.verifyParentIdentity();
    } catch (error: unknown) {
      return {
        kind: "failed",
        message: `The replacement was refused before any change: the target's parent chain no longer verifies (${describeError(error)}).`,
        quarantinePath: null,
      };
    }
  }
  const quarantinePath = join(dirname(options.targetPath), `.solaris-quarantine-${randomUUID()}`);
  // Identity capture immediately before the displacement: the object that
  // sits at the target at this instant is the only object the operation may
  // displace, and its dev+ino is re-verified on the quarantined path after
  // the rename, so a parent swapped in the final window (which would
  // displace an object outside the workspace) is detected and rolled back.
  const expectedDevIno = await captureTargetIdentity(ops, options.targetPath);
  if (expectedDevIno === null) {
    return {
      kind: "failed",
      message: "The replacement could not be committed: the target is not accessible.",
      quarantinePath: null,
    };
  }
  try {
    await ops.rename(options.targetPath, quarantinePath);
  } catch (error: unknown) {
    return {
      kind: "failed",
      message: `The replacement could not be committed and the original could not be moved to quarantine: ${describeError(error)}`,
      quarantinePath: null,
    };
  }
  const displacedIdentity = await verifyDisplacedIdentity(ops, quarantinePath, expectedDevIno);
  if (!displacedIdentity.ok) {
    const rollback = await restoreOriginal(ops, quarantinePath, options.targetPath);
    if (rollback.ok) {
      return {
        kind: "failed",
        message: `${displacedIdentity.message}; the displaced object was restored and the replacement was not committed.`,
        quarantinePath: null,
      };
    }
    return {
      kind: "uncertain",
      message: `${displacedIdentity.message} and the displaced object could not be restored (${rollback.message}); the recoverable original is at ${quarantinePath}.`,
      quarantinePath,
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
        message: `${identityCheck.message} and the original could not be restored (${rollback.message}); the recoverable original is at ${quarantinePath}.`,
        quarantinePath,
      };
    }
  }
  // The commit is an exclusive hard link: it atomically requires the target
  // to remain absent (EEXIST) and can never overwrite a target that
  // reappeared after the displacement.
  try {
    await ops.link(options.tempPath, options.targetPath);
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
      message: `The replacement could not be committed (${describeError(error)}) and the original could not be restored (${rollback.message}); the recoverable original is at ${quarantinePath}.`,
      quarantinePath,
    };
  }
  const committed = await verifyCommittedTarget(
    ops,
    options.targetPath,
    options.expectedStagedSha256,
  );
  if (!committed.ok) {
    const rollback = await rollbackCommittedTarget(
      ops,
      options.tempPath,
      options.targetPath,
      quarantinePath,
    );
    if (rollback.ok) {
      return {
        kind: "failed",
        message: `${committed.message}; the original was restored and the replacement was not kept.`,
        quarantinePath: null,
      };
    }
    return {
      kind: "uncertain",
      message: `${committed.message} and the original could not be restored (${rollback.message}); the recoverable original is at ${quarantinePath}.`,
      quarantinePath,
    };
  }
  return { kind: "success", quarantinePath };
}

export type UnlinkOutcome =
  | {
      readonly kind: "success";
      /** Path of the quarantine that held the deleted original, if any. */
      readonly quarantinePath: string | null;
    }
  | {
      /** Nothing was deleted and the original target remains intact. */
      readonly kind: "failed";
      readonly message: string;
      readonly quarantinePath: string | null;
    }
  | {
      /**
       * The deletion could not be proven safe: the target may be absent or
       * hold new content while the displaced original sits in the
       * quarantine. Never reported as success; the quarantine path is the
       * recoverable copy.
       */
      readonly kind: "uncertain";
      readonly message: string;
      readonly quarantinePath: string;
    };

export interface UnlinkWithIdentityVerificationOptions {
  readonly targetPath: string;
  /**
   * SHA-256 of the content the target is expected to hold. The target is
   * always moved to a same-directory quarantine before deletion, and the
   * displaced object is verified against this hash, so only the exact
   * validated object can ever be unlinked.
   */
  readonly expectedTargetSha256: string | null;
  /**
   * Optional final parent-chain identity verification, invoked immediately
   * before the displacement rename; see
   * `ReplaceFileWithQuarantineOptions.verifyParentIdentity`.
   */
  readonly verifyParentIdentity?: () => Promise<void>;
  readonly ops?: ReplacementFsOps;
}

/**
 * Safe delete primitive with the same compare-and-swap discipline as
 * `replaceFileWithQuarantine`: the target is displaced to a same-directory
 * quarantine, verified against the expected hash, and only then unlinked.
 * A target replaced between validation and deletion is detected by hash and
 * restored, so a later user change is never deleted.
 *
 * The quarantine is never unlinked while anything occupies the target path:
 * the target's absence is re-verified immediately before the unlink, and a
 * reoccupied target forces a link-based restore (which fails with EEXIST and
 * leaves the original recoverable in quarantine rather than overwriting).
 * Node offers no atomic unlink-if-absent primitive, so a target recreated in
 * the remaining window between the absence check and the unlink is preserved
 * (the unlink targets the quarantine path, never the target path) and the
 * caller's post-deletion verification reports the reappeared object.
 */
export async function unlinkWithIdentityVerification(
  options: UnlinkWithIdentityVerificationOptions,
): Promise<UnlinkOutcome> {
  const ops = options.ops ?? REAL_REPLACEMENT_FS_OPS;
  if (options.verifyParentIdentity !== undefined) {
    try {
      await options.verifyParentIdentity();
    } catch (error: unknown) {
      return {
        kind: "failed",
        message: `The deletion was refused before any change: the target's parent chain no longer verifies (${describeError(error)}).`,
        quarantinePath: null,
      };
    }
  }
  const quarantinePath = join(dirname(options.targetPath), `.solaris-quarantine-${randomUUID()}`);
  const expectedDevIno = await captureTargetIdentity(ops, options.targetPath);
  if (expectedDevIno === null) {
    return {
      kind: "failed",
      message: "The file could not be moved to quarantine: the target is not accessible.",
      quarantinePath: null,
    };
  }
  try {
    await ops.rename(options.targetPath, quarantinePath);
  } catch (error: unknown) {
    return {
      kind: "failed",
      message: `The file could not be moved to quarantine for deletion: ${describeError(error)}`,
      quarantinePath: null,
    };
  }
  const displacedIdentity = await verifyDisplacedIdentity(ops, quarantinePath, expectedDevIno);
  if (!displacedIdentity.ok) {
    const rollback = await restoreOriginal(ops, quarantinePath, options.targetPath);
    if (rollback.ok) {
      return {
        kind: "failed",
        message: `${displacedIdentity.message}; the displaced object was restored and nothing was deleted.`,
        quarantinePath: null,
      };
    }
    return {
      kind: "uncertain",
      message: `${displacedIdentity.message} and the displaced object could not be restored (${rollback.message}); the recoverable original is at ${quarantinePath}.`,
      quarantinePath,
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
          message: `${identityCheck.message}; the original was restored and nothing was deleted.`,
          quarantinePath: null,
        };
      }
      return {
        kind: "uncertain",
        message: `${identityCheck.message} and the original could not be restored (${rollback.message}); the recoverable original is at ${quarantinePath}.`,
        quarantinePath,
      };
    }
  }
  if (await targetIsOccupied(ops, options.targetPath)) {
    // A new object appeared at the target after the displacement. The
    // quarantine (the only remaining copy of the original) must not be
    // unlinked while it does, and the restore must not overwrite it.
    const rollback = await restoreOriginal(ops, quarantinePath, options.targetPath);
    if (rollback.ok) {
      return {
        kind: "failed",
        message:
          "A new target appeared before deletion; the original was restored and nothing was deleted.",
        quarantinePath: null,
      };
    }
    return {
      kind: "uncertain",
      message: `A new target appeared before deletion and the original could not be restored (${rollback.message}); the recoverable original is at ${quarantinePath}.`,
      quarantinePath,
    };
  }
  try {
    await ops.unlink(quarantinePath);
  } catch (error: unknown) {
    const rollback = await restoreOriginal(ops, quarantinePath, options.targetPath);
    if (rollback.ok) {
      return {
        kind: "failed",
        message: `The file could not be deleted (${describeError(error)}); the original was restored.`,
        quarantinePath: null,
      };
    }
    return {
      kind: "uncertain",
      message: `The file could not be deleted (${describeError(error)}) and the original could not be restored (${rollback.message}); the recoverable original is at ${quarantinePath}.`,
      quarantinePath,
    };
  }
  return { kind: "success", quarantinePath: null };
}

/**
 * Captures the dev+ino of the object currently at the target path. Returns
 * null when the target is not accessible (the rename would fail anyway).
 */
async function captureTargetIdentity(
  ops: ReplacementFsOps,
  targetPath: string,
): Promise<{ readonly dev: number | bigint; readonly ino: number | bigint } | null> {
  try {
    const stats = await ops.lstat(targetPath);
    return { dev: stats.dev, ino: stats.ino };
  } catch {
    return null;
  }
}

/**
 * Verifies that the quarantined path is the exact object that was captured
 * immediately before the displacement rename. A parent swapped in the final
 * window makes the rename displace a different object (or an object outside
 * the workspace), which the dev+ino comparison detects; the caller restores
 * the displaced object and fails closed instead of ever unlinking or
 * committing through it.
 */
async function verifyDisplacedIdentity(
  ops: ReplacementFsOps,
  quarantinePath: string,
  expected: { readonly dev: number | bigint; readonly ino: number | bigint },
): Promise<{ ok: true } | { ok: false; message: string }> {
  let stats;
  try {
    stats = await ops.lstat(quarantinePath);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The displaced object could not be inspected (${describeError(error)})`,
    };
  }
  if (stats.dev !== expected.dev || stats.ino !== expected.ino) {
    return {
      ok: false,
      message:
        "The displaced object is not the object that was at the target immediately before the commit; the target's parent may have been swapped",
    };
  }
  return { ok: true };
}

async function verifyQuarantinedIdentity(
  ops: ReplacementFsOps,
  quarantinePath: string,
  expectedSha256: string,
): Promise<{ ok: true } | { ok: false; message: string }> {
  let stats;
  try {
    stats = await ops.lstat(quarantinePath);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The quarantined original could not be inspected (${describeError(error)})`,
    };
  }
  // The displaced object must be a regular file, never a symlink: a symlink
  // displaced to quarantine would make readFile follow it and hash content
  // that is not the displaced object itself.
  if (stats.isSymbolicLink()) {
    return { ok: false, message: "The displaced object is a symbolic link" };
  }
  if (!stats.isFile()) {
    return { ok: false, message: "The displaced object is not a regular file" };
  }
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
      message: "The displaced original does not match the expected content hash",
    };
  }
  return { ok: true };
}

async function verifyCommittedTarget(
  ops: ReplacementFsOps,
  targetPath: string,
  expectedStagedSha256: string | null,
): Promise<{ ok: true } | { ok: false; message: string }> {
  let stats;
  try {
    stats = await ops.lstat(targetPath);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The committed replacement could not be verified (${describeError(error)})`,
    };
  }
  if (stats.isSymbolicLink()) {
    return { ok: false, message: "The committed replacement is a symbolic link" };
  }
  if (!stats.isFile()) {
    return { ok: false, message: "The committed replacement is not a regular file" };
  }
  if (expectedStagedSha256 !== null) {
    let bytes: Buffer;
    try {
      bytes = await ops.readFile(targetPath);
    } catch (error: unknown) {
      return {
        ok: false,
        message: `The committed replacement could not be read for verification (${describeError(error)})`,
      };
    }
    const hash = createHash("sha256").update(bytes).digest("hex");
    if (hash !== expectedStagedSha256) {
      return {
        ok: false,
        message:
          "Post-write verification failed: the committed replacement does not match the expected staged content hash",
      };
    }
  }
  return { ok: true };
}

/**
 * Link-based restore: creates the original at the target only when the
 * target is still absent. EEXIST (a newly appeared target) and hard-link
 * unsupported errors both fail the restore; the quarantine stays in place as
 * the recoverable copy. An overwrite-capable rename is never used here.
 */
async function restoreOriginal(
  ops: ReplacementFsOps,
  quarantinePath: string,
  targetPath: string,
): Promise<{ ok: true } | { ok: false; message: string }> {
  try {
    await ops.link(quarantinePath, targetPath);
  } catch (error: unknown) {
    return { ok: false, message: describeError(error) };
  }
  // The target now holds the original; the quarantine name is a redundant
  // hard link to the same inode, never the only copy, so it can be removed.
  // Best-effort: a failed removal leaves a harmless duplicate.
  try {
    await ops.unlink(quarantinePath);
  } catch {
    // The duplicate link remains; the original is recoverable at the target.
  }
  return { ok: true };
}

/**
 * Rollback after a committed-but-unverified target: only the exact staged
 * inode may be displaced, so the target is verified against the staged temp
 * file's dev+ino before the target link is unlinked; anything else at the
 * target is preserved and the original stays in quarantine (uncertain).
 */
async function rollbackCommittedTarget(
  ops: ReplacementFsOps,
  tempPath: string,
  targetPath: string,
  quarantinePath: string,
): Promise<{ ok: true } | { ok: false; message: string }> {
  let targetStats;
  let tempStats;
  try {
    targetStats = await ops.lstat(targetPath);
    tempStats = await ops.lstat(tempPath);
  } catch (error: unknown) {
    return { ok: false, message: describeError(error) };
  }
  if (targetStats.ino !== tempStats.ino || targetStats.dev !== tempStats.dev) {
    return { ok: false, message: "the committed object was replaced before rollback" };
  }
  try {
    await ops.unlink(targetPath);
  } catch (error: unknown) {
    return { ok: false, message: describeError(error) };
  }
  return restoreOriginal(ops, quarantinePath, targetPath);
}

async function targetIsOccupied(ops: ReplacementFsOps, targetPath: string): Promise<boolean> {
  try {
    await ops.lstat(targetPath);
    return true;
  } catch {
    return false;
  }
}

export async function removeQuarantinedCopy(
  quarantinePath: string,
  ops?: ReplacementFsOps,
): Promise<void> {
  await (ops ?? REAL_REPLACEMENT_FS_OPS).rm(quarantinePath, { force: true });
}
