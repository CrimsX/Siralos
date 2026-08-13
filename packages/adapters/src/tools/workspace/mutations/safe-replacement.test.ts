import { mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { SYMLINKS_SUPPORTED } from "../workspace-fixtures.js";
import {
  REAL_REPLACEMENT_FS_OPS,
  replaceFileWithQuarantine,
  unlinkWithIdentityVerification,
  type ReplacementFsOps,
} from "./safe-replacement.js";

const tempDirectories: string[] = [];

async function withDirectory(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "siralos-replace-"));
  tempDirectories.push(directory);
  return directory;
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

function sha256(bytes: Buffer): string {
  return createHash("sha256").update(bytes).digest("hex");
}

function injectedError(code: string, message: string): Error & { readonly code: string } {
  const error = new Error(message) as Error & { code: string };
  error.code = code;
  return error;
}

/**
 * Fault-injection seam: fails specific rename/link/readFile/unlink/lstat/rm
 * calls by 1-based call index and runs optional deterministic barriers
 * before each call, so race scenarios are reproducible without timing.
 */
function recordingOps(
  options: {
    readonly failRenameCalls?: readonly number[];
    readonly failLinkCalls?: readonly number[];
    readonly failReadFileCalls?: readonly number[];
    readonly failRmCalls?: readonly number[];
    readonly failUnlinkCalls?: readonly number[];
    readonly failLstatCalls?: readonly number[];
    readonly beforeRename?: (from: string, to: string, callIndex: number) => Promise<void>;
    readonly beforeLink?: (from: string, to: string, callIndex: number) => Promise<void>;
    readonly beforeUnlink?: (path: string, callIndex: number) => Promise<void>;
    readonly beforeLstat?: (path: string, callIndex: number) => Promise<void>;
  } = {},
): ReplacementFsOps & {
  readonly renameCalls: () => number;
  readonly linkCalls: () => number;
  readonly lstatCalls: () => number;
  readonly rmCalls: () => number;
} {
  let renameCalls = 0;
  let linkCalls = 0;
  let readFileCalls = 0;
  let rmCalls = 0;
  let unlinkCalls = 0;
  let lstatCalls = 0;
  return {
    async rename(from, to) {
      renameCalls += 1;
      if (options.beforeRename !== undefined) {
        await options.beforeRename(from, to, renameCalls);
      }
      if ((options.failRenameCalls ?? []).includes(renameCalls)) {
        throw new Error(`injected rename failure (call ${renameCalls})`);
      }
      await REAL_REPLACEMENT_FS_OPS.rename(from, to);
    },
    async link(from, to) {
      linkCalls += 1;
      if (options.beforeLink !== undefined) {
        await options.beforeLink(from, to, linkCalls);
      }
      if ((options.failLinkCalls ?? []).includes(linkCalls)) {
        throw new Error(`injected link failure (call ${linkCalls})`);
      }
      await REAL_REPLACEMENT_FS_OPS.link(from, to);
    },
    async unlink(path) {
      unlinkCalls += 1;
      if (options.beforeUnlink !== undefined) {
        await options.beforeUnlink(path, unlinkCalls);
      }
      if ((options.failUnlinkCalls ?? []).includes(unlinkCalls)) {
        throw new Error(`injected unlink failure (call ${unlinkCalls})`);
      }
      await REAL_REPLACEMENT_FS_OPS.unlink(path);
    },
    async readFile(path) {
      readFileCalls += 1;
      if ((options.failReadFileCalls ?? []).includes(readFileCalls)) {
        throw new Error(`injected read failure (call ${readFileCalls})`);
      }
      return REAL_REPLACEMENT_FS_OPS.readFile(path);
    },
    async lstat(path) {
      lstatCalls += 1;
      if (options.beforeLstat !== undefined) {
        await options.beforeLstat(path, lstatCalls);
      }
      if ((options.failLstatCalls ?? []).includes(lstatCalls)) {
        throw new Error(`injected lstat failure (call ${lstatCalls})`);
      }
      return REAL_REPLACEMENT_FS_OPS.lstat(path);
    },
    async rm(path) {
      rmCalls += 1;
      if ((options.failRmCalls ?? []).includes(rmCalls)) {
        throw new Error(`injected rm failure (call ${rmCalls})`);
      }
      await REAL_REPLACEMENT_FS_OPS.rm(path, { force: true });
    },
    renameCalls: () => renameCalls,
    linkCalls: () => linkCalls,
    lstatCalls: () => lstatCalls,
    rmCalls: () => rmCalls,
  };
}

