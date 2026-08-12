import { describe, expect, it } from "vitest";
import type { GitStatusResult } from "@siralos/core";
import {
  canonicalizeGitStatus,
  compareWorkspaceIntegrity,
  type WorkspaceIntegritySnapshot,
} from "./workspace-integrity.js";

function sampleGitStatus(): GitStatusResult {
  return {
    repository: true,
    branch: {
      head: "main",
      detached: false,
      unborn: false,
      oid: null,
      upstream: null,
      ahead: 0,
      behind: 0,
    },
    changes: [
      {
        path: "a.gd",
        indexStatus: "modified",
        worktreeStatus: "modified",
        originalPath: null,
        kind: "ordinary",
      },
      {
        path: "b.gd",
        indexStatus: "unmodified",
        worktreeStatus: "modified",
        originalPath: null,
        kind: "ordinary",
      },
    ],
    conflicts: [],
    untracked: ["new.txt"],
    truncated: false,
  };
}

function snapshot(overrides: Partial<WorkspaceIntegritySnapshot> = {}): WorkspaceIntegritySnapshot {
  return {
    gitStatus: canonicalizeGitStatus(sampleGitStatus()),
    authored: {
      entries: [],
      fileCount: 3,
      totalBytes: 99,
      digest: "d".repeat(64),
      truncated: false,
    },
    ...overrides,
  };
}

describe("canonicalizeGitStatus", () => {
  it("is deterministic across change orderings", () => {
    const status = sampleGitStatus();
    const reordered: GitStatusResult = {
      ...status,
      changes: [
        status.changes[1] as GitStatusResult["changes"][number],
        status.changes[0] as GitStatusResult["changes"][number],
      ],
    };
    expect(canonicalizeGitStatus(status)).toBe(canonicalizeGitStatus(reordered));
  });

  it("changes when the repository view changes", () => {
    const status = sampleGitStatus();
    const changed: GitStatusResult = {
      ...status,
      untracked: ["other.txt"],
    };
    expect(canonicalizeGitStatus(changed)).not.toBe(canonicalizeGitStatus(status));
  });
});

describe("compareWorkspaceIntegrity", () => {
  it("reports unchanged when everything matches", () => {
    const before = snapshot();
    const after = snapshot();
    expect(compareWorkspaceIntegrity(before, after)).toEqual({ unchanged: true, bounded: false });
  });

  it("detects authored content changes", () => {
    const before = snapshot();
    const after = snapshot({
      authored: {
        entries: [],
        fileCount: 4,
        totalBytes: 99,
        digest: "d".repeat(64),
        truncated: false,
      },
    });
    expect(compareWorkspaceIntegrity(before, after).unchanged).toBe(false);
  });

  it("detects git view changes when a git baseline exists", () => {
    const before = snapshot();
    const after = snapshot({ gitStatus: "different" });
    expect(compareWorkspaceIntegrity(before, after).unchanged).toBe(false);
  });

  it("does not fail when no git baseline was available", () => {
    const before = snapshot({ gitStatus: null });
    const after = snapshot({ gitStatus: "different" });
    expect(compareWorkspaceIntegrity(before, after).unchanged).toBe(true);
  });

  it("flags bounded baselines", () => {
    const before = snapshot({
      authored: {
        entries: [],
        fileCount: 3,
        totalBytes: 99,
        digest: "d".repeat(64),
        truncated: true,
      },
    });
    const after = snapshot();
    expect(compareWorkspaceIntegrity(before, after)).toEqual({ unchanged: true, bounded: true });
  });
});
