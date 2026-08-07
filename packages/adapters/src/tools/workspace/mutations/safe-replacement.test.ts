import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import {
  REAL_REPLACEMENT_FS_OPS,
  replaceFileWithQuarantine,
  unlinkWithIdentityVerification,
  type ReplacementFsOps,
} from "./safe-replacement.js";

const tempDirectories: string[] = [];

async function withDirectory(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-replace-"));
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

/**
 * Fault-injection seam: fails specific rename/readFile/unlink calls by
 * 1-based call index and runs optional deterministic barriers before each
 * call, so race scenarios are reproducible without timing.
 */
function recordingOps(
  options: {
    readonly failRenameCalls?: readonly number[];
    readonly failReadFileCalls?: readonly number[];
    readonly failRmCalls?: readonly number[];
    readonly failUnlinkCalls?: readonly number[];
    readonly beforeRename?: (from: string, to: string, callIndex: number) => Promise<void>;
  } = {},
): ReplacementFsOps {
  let renameCalls = 0;
  let readFileCalls = 0;
  let rmCalls = 0;
  let unlinkCalls = 0;
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
    async unlink(path) {
      unlinkCalls += 1;
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
      return REAL_REPLACEMENT_FS_OPS.lstat(path);
    },
    async rm(path) {
      rmCalls += 1;
      if ((options.failRmCalls ?? []).includes(rmCalls)) {
        throw new Error(`injected rm failure (call ${rmCalls})`);
      }
      await REAL_REPLACEMENT_FS_OPS.rm(path, { force: true });
    },
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
      ops,
    });
    expect(outcome.kind).toBe("success");
    if (outcome.kind !== "success") {
      return;
    }
    expect(outcome.quarantinePath).not.toBeNull();
    expect(sha256(await readFile(target))).toBe(newHash);
    expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
  });

  it("fails without committing when the original cannot be moved to quarantine", async () => {
    const ops = recordingOps({ failRenameCalls: [1] });
    const { target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
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
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(sha256(concurrent));
  });

  it("restores the original when the commit rename fails", async () => {
    const ops = recordingOps({ failRenameCalls: [2] });
    const { target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(originalHash);
  });

  it("reports an uncertain state with a recoverable quarantine when rollback fails", async () => {
    const ops = recordingOps({ failRenameCalls: [2, 3] });
    const { directory, target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
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
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(originalHash);
  });

  it("restores the original when the committed replacement is not a regular file", async () => {
    const ops = recordingOps({
      beforeRename: async (_from, to, callIndex) => {
        if (callIndex === 2) {
          const temp = await import("node:fs/promises");
          await temp.rm(to, { force: true });
          await temp.symlink("elsewhere", to);
        }
      },
    });
    const { target, temp, originalHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: originalHash,
      ops,
    });
    expect(outcome.kind).toBe("failed");
    expect(sha256(await readFile(target))).toBe(originalHash);
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
    const ops = recordingOps({ failUnlinkCalls: [1], failRenameCalls: [2] });
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
});