async function setupReplacement(): Promise<{
  directory: string;
  target: string;
  temp: string;
  originalHash: string;
  newHash: string;
}> {
  const directory = await withDirectory();
  const target = join(directory, "file.txt");
  const temp = join(directory, "staged.tmp");
  await writeFile(target, "original content\n");
  await writeFile(temp, "new content\n");
  return {
    directory,
    target,
    temp,
    originalHash: sha256(await readFile(target)),
    newHash: sha256(await readFile(temp)),
  };
}

describe("safe replacement state machine", () => {
  it("commits through quarantine and always verifies the displaced original", async () => {
    const ops = recordingOps();
    const { target, temp, newHash, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: newHash,
      ops,
    });
    expect(outcome.kind).toBe("success");
    if (outcome.kind !== "success") {
      return;
    }
    expect(outcome.quarantinePath).not.toBeNull();
    expect(sha256(await readFile(target))).toBe(newHash);
    expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
    // The staged temp file remains as a hard link to the committed object;
    // the caller removes the temp link once the outcome is final.
    expect(sha256(await readFile(temp))).toBe(newHash);
  });

  it("fails without committing when the original cannot be moved to quarantine", async () => {
    const ops = recordingOps({ failRenameCalls: [1] });
    const { target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: null,
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(originalHash);
  });

  it("never commits over a substituted target even without any injected failure", async () => {
    const ops = recordingOps();
    const { target, temp } = await setupReplacement();
    const substituted = Buffer.from("substituted by an attacker\n");
    await writeFile(target, substituted);
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: sha256(Buffer.from("original content\n")),
      expectedStagedSha256: null,
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(sha256(substituted));
  });

  it("preserves a later user change made immediately before the commit displacement", async () => {
    const concurrent = Buffer.from("concurrent user edit\n");
    const ops = recordingOps({
      beforeRename: async (from, _to, callIndex) => {
        if (callIndex === 1) {
          await writeFile(from, concurrent);
        }
      },
    });
    const { target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: null,
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(sha256(concurrent));
  });

  it("restores the original when the commit link fails", async () => {
    const ops = recordingOps({ failLinkCalls: [1] });
    const { target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: null,
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(originalHash);
  });

  it("reports an uncertain state with a recoverable quarantine when rollback fails", async () => {
    const ops = recordingOps({ failLinkCalls: [1, 2] });
    const { directory, target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: null,
      ops,
    });
    expect(outcome.kind).toBe("uncertain");
    if (outcome.kind !== "uncertain") {
      return;
    }
    expect(outcome.message).toContain("recoverable original");
    expect(outcome.quarantinePath.startsWith(directory)).toBe(true);
    expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
  });

  it("restores the original when the quarantined copy cannot be read", async () => {
    const ops = recordingOps({ failReadFileCalls: [1] });
    const { target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: null,
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(originalHash);
  });

  it("never overwrites a new target that appears after the quarantine verification", async () => {
    // Seam variant: the commit link itself reports EEXIST.
    const { target, temp, originalHash } = await setupReplacement();
    const raced = Buffer.from("new file created by another process\n");
    const ops = recordingOps({
      failLinkCalls: [1],
      beforeLink: async (from, _to, callIndex) => {
        if (callIndex === 1 && from.endsWith("staged.tmp")) {
          await writeFile(target, raced);
        }
      },
    });
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: null,
      ops,
    });
    expect(outcome.kind).toBe("uncertain");
    if (outcome.kind !== "uncertain") {
      return;
    }
    expect(sha256(await readFile(target))).toBe(sha256(raced));
    expect(outcome.message).toContain("recoverable original");
    expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
  });

  it("never overwrites a target re-created through the real filesystem after verification", async () => {
    // Real-fs variant: everything except the barrier is the real fs, so the
    // commit link fails with a real EEXIST from the operating system.
    const { target, temp, originalHash } = await setupReplacement();
    const ops = recordingOps({
      beforeLink: async (from, _to, callIndex) => {
        if (callIndex === 1 && from.endsWith("staged.tmp")) {
          await writeFile(target, "raced between verification and commit\n");
        }
      },
    });
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: null,
      ops,
    });
    expect(outcome.kind).toBe("uncertain");
    if (outcome.kind !== "uncertain") {
      return;
    }
    expect(await readFile(target, "utf8")).toBe("raced between verification and commit\n");
    expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
  });

  it("leaves the original in quarantine and reports uncertain when a new target appears before rollback", async () => {
    const { target, temp, originalHash } = await setupReplacement();
    const ops = recordingOps({
      failReadFileCalls: [1],
      beforeLink: async (from, _to, callIndex) => {
        if (callIndex === 1 && from.includes(".siralos-quarantine-")) {
          await writeFile(target, "appeared before rollback\n");
        }
      },
    });
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: null,
      ops,
    });
    expect(outcome.kind).toBe("uncertain");
    if (outcome.kind !== "uncertain") {
      return;
    }
    expect(await readFile(target, "utf8")).toBe("appeared before rollback\n");
    expect(outcome.message).toContain("recoverable original");
    expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
  });

  it(
    "fails closed when a symbolic link appears at the target before the commit",
    { skip: !SYMLINKS_SUPPORTED },
    async () => {
      const ops = recordingOps({
        beforeLink: async (from, to, callIndex) => {
          if (callIndex === 1 && from.endsWith("staged.tmp")) {
            // The target was already displaced to quarantine, so a symlink
            // can be placed at the target without unlinking anything.
            await symlink("elsewhere", to);
          }
        },
      });
      const { target, temp, originalHash } = await setupReplacement();
      const outcome = await replaceFileWithQuarantine({
        tempPath: temp,
        targetPath: target,
        expectedTargetSha256: originalHash,
        expectedStagedSha256: null,
        ops,
      });
      expect(outcome.kind).toBe("uncertain");
      if (outcome.kind !== "uncertain") {
        return;
      }
      const stats = await REAL_REPLACEMENT_FS_OPS.lstat(target);
      expect(stats.isSymbolicLink()).toBe(true);
      expect(outcome.message).toContain("recoverable original");
      expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
    },
  );

  it("fails closed when hard links are unsupported, preserving the quarantine", async () => {
    const ops: ReplacementFsOps = {
      ...recordingOps(),
      link() {
        throw injectedError("ENOTSUP", "hard links are not supported on this filesystem");
      },
    };
    const { directory, target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: null,
      ops,
    });
    expect(outcome.kind).toBe("uncertain");
    if (outcome.kind !== "uncertain") {
      return;
    }
    expect(outcome.quarantinePath.startsWith(directory)).toBe(true);
    expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
    await expect(readFile(target)).rejects.toThrow();
  });

  it("rolls back the committed object when its hash no longer matches the staged content", async () => {
    const ops = recordingOps({
      beforeLink: async (from, _to, callIndex) => {
        if (callIndex === 1 && from.endsWith("staged.tmp")) {
          // Tamper the staged file in place (same inode): the exclusive
          // link commits the tampered bytes.
          await writeFile(from, "tampered staged content\n");
        }
      },
    });
    const { target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: sha256(Buffer.from("new content\n")),
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(originalHash);
  });

  it("reports uncertain and preserves the quarantine when the committed object is replaced", async () => {
    const ops = recordingOps({
      beforeLink: async (_from, to, callIndex) => {
        if (callIndex === 1 && to.endsWith("file.txt")) {
          const fs = await import("node:fs/promises");
          await fs.rm(to, { force: true });
          await fs.writeFile(to, "replaced after commit\n");
        }
      },
    });
    const { target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: sha256(Buffer.from("new content\n")),
      ops,
    });
    expect(outcome.kind).toBe("uncertain");
    if (outcome.kind !== "uncertain") {
      return;
    }
    expect(await readFile(target, "utf8")).toBe("replaced after commit\n");
    expect(outcome.message).toContain("recoverable original");
    expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
  });
});

