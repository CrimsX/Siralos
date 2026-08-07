import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import {
  REAL_REPLACEMENT_FS_OPS,
  replaceFileWithQuarantine,
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
 * Fault-injection seam: fails specific rename/readFile calls by 1-based call
 * index so deterministic race scenarios are reproducible without timing.
 */
function recordingOps(
  options: {
    readonly failRenameCalls?: readonly number[];
    readonly failReadFileCalls?: readonly number[];
    readonly failRmCalls?: readonly number[];
  } = {},
): ReplacementFsOps {
  let renameCalls = 0;
  let readFileCalls = 0;
  let rmCalls = 0;
  return {
    async rename(from, to) {
      renameCalls += 1;
      if ((options.failRenameCalls ?? []).includes(renameCalls)) {
        throw new Error(`injected rename failure (call ${renameCalls})`);
      }
      await REAL_REPLACEMENT_FS_OPS.rename(from, to);
    },
    async unlink(path) {
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
  it("commits atomically on the direct rename path without a quarantine", async () => {
    const ops = recordingOps();
    const { target, temp, newHash } = await setupReplacement();
    const outcome = await replaceFileWithQuarantine({
      tempPath: temp,
      targetPath: target,
      expectedTargetSha256: "any-hash",
      ops,
    });
    expect(outcome).toEqual({ kind: "success", quarantinePath: null });
    expect(sha256(await readFile(target))).toBe(newHash);
  });

  it("recovers through quarantine when the first rename fails", async () => {
    const ops = recordingOps({ failRenameCalls: [1] });
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
    if (outcome.quarantinePath !== null) {
      expect(sha256(await readFile(outcome.quarantinePath))).toBe(originalHash);
    }
  });

  it("fails without committing when the quarantine rename fails", async () => {
    const ops = recordingOps({ failRenameCalls: [1, 2] });
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

  it("restores the original when the second rename fails", async () => {
    const ops = recordingOps({ failRenameCalls: [1, 3] });
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
    const ops = recordingOps({ failRenameCalls: [1, 3, 4] });
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

  it("never destroys a substituted target: identity mismatch restores it", async () => {
    const ops = recordingOps({ failRenameCalls: [1] });
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

  it("restores the original when the quarantined copy cannot be read", async () => {
    const ops = recordingOps({ failRenameCalls: [1], failReadFileCalls: [1] });
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
