import { describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  createProjectionService,
  createTaskContract,
  createTaskRuntime,
  createTaskRuntimeSnapshot,
  createToolProjector,
  DEVELOP_OFFLINE_PROFILE,
  INSPECT_PROFILE,
  type Capability,
  type ReferenceEvidenceView,
  type RegisteredToolInfo,
  type ResearchEvidence,
} from "../index.js";
import { createReferenceId } from "../reference/reference-model.js";
import type { ReferenceAlias, ReferenceRevision } from "../reference/reference-model.js";

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

function referenceView(overrides: Partial<ReferenceEvidenceView> = {}): ReferenceEvidenceView {
  return {
    referenceId: createReferenceId("docs"),
    alias,
    revision,
    path: "docs/signals.md",
    operation: "read",
    mode: "exact",
    sha256: "a".repeat(64),
    evidenceId: "ev-ref-1",
    ...overrides,
  };
}

function researchEvidence(overrides: Partial<ResearchEvidence> = {}): ResearchEvidence {
  return {
    evidenceId: "ev-research-1",
    requestId: "req_abcd",
    taskId: "task-research",
    taskContractRevision: 1,
    source: { kind: "godot-docs", id: "godot-docs", label: "Godot documentation" },
    fetchedAtMs: 1_700_000_000_000,
    resolvedRevision: "abc123",
    version: null,
    fallback: false,
    excerpt: "Signals connect objects.",
    truncated: false,
    byteLength: 24,
    ...overrides,
  };
}

function tool(name: string, capability: Capability): RegisteredToolInfo {
  return {
    definition: { name, description: `${name} tool`, inputSchema: { type: "object" } },
    capability,
  };
}

function makeTaskFixture() {
  const runtime = createTaskRuntime({ now: () => 1_700_000_000_000 });
  const snapshot = createTaskRuntimeSnapshot({
    runtimeVersion: "task-runtime-1",
    provider: null,
    sandboxProfileId: "develop-offline",
    capabilityPolicyRevision: null,
    workspaceIdentity: "/home/user/project",
    godotEngineFingerprint: null,
    workflow: null,
  });
  const contract = createTaskContract({
    id: "task-m5-proj",
    request: "Add a health component",
    acceptanceCriteria: [{ id: "c1", description: "done", verificationKind: "deterministic" }],
    pausePolicy: "none",
  });
  const handle = runtime.createTask({ contract, snapshot });
  handle.attachEvidence({
    id: "ev-latest",
    kind: "parser_result",
    source: { type: "parser", checkedFiles: 1, validFiles: 1, errors: 0 },
  });
  return { runtime, handle };
}

describe("milestone 5 tool projector gating", () => {
  const defaultProjector = createToolProjector({
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
  });

  it("review mode exposes reference tools by exact name and hides research", () => {
    const projection = defaultProjector.project({
      mode: "review",
      registeredTools: [
        tool("reference.list", "reference.inspect"),
        tool("reference.read", "reference.inspect"),
        tool("reference.search", "reference.inspect"),
        tool("research.repository", "research.fetch"),
        tool("research.godot_docs", "research.fetch"),
      ],
    });
    expect(projection.counts).toEqual({ available: 3, gated: 0, hidden: 2 });
    expect(projection.requestTools.map((t) => t.name)).toEqual([
      "reference.list",
      "reference.read",
      "reference.search",
    ]);
  });

  it("development mode hides research under the default policy (network denied)", () => {
    const projection = defaultProjector.project({
      mode: "development",
      registeredTools: [
        tool("reference.read", "reference.inspect"),
        tool("research.repository", "research.fetch"),
      ],
    });
    expect(projection.counts).toEqual({ available: 1, gated: 0, hidden: 1 });
    expect(projection.requestTools.map((t) => t.name)).toEqual(["reference.read"]);
  });

  it("development mode shows research as available when the policy allows network", () => {
    const base = createDefaultPolicy("develop-offline");
    const projector = createToolProjector({
      policy: { rules: { ...base.rules, "research.fetch": "allow" } },
      profile: DEVELOP_OFFLINE_PROFILE,
    });
    const projection = projector.project({
      mode: "development",
      registeredTools: [
        tool("research.repository", "research.fetch"),
        tool("research.godot_docs", "research.fetch"),
      ],
    });
    expect(projection.counts).toEqual({ available: 2, gated: 0, hidden: 0 });
  });

  it("inspection mode gates research under an ask policy", () => {
    const base = createDefaultPolicy("inspect");
    const projector = createToolProjector({
      policy: { rules: { ...base.rules, "research.fetch": "ask" } },
      profile: INSPECT_PROFILE,
    });
    const projection = projector.project({
      mode: "inspection",
      registeredTools: [
        tool("reference.list", "reference.inspect"),
        tool("research.repository", "research.fetch"),
      ],
    });
    expect(projection.counts).toEqual({ available: 1, gated: 1, hidden: 0 });
  });

  it("the ABI fingerprint changes only for visible changes", () => {
    // Under the default policy the research tool is hidden; its presence
    // must not change the ABI fingerprint.
    const defaultProjector = createToolProjector({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
    });
    const withHidden = defaultProjector.project({
      mode: "development",
      registeredTools: [
        tool("reference.read", "reference.inspect"),
        tool("research.repository", "research.fetch"),
      ],
    });
    const visibleOnly = defaultProjector.project({
      mode: "development",
      registeredTools: [tool("reference.read", "reference.inspect")],
    });
    expect(withHidden.counts).toEqual({ available: 1, gated: 0, hidden: 1 });
    // Both projections show only reference.read; the hidden research tool
    // must not change the ABI fingerprint.
    expect(withHidden.fingerprint).toBe(visibleOnly.fingerprint);
    // With an explicit allow policy the same tool becomes visible and the
    // fingerprint must change.
    const base = createDefaultPolicy("develop-offline");
    const allowProjector = createToolProjector({
      policy: { rules: { ...base.rules, "research.fetch": "allow" } },
      profile: DEVELOP_OFFLINE_PROFILE,
    });
    const withBothVisible = allowProjector.project({
      mode: "development",
      registeredTools: [
        tool("reference.read", "reference.inspect"),
        tool("research.repository", "research.fetch"),
      ],
    });
    expect(withBothVisible.counts.available).toBe(2);
    expect(withBothVisible.fingerprint).not.toBe(withHidden.fingerprint);
  });
});

