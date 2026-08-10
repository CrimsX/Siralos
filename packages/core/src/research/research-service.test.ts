import { describe, expect, it } from "vitest";
import type { CapabilityPolicy } from "../security/capability.js";
import { createDefaultPolicy } from "../security/default-policy.js";
import { INSPECT_PROFILE } from "../security/profile.js";
import {
  computeResearchDocumentId,
  type ResearchDocument,
  type ResearchOutcome,
  type ResearchRequest,
  type ResearchSourceKind,
} from "./research-model.js";
import type { ResearchSourcePort } from "./research-ports.js";
import {
  createResearchService,
  formatResearchEvidenceView,
  type ResearchEvidence,
  type ResearchService,
  type ResearchServiceOptions,
} from "./research-service.js";

const request: ResearchRequest = {
  source: { kind: "fake", id: "fake-source", label: "Fake source" },
  query: "signals",
  topic: null,
  path: "docs/signals.md",
  ref: null,
  version: null,
  maxBytes: null,
};

const TEST_TASK = { taskId: "task-research-test", taskContractRevision: 1 } as const;

function makeDocument(overrides: Partial<ResearchDocument> = {}): ResearchDocument {
  const source = { kind: "fake" as const, id: "fake-source", label: "Fake source" };
  return {
    id: computeResearchDocumentId("fake-source", "digest"),
    source,
    title: "Fake doc",
    fetchedAtMs: 1_700_000_000_000,
    contentType: "text/markdown",
    sections: [{ heading: "Intro", text: "Signals connect objects.", byteLength: 24 }],
    links: [],
    provenance: {
      source,
      requestedRef: null,
      resolvedRevision: "abc123",
      requestedVersion: null,
      usedVersion: null,
      fallback: false,
      fallbackReason: null,
      fetchedAtMs: 1_700_000_000_000,
      resource: "docs/signals.md",
    },
    truncated: false,
    truncationReason: null,
    byteLength: 24,
    ...overrides,
  };
}

function fakeSource(
  handler: (
    req: ResearchRequest,
    bounds: ResearchServiceOptions["bounds"],
    signal: AbortSignal,
  ) => ResearchOutcome | Promise<ResearchOutcome>,
  kind: ResearchSourceKind = "fake",
  id = "fake-source",
  label = "Fake source",
): ResearchSourcePort & { readonly calls: () => number } {
  let count = 0;
  return {
    kind,
    id,
    label,
    async fetch(req, bounds, signal): Promise<ResearchOutcome> {
      count += 1;
      return Promise.resolve(handler(req, bounds, signal));
    },
    calls: () => count,
  };
}

function policyWith(network: "allow" | "ask" | "deny"): CapabilityPolicy {
  const base = createDefaultPolicy("inspect");
  return { rules: { ...base.rules, "research.fetch": network } };
}

function makeService(
  sources: readonly ResearchSourcePort[],
  overrides: Partial<Omit<ResearchServiceOptions, "policy" | "profile" | "sources">> = {},
): ResearchService {
  return createResearchService({
    policy: policyWith("allow"),
    profile: INSPECT_PROFILE,
    sources,
    currentTask: () => TEST_TASK,
    ...overrides,
  });
}

describe("research service policy gate", () => {
  it("deny policy refuses and NEVER invokes the source port", async () => {
    const source = fakeSource(() => ({ status: "document", document: makeDocument() }));
    const service = createResearchService({
      policy: policyWith("deny"),
      profile: INSPECT_PROFILE,
      sources: [source],
      currentTask: () => TEST_TASK,
    });
    const result = await service.fetch(request);
    expect(result.status).toBe("refused");
    if (result.status === "refused") {
      expect(result.reason).toBe("network policy denies research");
    }
    expect(source.calls()).toBe(0);
  });

  it("ask policy refuses (no approval protocol exists for research)", async () => {
    const source = fakeSource(() => ({ status: "document", document: makeDocument() }));
    const service = createResearchService({
      policy: policyWith("ask"),
      profile: INSPECT_PROFILE,
      sources: [source],
      currentTask: () => TEST_TASK,
    });
    const result = await service.fetch(request);
    expect(result.status).toBe("refused");
    if (result.status === "refused") {
      expect(result.reason).toBe("research requires explicit network permission");
    }
    expect(source.calls()).toBe(0);
  });

  it("the policy gate runs BEFORE request validation", async () => {
    const source = fakeSource(() => ({ status: "document", document: makeDocument() }));
    const service = createResearchService({
      policy: policyWith("deny"),
      profile: INSPECT_PROFILE,
      sources: [source],
      currentTask: () => TEST_TASK,
    });
    const result = await service.fetch({ ...request, query: "" });
    expect(result.status).toBe("refused");
    expect(source.calls()).toBe(0);
  });
});

