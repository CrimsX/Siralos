import { describe, expect, it } from "vitest";
import {
  DEFAULT_SOURCE_EXCLUSIONS,
  createActiveWorkingSet,
  createWorkspaceScope,
  evictLowValueContext,
  isExcludedSourcePath,
  promoteCandidateFile,
  setFileView,
  type WorkspaceScope,
} from "./workspace-scope.js";

const REV = "rev_".padEnd(36, "a");

function baseScope(): WorkspaceScope {
  return createWorkspaceScope({
    verifiedFiles: [
      {
        path: "packages/core/src/executor/context-pack.ts",
        confidence: "verified",
        view: "structural",
        revision: REV,
        evidence: "read:packages/core/src/executor/context-pack.ts",
      },
    ],
    candidateFiles: [
      {
        path: "packages/core/src/executor/brief-compiler.ts",
        confidence: "candidate",
        view: "none",
      },
    ],
  });
}

describe("workspace scope", () => {
  it("keeps verified and candidate files distinct and never mixes them", () => {
    const scope = baseScope();
    expect(scope.verifiedFiles.map((file) => file.path)).toEqual([
      "packages/core/src/executor/context-pack.ts",
    ]);
    expect(scope.candidateFiles.map((file) => file.path)).toEqual([
      "packages/core/src/executor/brief-compiler.ts",
    ]);
    // A candidate may never appear in the verified set without promotion.
    expect(scope.verifiedFiles.some((file) => file.path.includes("brief-compiler"))).toBe(false);
  });

  it("rejects a verified file without an exact revision handle", () => {
    expect(() =>
      createWorkspaceScope({
        verifiedFiles: [
          {
            path: "src/a.ts",
            confidence: "verified",
            view: "exact",
            evidence: "read:src/a.ts",
          },
        ],
      }),
    ).toThrow(/revision handle/);
  });

  it("rejects a verified file without evidence — a guess is never verified", () => {
    expect(() =>
      createWorkspaceScope({
        verifiedFiles: [
          {
            path: "src/a.ts",
            confidence: "verified",
            view: "exact",
            revision: REV,
          },
        ],
      }),
    ).toThrow(/evidence/);
  });

  it("promotes a candidate to verified only with evidence, recording the promotion", () => {
    const scope = baseScope();
    const { scope: promoted, record } = promoteCandidateFile(
      scope,
      "packages/core/src/executor/brief-compiler.ts",
      {
        evidence: "read:packages/core/src/executor/brief-compiler.ts",
        revision: REV,
        reason: "exact behavior analysis required by current step",
      },
    );
    expect(record.evidence).toContain("read:");
    expect(promoted.verifiedFiles.map((file) => file.path)).toContain(
      "packages/core/src/executor/brief-compiler.ts",
    );
    // The promoted file leaves the candidate set: sets never overlap.
    expect(promoted.candidateFiles).toHaveLength(0);
    expect(promoted.promotions).toHaveLength(1);
    expect(promoted.promotions[0]!.path).toBe("packages/core/src/executor/brief-compiler.ts");
  });

  it("refuses promotion of an unknown candidate", () => {
    const scope = baseScope();
    expect(() =>
      promoteCandidateFile(scope, "src/unknown.ts", {
        evidence: "read:src/unknown.ts",
        revision: REV,
        reason: "needed",
      }),
    ).toThrow(/unknown candidate/);
  });

  it("excludes noisy generated and vendor paths by default", () => {
    for (const path of [
      "node_modules/@siralos/core/index.js",
      "dist/index.js",
      "build/out.js",
      "coverage/lcov.info",
      ".git/config",
      ".godot/imported/thing",
      "generated/models.ts",
    ]) {
      expect(isExcludedSourcePath(path), path).toBe(true);
    }
    for (const path of ["src/main.ts", "packages/core/src/index.ts", "project.godot"]) {
      expect(isExcludedSourcePath(path), path).toBe(false);
    }
    expect(DEFAULT_SOURCE_EXCLUSIONS).toContain("node_modules/");
  });

  it("treats exclusion as context suppression, not security denial", () => {
    // An explicitly required path is still read when the task needs it.
    const scope = createWorkspaceScope({
      verifiedFiles: [
        {
          path: "dist/artifact.ts",
          confidence: "verified",
          view: "exact",
          revision: REV,
          evidence: "read:dist/artifact.ts",
        },
      ],
    });
    expect(scope.verifiedFiles[0]!.path).toBe("dist/artifact.ts");
  });

  it("bounds the working set to a small current-step subset with inclusion reasons", () => {
    const workingSet = createActiveWorkingSet({
      stepId: "s2",
      files: [
        {
          path: "packages/core/src/executor/context-pack.ts",
          reason: "direct task target",
          view: "exact",
        },
        {
          path: "packages/core/src/executor/brief-compiler.ts",
          reason: "dependency",
          view: "structural",
        },
      ],
    });
    expect(workingSet.files).toHaveLength(2);
    expect(workingSet.files[0]!.reason).toBe("direct task target");
    expect(() =>
      createActiveWorkingSet({
        stepId: "s1",
        files: [{ path: "a.ts", reason: "no such reason" as never, view: "exact" }],
      }),
    ).toThrow(/inclusion reason/);
  });

  it("evicts low-value exact source over budget while retaining revision and evidence", () => {
    const scope = createWorkspaceScope({
      verifiedFiles: [
        {
          path: "src/active.ts",
          confidence: "verified",
          view: "exact",
          revision: REV,
          evidence: "read:src/active.ts",
        },
        {
          path: "src/stale.ts",
          confidence: "verified",
          view: "exact",
          revision: REV,
          evidence: "read:src/stale.ts",
        },
      ],
      candidateFiles: [{ path: "src/guess.ts", confidence: "candidate", view: "exact" }],
      budget: {
        maxActiveExactFiles: 1,
        maxExactBytes: 1024 * 1024,
        maxStructuralSummaries: 12,
        maxCandidateFiles: 16,
        maxRetainedHistoricalViews: 4,
      },
    });
    const workingSet = createActiveWorkingSet({
      stepId: "s1",
      files: [{ path: "src/active.ts", reason: "direct task target", view: "exact" }],
    });
    const { scope: after, evicted } = evictLowValueContext({ scope, workingSet });
    expect(evicted.length).toBe(2);
    // Eviction order: candidate details first, then exact source outside
    // the working set. The active file is never evicted here.
    expect(evicted[0]!.path).toBe("src/guess.ts");
    expect(evicted[1]!.path).toBe("src/stale.ts");
    expect(after.verifiedFiles.find((file) => file.path === "src/stale.ts")?.view).toBe("summary");
    // Revision identity and evidence are retained after eviction.
    expect(after.verifiedFiles.find((file) => file.path === "src/stale.ts")?.revision).toBe(REV);
    expect(after.verifiedFiles.find((file) => file.path === "src/stale.ts")?.evidence).toContain(
      "read:",
    );
    expect(after.verifiedFiles.find((file) => file.path === "src/active.ts")?.view).toBe("exact");
    // Authoritative evidence is never deleted: the file still exists in scope.
    expect(after.verifiedFiles.map((file) => file.path)).toContain("src/stale.ts");
  });

  it("accounts host-observed exact bytes deterministically", () => {
    const scope = createWorkspaceScope({
      verifiedFiles: [
        {
          path: "src/big.ts",
          confidence: "verified",
          view: "exact",
          revision: REV,
          evidence: "read:src/big.ts",
        },
      ],
      budget: {
        maxActiveExactFiles: 4,
        maxExactBytes: 100,
        maxStructuralSummaries: 12,
        maxCandidateFiles: 16,
        maxRetainedHistoricalViews: 4,
      },
    });
    const { evicted } = evictLowValueContext({
      scope,
      workingSet: null,
      exactBytesOf: { "src/big.ts": 200 },
    });
    expect(evicted).toHaveLength(1);
    expect(evicted[0]!.reason).toContain("over budget");
  });

  it("is immutable: mutation helpers return new frozen scopes", () => {
    const scope = baseScope();
    const promoted = setFileView(scope, "packages/core/src/executor/context-pack.ts", "exact");
    expect(scope.verifiedFiles[0]!.view).toBe("structural");
    expect(promoted.verifiedFiles[0]!.view).toBe("exact");
    expect(Object.isFrozen(promoted)).toBe(true);
  });

  it("is deterministic: identical inputs produce identical scopes", () => {
    const a = baseScope();
    const b = baseScope();
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
  });
});