describe("safe deletion state machine", () => {
  it("deletes only the verified displaced object", async () => {
    const ops = recordingOps();
    const { target, originalHash } = await setupReplacement();
    const outcome = await unlinkWithIdentityVerification({
      targetPath: target,
      expectedTargetSha256: originalHash,
      ops,
    });
    expect(outcome.kind).toBe("success");
    await expect(readFile(target)).rejects.toThrow();
  });

  it("preserves a substituted target between validation and deletion", async () => {
    const ops = recordingOps({
      beforeRename: async (from, _to, callIndex) => {
        if (callIndex === 1) {
          await writeFile(from, "newer content\n");
        }
      },
    });
    const { target, originalHash } = await setupReplacement();
    const outcome = await unlinkWithIdentityVerification({
      targetPath: target,
      expectedTargetSha256: originalHash,
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(await readFile(target, "utf8")).toBe("newer content\n");
  });

  it("restores the original when the quarantine unlink fails", async () => {
    const ops = recordingOps({ failUnlinkCalls: [1] });
    const { target, originalHash } = await setupReplacement();
    const outcome = await unlinkWithIdentityVerification({
      targetPath: target,
      expectedTargetSha256: originalHash,
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(originalHash);
  });

  it("reports an uncertain state with a recoverable quarantine when deletion rollback fails", async () => {
    const ops = recordingOps({ failUnlinkCalls: [1], failLinkCalls: [1] });
    const { directory, target, originalHash } = await setupReplacement();
    const outcome = await unlinkWithIdentityVerification({
      targetPath: target,
      expectedTargetSha256: originalHash,
      ops,
    });
    expect(outcome.kind).toBe("uncertain");
    if (outcome.kind !== "uncertain") {
      return;
    }
    expect(outcome.quarantinePath.startsWith(directory)).toBe(true);
    expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
  });

  it("never unlinks the quarantine while a new target occupies the path", async () => {
    const ops = recordingOps({
      beforeLstat: async (path, callIndex) => {
        // The lstat calls are: (1) target identity capture, (2) displaced
        // identity verification, (3) quarantined hash verification, and
        // (4) the pre-unlink target-absence check: a new object appears
        // exactly there.
        if (callIndex === 4 && path.endsWith("file.txt")) {
          await writeFile(path, "new target appeared\n");
        }
      },
    });
    const { target, originalHash } = await setupReplacement();
    const outcome = await unlinkWithIdentityVerification({
      targetPath: target,
      expectedTargetSha256: originalHash,
      ops,
    });
    expect(outcome.kind).toBe("uncertain");
    if (outcome.kind !== "uncertain") {
      return;
    }
    expect(await readFile(target, "utf8")).toBe("new target appeared\n");
    expect(outcome.message).toContain("recoverable original");
    expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
  });

  it("fails closed before any rename when the parent chain no longer verifies", async () => {
    const ops = recordingOps({});
    const { target, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: join(join(target, ".."), "staged.tmp"),
      targetPath: target,
      expectedTargetSha256: originalHash,
      expectedStagedSha256: null,
      ops,
      verifyParentIdentity: () => Promise.reject(new Error("parent path component swapped")),
    });
    expect(outcome.kind).toBe("failed");
    if (outcome.kind !== "failed") {
      return;
    }
    expect(outcome.message).toContain("refused before any change");
    expect(outcome.quarantinePath).toBeNull();
    // Nothing was renamed: the original is untouched at the target.
    expect(await readFile(target, "utf8")).toBe("original content\n");
    expect(ops.renameCalls()).toBe(0);
  });

  it("restores and fails closed when the displaced object is not the captured one", async () => {
    const ops = recordingOps({
      beforeRename: async (_from, _to) => {
        // Between the identity capture and the displacement rename the
        // target is substituted with a different object (a swapped parent
        // would do the same): the rename displaces the substitute, whose
        // dev+ino differs from the captured identity. Keep the captured
        // object linked at a side path so POSIX cannot immediately reuse
        // its inode for the substitute and mask the identity branch behind
        // the later content-hash check.
        await REAL_REPLACEMENT_FS_OPS.rename(_from, `${_from}.captured-original`);
        await writeFile(_from, "swapped content\n");
      },
    });
    const { target, originalHash } = await setupReplacement();
    const outcome = await unlinkWithIdentityVerification({
      targetPath: target,
      expectedTargetSha256: originalHash,
      ops,
    });
    expect(outcome.kind).toBe("failed");
    if (outcome.kind !== "failed") {
      return;
    }
    expect(outcome.message).toContain("not the object that was at the target");
    expect(outcome.quarantinePath).toBeNull();
    // The displaced substitute was restored to the target; nothing was
    // deleted and nothing committed.
    expect(await readFile(target, "utf8")).toBe("swapped content\n");
  });
});