describe("research service fetch", () => {
  it("returns a document with bounded evidence when allowed", async () => {
    const source = fakeSource(() => ({ status: "document", document: makeDocument() }));
    const service = makeService([source]);
    const result = await service.fetch(request);
    expect(result.status).toBe("document");
    if (result.status === "document") {
      expect(result.document.title).toBe("Fake doc");
      expect(result.evidence.evidenceId).toMatch(/^ev-research-\d+$/);
      expect(result.evidence.requestId).toMatch(/^req_[0-9a-f]{24}$/);
      expect(result.evidence.excerpt).toBe("Signals connect objects.");
      expect(result.evidence.resolvedRevision).toBe("abc123");
      expect(result.evidence.truncated).toBe(false);
      expect(result.evidence.source.id).toBe("fake-source");
    }
    expect(source.calls()).toBe(1);
  });

  it("refuses unknown sources without invoking any port", async () => {
    const source = fakeSource(
      () => ({ status: "document", document: makeDocument() }),
      "fake",
      "other",
      "Other label",
    );
    const service = makeService([source]);
    const result = await service.fetch(request);
    expect(result.status).toBe("refused");
    if (result.status === "refused") {
      expect(result.reason).toContain("not configured");
    }
    expect(source.calls()).toBe(0);
  });

  it("refuses before source invocation when no active task can bind the result", async () => {
    const source = fakeSource(() => ({ status: "document", document: makeDocument() }));
    const service = makeService([source], { currentTask: () => null });

    const result = await service.fetch(request);

    expect(result.status).toBe("refused");
    expect(source.calls()).toBe(0);
  });

  it("matches a source by label but canonicalizes its configured identity", async () => {
    let observedSource: ResearchRequest["source"] | null = null;
    const source = fakeSource(
      (received) => {
        observedSource = received.source;
        return {
          status: "document",
          document: makeDocument({ source: received.source }),
        };
      },
      "fake",
      "other-id",
    );
    const service = makeService([source]);
    // The request names an id that is not configured but a label that is.
    const result = await service.fetch({
      ...request,
      source: { kind: "fake", id: "unknown-id", label: "Fake source" },
    });
    expect(result.status).toBe("document");
    expect(source.calls()).toBe(1);
    expect(observedSource).toEqual({ kind: "fake", id: "other-id", label: "Fake source" });
    if (result.status === "document") {
      expect(result.document.source.id).toBe("other-id");
    }
  });

  it("passes source outcomes through (unsupported-content, oversized, failed)", async () => {
    const unsupported = makeService([
      fakeSource(() => ({ status: "unsupported-content", reason: "binary content" })),
    ]);
    const unsupportedResult = await unsupported.fetch(request);
    expect(unsupportedResult).toEqual({ status: "unsupported-content", reason: "binary content" });
    const oversized = makeService([fakeSource(() => ({ status: "oversized", reason: "too big" }))]);
    expect(await oversized.fetch(request)).toEqual({ status: "oversized", reason: "too big" });
    const failed = makeService([fakeSource(() => ({ status: "failed", reason: "boom" }))]);
    expect(await failed.fetch(request)).toEqual({ status: "failed", reason: "boom" });
  });

  it("maps a throwing source to a failed outcome (provider output is untrusted)", async () => {
    const source = fakeSource(() => {
      throw new Error("adapter exploded");
    });
    const service = makeService([source]);
    const result = await service.fetch(request);
    expect(result.status).toBe("failed");
    if (result.status === "failed") {
      expect(result.reason).toContain("adapter exploded");
    }
  });

  it("invalid requests fail closed with a precise reason", async () => {
    const source = fakeSource(() => ({ status: "document", document: makeDocument() }));
    const service = makeService([source]);
    const result = await service.fetch({ ...request, path: "../escape" });
    expect(result.status).toBe("failed");
    if (result.status === "failed") {
      expect(result.reason).toContain("invalid research request");
    }
    expect(source.calls()).toBe(0);
  });

  it("times out without waiting indefinitely", async () => {
    let sourceSignalAborted = false;
    const source = fakeSource(
      (_request, _bounds, signal) =>
        new Promise<ResearchOutcome>(() => {
          signal.addEventListener(
            "abort",
            () => {
              sourceSignalAborted = true;
            },
            { once: true },
          );
        }),
    );
    const service = makeService([source], {
      bounds: { ...defaultBounds(), timeoutMs: 20 },
    });
    const started = Date.now();
    const result = await service.fetch(request);
    expect(result.status).toBe("timeout");
    expect(Date.now() - started).toBeLessThan(2_000);
    expect(sourceSignalAborted).toBe(true);
  });

  it("aborts to cancelled (pre-aborted call never invokes the source)", async () => {
    const source = fakeSource(() => ({ status: "document", document: makeDocument() }));
    const service = makeService([source]);
    const controller = new AbortController();
    controller.abort();
    const result = await service.fetch(request, { signal: controller.signal });
    expect(result.status).toBe("cancelled");
    expect(source.calls()).toBe(0);
  });

  it("aborts to cancelled mid-flight", async () => {
    const source = fakeSource(
      (_req, _bounds, signal) =>
        new Promise<ResearchOutcome>((resolve) => {
          signal.addEventListener("abort", () => resolve({ status: "failed", reason: "aborted" }), {
            once: true,
          });
        }),
    );
    const service = makeService([source]);
    const controller = new AbortController();
    const pending = service.fetch(request, { signal: controller.signal });
    setTimeout(() => controller.abort(), 5);
    const result = await pending;
    expect(result.status).toBe("cancelled");
    expect(source.calls()).toBe(1);
  });

  it("truncates oversized excerpts with an explicit flag", async () => {
    const huge = "x".repeat(10_000);
    const source = fakeSource(() => ({
      status: "document",
      document: makeDocument({
        sections: [{ heading: null, text: huge, byteLength: huge.length }],
      }),
    }));
    const service = makeService([source]);
    const result = await service.fetch(request);
    expect(result.status).toBe("document");
    if (result.status === "document") {
      expect(result.evidence.truncated).toBe(true);
      expect(new TextEncoder().encode(result.evidence.excerpt).length).toBeLessThanOrEqual(4096);
      expect(result.evidence.excerpt).toContain("[truncated]");
    }
  });
});