describe("milestone 5 projection-service volatile sections", () => {
  it("renders [Reference evidence] and [Research evidence] after [Latest evidence]", () => {
    const fixture = makeTaskFixture();
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: {
        advertisedMaximum: null,
        verifiedMaximum: null,
        workingMaximum: 100_000,
        maxOutputTokens: 4_096,
      },
      getTaskSnapshot: () => fixture.runtime.getTask(fixture.handle.taskId)?.snapshot() ?? null,
      getTaskRequest: () => "Add a health component",
      references: {
        list: () => [],
        latestEvidence: () => [referenceView()],
      },
      research: {
        latestEvidence: () => [researchEvidence()],
      },
    });
    const projection = service.projectRequest({
      mode: "development",
      messages: [{ type: "user_message", content: "proceed" }],
      tools: [],
      providerToolCalling: true,
    });
    const ids = projection.contextProjection.contextualSegments.map((segment) => segment.id);
    expect(ids).toEqual(["reference-evidence", "research-evidence", "task-contract", "task-state"]);
    const referenceSegment = projection.contextProjection.contextualSegments.find(
      (segment) => segment.id === "reference-evidence",
    );
    const researchSegment = projection.contextProjection.contextualSegments.find(
      (segment) => segment.id === "research-evidence",
    );
    expect(referenceSegment?.content).toContain(
      "@reference/docs @ a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0 docs/signals.md (read, exact)",
    );
    expect(researchSegment?.content).toContain("Source: Godot documentation");
    expect(researchSegment?.content).toContain("Excerpt: Signals connect objects.");
    // Never absolute cache paths, never the internal registry surface.
    expect(referenceSegment?.content).not.toContain("C:\\");
    expect(referenceSegment?.content).not.toContain("/srv/");
    expect(researchSegment?.content).not.toContain("C:\\");
  });

  it("omits the sections when no observations exist", () => {
    const fixture = makeTaskFixture();
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: {
        advertisedMaximum: null,
        verifiedMaximum: null,
        workingMaximum: 100_000,
        maxOutputTokens: 4_096,
      },
      getTaskSnapshot: () => fixture.runtime.getTask(fixture.handle.taskId)?.snapshot() ?? null,
      getTaskRequest: () => "Add a health component",
      references: { list: () => [], latestEvidence: () => [] },
      research: { latestEvidence: () => [] },
    });
    const projection = service.projectRequest({
      mode: "development",
      messages: [{ type: "user_message", content: "proceed" }],
      tools: [],
      providerToolCalling: true,
    });
    const ids = projection.contextProjection.contextualSegments.map((segment) => segment.id);
    // The reference/research sections are omitted when no observations exist.
    expect(ids).not.toContain("reference-evidence");
    expect(ids).not.toContain("research-evidence");
  });

  it("bounds the combined volatile sections to 12 KiB with an explicit truncation marker", () => {
    const fixture = makeTaskFixture();
    const views = Array.from({ length: 4 }, (_, index) =>
      referenceView({
        alias: `ref${index}` as ReferenceAlias,
        path: `docs/file-${index}.md`,
        evidenceId: `ev-ref-${index}`,
      }),
    );
    const entries = Array.from({ length: 4 }, (_, index) =>
      researchEvidence({
        evidenceId: `ev-research-${index}`,
        excerpt: `e`.repeat(4_000) + `-${index}`,
        byteLength: 4_000,
      }),
    );
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: {
        advertisedMaximum: null,
        verifiedMaximum: null,
        workingMaximum: 100_000,
        maxOutputTokens: 4_096,
      },
      getTaskSnapshot: () => fixture.runtime.getTask(fixture.handle.taskId)?.snapshot() ?? null,
      getTaskRequest: () => "Add a health component",
      references: { list: () => [], latestEvidence: () => views },
      research: { latestEvidence: () => entries },
    });
    const projection = service.projectRequest({
      mode: "development",
      messages: [{ type: "user_message", content: "proceed" }],
      tools: [],
      providerToolCalling: true,
    });
    const encoder = new TextEncoder();
    const total = projection.contextProjection.contextualSegments
      .filter(
        (segment) => segment.id.startsWith("reference-") || segment.id.startsWith("research-"),
      )
      .reduce((sum, segment) => sum + encoder.encode(segment.content).length, 0);
    expect(total).toBeLessThanOrEqual(12 * 1024);
    const researchSegment = projection.contextProjection.contextualSegments.find(
      (segment) => segment.id === "research-evidence",
    );
    expect(researchSegment?.content).toContain("[truncated]");
  });

  it("keeps the stable fingerprint unchanged when only reference/research evidence changes", () => {
    const fixture = makeTaskFixture();
    function project(views: readonly ReferenceEvidenceView[]) {
      const service = createProjectionService({
        policy: createDefaultPolicy("develop-offline"),
        profile: DEVELOP_OFFLINE_PROFILE,
        capacity: {
          advertisedMaximum: null,
          verifiedMaximum: null,
          workingMaximum: 100_000,
          maxOutputTokens: 4_096,
        },
        getTaskSnapshot: () => fixture.runtime.getTask(fixture.handle.taskId)?.snapshot() ?? null,
        getTaskRequest: () => "Add a health component",
        references: { list: () => [], latestEvidence: () => views },
        research: { latestEvidence: () => [researchEvidence()] },
      });
      return service.projectRequest({
        mode: "development",
        messages: [{ type: "user_message", content: "proceed" }],
        tools: [],
        providerToolCalling: true,
      });
    }
    const first = project([referenceView()]);
    const second = project([referenceView({ path: "docs/other.md" })]);
    expect(second.contextProjection.stableFingerprint).toBe(
      first.contextProjection.stableFingerprint,
    );
    expect(second.system?.slice(0, second.contextProjection.stableBytes)).toBe(
      first.system?.slice(0, first.contextProjection.stableBytes),
    );
  });

  it("derives the reference anchor from the revision identity (commit vs fingerprint)", () => {
    const fixture = makeTaskFixture();
    const service = createProjectionService({
      policy: createDefaultPolicy("develop-offline"),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: {
        advertisedMaximum: null,
        verifiedMaximum: null,
        workingMaximum: 100_000,
        maxOutputTokens: 4_096,
      },
      getTaskSnapshot: () => fixture.runtime.getTask(fixture.handle.taskId)?.snapshot() ?? null,
      getTaskRequest: () => "Add a health component",
      references: {
        list: () => [],
        latestEvidence: () => [
          referenceView({
            revision: {
              identity: {
                kind: "local-directory",
                canonicalPath: "/srv/assets",
                fingerprint: "fp-local-9",
              },
              resolvedAtMs: 1_700_000_000_000,
            },
            path: "icons/coin.svg",
            operation: "list",
            mode: null,
            sha256: null,
          }),
        ],
      },
    });
    const projection = service.projectRequest({
      mode: "development",
      messages: [{ type: "user_message", content: "proceed" }],
      tools: [],
      providerToolCalling: true,
    });
    const referenceSegment = projection.contextProjection.contextualSegments.find(
      (segment) => segment.id === "reference-evidence",
    );
    expect(referenceSegment?.content).toContain(
      "@reference/docs @ fp-local-9 icons/coin.svg (list)",
    );
    // The canonical path itself must never surface.
    expect(referenceSegment?.content).not.toContain("/srv/assets");
  });

  it("reference tool visibility via createReferenceId stays deterministic", () => {
    expect(createReferenceId(alias).startsWith("ref_")).toBe(true);
  });
});
