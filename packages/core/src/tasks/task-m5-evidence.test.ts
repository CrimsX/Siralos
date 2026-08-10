import { describe, expect, it } from "vitest";
import { createReferenceId } from "../reference/reference-model.js";
import type { ReferenceAlias, ReferenceRevision } from "../reference/reference-model.js";
import { createTaskContract } from "./task-contract.js";
import { createTaskRuntime } from "./task-runtime.js";
import { createTaskRuntimeSnapshot } from "./task-snapshot.js";

const revision: ReferenceRevision = {
  identity: {
    kind: "repository",
    origin: "https://github.com/godotengine/godot",
    commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0",
    requestedRef: { kind: "commit", commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0" },
  },
  resolvedAtMs: 1_700_000_000_000,
};

const alias = "docs" as ReferenceAlias;
const referenceId = createReferenceId(alias);

function makeRuntime() {
  const runtime = createTaskRuntime({ now: () => 1_700_000_000_000 });
  const contract = createTaskContract({
    id: "task-m5-1",
    request: "Inspect the reference",
    constraints: [],
    acceptanceCriteria: [{ id: "c1", description: "done", verificationKind: "deterministic" }],
    pausePolicy: "on_approval",
  });
  const snapshot = createTaskRuntimeSnapshot({
    runtimeVersion: "task-runtime-1",
    provider: null,
    sandboxProfileId: "inspect",
    capabilityPolicyRevision: "policy-1",
    workspaceIdentity: "/home/user/project",
    godotEngineFingerprint: null,
    workflow: null,
  });
  const handle = runtime.createTask({
    contract,
    snapshot,
    steps: [
      {
        id: "investigate",
        description: "Investigate",
        kind: "research",
        accepts: ["reference_read", "reference_search", "research"],
      },
      { id: "strict", description: "Strict", kind: "research", accepts: ["workspace_read"] },
    ],
  });
  return { runtime, handle };
}

describe("milestone 5 evidence kinds", () => {
  it("completes a step with reference_read evidence", () => {
    const { handle } = makeRuntime();
    handle.beginStep("investigate");
    const attached = handle.attachEvidence({
      id: "ev-ref-read-1",
      kind: "reference_read",
      source: {
        type: "reference_read",
        referenceId,
        alias,
        revision,
        path: "docs/signals.md",
        mode: "exact",
        sha256: "a".repeat(64),
      },
    });
    expect(attached.status).toBe("attached");
    const completed = handle.completeStep("investigate", [
      { evidenceId: "ev-ref-read-1", kind: "reference_read" },
    ]);
    expect(completed).toEqual({ status: "ok" });
    const state = handle.snapshot();
    expect(state.steps[0]?.status).toBe("completed");
    expect(state.evidence[0]?.kind).toBe("reference_read");
  });

  it("completes a step with reference_search evidence", () => {
    const { handle } = makeRuntime();
    handle.beginStep("investigate");
    expect(
      handle.attachEvidence({
        id: "ev-ref-search-1",
        kind: "reference_search",
        source: {
          type: "reference_search",
          referenceId,
          alias,
          revision,
          query: "signals",
          matchCount: 3,
        },
      }).status,
    ).toBe("attached");
    expect(
      handle.completeStep("investigate", [
        { evidenceId: "ev-ref-search-1", kind: "reference_search" },
      ]),
    ).toEqual({ status: "ok" });
  });

  it("completes a step with research evidence", () => {
    const { handle } = makeRuntime();
    handle.beginStep("investigate");
    expect(
      handle.attachEvidence({
        id: "ev-research-1",
        kind: "research",
        source: {
          type: "research",
          source: { kind: "godot-docs", id: "godot-docs", label: "Godot documentation" },
          requestId: "req_abcd",
          fetchedAtMs: 1_700_000_000_000,
          resolvedRevision: "abc123",
          version: null,
          fallback: false,
          excerpt: "Signals connect objects.",
          truncated: false,
        },
      }).status,
    ).toBe("attached");
    expect(
      handle.completeStep("investigate", [{ evidenceId: "ev-research-1", kind: "research" }]),
    ).toEqual({ status: "ok" });
  });

  it("still rejects evidence kinds a step does not accept", () => {
    const { handle } = makeRuntime();
    handle.beginStep("strict");
    expect(
      handle.attachEvidence({
        id: "ev-ref-read-2",
        kind: "reference_read",
        source: {
          type: "reference_read",
          referenceId,
          alias,
          revision,
          path: "x",
          mode: "summary",
          sha256: "b".repeat(64),
        },
      }).status,
    ).toBe("attached");
    expect(
      handle.completeStep("strict", [{ evidenceId: "ev-ref-read-2", kind: "reference_read" }]),
    ).toMatchObject({ status: "rejected" });
  });

  it("attach-time byte bounds still catch oversized research sources", () => {
    const { handle } = makeRuntime();
    const oversized = "x".repeat(5000);
    const result = handle.attachEvidence({
      id: "ev-research-huge",
      kind: "research",
      source: {
        type: "research",
        source: { kind: "godot-docs", id: "godot-docs", label: "Godot documentation" },
        requestId: "req_abcd",
        fetchedAtMs: 1_700_000_000_000,
        resolvedRevision: null,
        version: null,
        fallback: false,
        excerpt: oversized,
        truncated: true,
      },
    });
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.reason).toContain("byte bound");
    }
  });
});

describe("milestone 5 task snapshot", () => {
  it("captures reference revisions at task start, bounded to 16", () => {
    const entries = Array.from({ length: 20 }, (_, index) => ({
      alias: `ref${index}` as ReferenceAlias,
      revision,
    }));
    const snapshot = createTaskRuntimeSnapshot({
      runtimeVersion: "task-runtime-1",
      provider: null,
      sandboxProfileId: "inspect",
      capabilityPolicyRevision: null,
      workspaceIdentity: "/home/user/project",
      godotEngineFingerprint: null,
      workflow: null,
      referenceRevisions: entries,
    });
    expect(snapshot.referenceRevisions).toHaveLength(16);
    expect(snapshot.referenceRevisions[0]?.alias).toBe("ref0");
  });

  it("defaults to an empty reference-revision list when not provided", () => {
    const snapshot = createTaskRuntimeSnapshot({
      runtimeVersion: "task-runtime-1",
      provider: null,
      sandboxProfileId: null,
      capabilityPolicyRevision: null,
      workspaceIdentity: null,
      godotEngineFingerprint: null,
      workflow: null,
    });
    expect(snapshot.referenceRevisions).toEqual([]);
  });

  it("keeps the captured snapshot immutable after a registry-style refresh", () => {
    const entries = [{ alias, revision }];
    const snapshot = createTaskRuntimeSnapshot({
      runtimeVersion: "task-runtime-1",
      provider: null,
      sandboxProfileId: "inspect",
      capabilityPolicyRevision: null,
      workspaceIdentity: "/home/user/project",
      godotEngineFingerprint: null,
      workflow: null,
      referenceRevisions: entries,
    });
    expect(snapshot.referenceRevisions[0]?.revision.identity).toMatchObject({
      kind: "repository",
      commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0",
    });
    expect(Object.isFrozen(snapshot)).toBe(true);
    expect(Object.isFrozen(snapshot.referenceRevisions)).toBe(true);
    expect(Object.isFrozen(snapshot.referenceRevisions[0])).toBe(true);
    expect(Object.isFrozen(snapshot.referenceRevisions[0]?.revision.identity)).toBe(true);
    expect(() => {
      (snapshot.referenceRevisions as unknown as unknown[]).push({});
    }).toThrow();
  });
});