describe("research service evidence ring", () => {
  it("retains at most maxRetainedEvidenceViews entries, FIFO", async () => {
    const source = fakeSource(() => ({
      status: "document",
      document: makeDocument({ sections: [{ heading: null, text: "entry", byteLength: 5 }] }),
    }));
    const service = makeService([source]);
    for (let index = 0; index < 10; index += 1) {
      await service.fetch(request);
    }
    const latest = service.latestEvidence();
    expect(latest).toHaveLength(8);
    expect(latest[0]?.evidenceId).toBe("ev-research-3"); // oldest retained (1-2 evicted)
    expect(latest[7]?.evidenceId).toBe("ev-research-10");
  });

  it("bounds the retained ring by maxEvidenceBytes", async () => {
    const source = fakeSource(() => ({
      status: "document",
      document: makeDocument({
        sections: [{ heading: null, text: "e".repeat(100), byteLength: 100 }],
      }),
    }));
    const service = makeService([source], { maxEvidenceBytes: 250 });
    for (let index = 0; index < 10; index += 1) {
      await service.fetch(request);
    }
    const latest = service.latestEvidence();
    const total = latest.reduce((sum, entry) => sum + entry.byteLength, 0);
    expect(total).toBeLessThanOrEqual(250);
    expect(latest.length).toBeGreaterThan(0);
  });

  it("reports active request counts and source kinds", async () => {
    const source = fakeSource(() => new Promise<ResearchOutcome>(() => {}), "fake", "a");
    const other = fakeSource(
      () => ({ status: "document", document: makeDocument() }),
      "godot-docs",
      "b",
    );
    const service = makeService([source, other], { bounds: { ...defaultBounds(), timeoutMs: 20 } });
    expect(service.sourceKinds()).toEqual(["fake", "godot-docs"]);
    const pending = service.fetch({
      ...request,
      source: { kind: "fake", id: "a", label: "Fake source A" },
    });
    expect(service.activeRequestCount()).toBe(1);
    await pending;
    expect(service.activeRequestCount()).toBe(0);
  });
});

describe("stale-result binding", () => {
  it("discards an in-flight result when the active task revision changes", async () => {
    let current = { taskId: "task-research", taskContractRevision: 1 };
    let resolveOutcome!: (outcome: ResearchOutcome) => void;
    const source = fakeSource(
      () =>
        new Promise<ResearchOutcome>((resolve) => {
          resolveOutcome = resolve;
        }),
    );
    const service = makeService([source], { currentTask: () => current });
    const pending = service.fetch(request);
    await Promise.resolve();
    current = { taskId: "task-research", taskContractRevision: 2 };
    resolveOutcome({ status: "document", document: makeDocument() });

    const result = await pending;

    expect(result.status).toBe("stale");
    expect(service.latestEvidence()).toEqual([]);
  });

  it("detects an in-place mutation of the active task binding object", async () => {
    const current = { taskId: "task-research", taskContractRevision: 1 };
    let resolveOutcome!: (outcome: ResearchOutcome) => void;
    const source = fakeSource(
      () =>
        new Promise<ResearchOutcome>((resolve) => {
          resolveOutcome = resolve;
        }),
    );
    const service = makeService([source], { currentTask: () => current });
    const pending = service.fetch(request);
    await Promise.resolve();
    current.taskContractRevision = 2;
    resolveOutcome({ status: "document", document: makeDocument() });

    expect((await pending).status).toBe("stale");
    expect(service.latestEvidence()).toEqual([]);
  });

  it("binds retained evidence to the exact active task identity", async () => {
    const source = fakeSource(() => ({ status: "document", document: makeDocument() }));
    const service = makeService([source]);
    const result = await service.fetch(request);

    expect(result.status).toBe("document");
    if (result.status === "document") {
      expect(result.evidence.taskId).toBe(TEST_TASK.taskId);
      expect(result.evidence.taskContractRevision).toBe(TEST_TASK.taskContractRevision);
    }
  });

  it("mints unique request ids for identical requests", async () => {
    const source = fakeSource(() => ({ status: "document", document: makeDocument() }));
    const service = makeService([source]);
    const first = await service.fetch(request);
    const second = await service.fetch(request);

    expect(first.status).toBe("document");
    expect(second.status).toBe("document");
    if (first.status === "document" && second.status === "document") {
      expect(first.evidence.requestId).not.toBe(second.evidence.requestId);
    }
  });
});

describe("formatResearchEvidenceView", () => {
  const evidence: ResearchEvidence = {
    evidenceId: "ev-research-1",
    requestId: "req_abcd",
    taskId: "task-research-test",
    taskContractRevision: 1,
    source: { kind: "fake", id: "fake-source", label: "Fake source" },
    fetchedAtMs: 1_700_000_000_000,
    resolvedRevision: "abc123",
    version: "4.3",
    fallback: true,
    excerpt: "Signals connect objects.",
    truncated: false,
    byteLength: 24,
  };

  it("renders the exact view shape", () => {
    expect(formatResearchEvidenceView(evidence)).toBe(
      [
        "Source: Fake source",
        "Request: req_abcd",
        "Fetched: 2023-11-14T22:13:20.000Z",
        "Revision: abc123",
        "Version: 4.3 (fallback)",
        "Excerpt: Signals connect objects.",
        "Evidence: ev-research-1",
      ].join("\n"),
    );
  });

  it("renders unknown revision/version and no fallback suffix", () => {
    const minimal = { ...evidence, resolvedRevision: null, version: null, fallback: false };
    const text = formatResearchEvidenceView(minimal);
    expect(text).toContain("Revision: unknown");
    expect(text).toContain("Version: unknown");
    expect(text).not.toContain("(fallback)");
  });

  it("bounds the view with an explicit truncation marker", () => {
    const text = formatResearchEvidenceView(evidence, { maxBytes: 60 });
    expect(new TextEncoder().encode(text).length).toBeLessThanOrEqual(60);
    expect(text).toContain("[truncated]");
  });
});

function defaultBounds() {
  return {
    maxDownloadBytes: 2 * 1024 * 1024,
    maxDocumentBytes: 256 * 1024,
    maxSections: 64,
    maxLinks: 32,
    maxHeadingBytes: 512,
    maxSectionTextBytes: 32 * 1024,
    maxRedirects: 4,
    timeoutMs: 10_000,
    hardLifetimeMs: 30_000,
  };
}
