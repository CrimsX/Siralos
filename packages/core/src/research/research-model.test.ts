import { describe, expect, it } from "vitest";
import {
  RESEARCH_LIMITS,
  computeResearchDocumentId,
  defaultResearchBounds,
  validateResearchRequest,
  type ResearchRequest,
} from "./research-model.js";

const validRequest: ResearchRequest = {
  source: { kind: "godot-docs", id: "godot-docs", label: "Godot documentation" },
  query: "how do signals work",
  topic: null,
  path: null,
  ref: null,
  version: null,
  maxBytes: null,
};

describe("research request validation", () => {
  it("accepts a well-formed request", () => {
    const result = validateResearchRequest(validRequest);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.request.query).toBe(validRequest.query);
      expect(result.request.source.kind).toBe("godot-docs");
    }
  });

  it("rejects missing or empty queries", () => {
    expect(validateResearchRequest({ ...validRequest, query: "" }).ok).toBe(false);
    expect(validateResearchRequest({ ...validRequest, query: "   " }).ok).toBe(false);
    const missing = { ...validRequest } as Record<string, unknown>;
    delete missing.query;
    expect(validateResearchRequest(missing).ok).toBe(false);
  });

  it("bounds the query to 512 bytes", () => {
    expect(validateResearchRequest({ ...validRequest, query: "q".repeat(512) }).ok).toBe(true);
    expect(validateResearchRequest({ ...validRequest, query: "q".repeat(513) }).ok).toBe(false);
  });

  it("bounds topic to 256 bytes", () => {
    expect(validateResearchRequest({ ...validRequest, topic: "t".repeat(256) }).ok).toBe(true);
    expect(validateResearchRequest({ ...validRequest, topic: "t".repeat(257) }).ok).toBe(false);
  });

  it("rejects path traversal and absolute paths", () => {
    expect(validateResearchRequest({ ...validRequest, path: "../secret" }).ok).toBe(false);
    expect(validateResearchRequest({ ...validRequest, path: "a/../../b" }).ok).toBe(false);
    expect(validateResearchRequest({ ...validRequest, path: "/etc/passwd" }).ok).toBe(false);
    expect(validateResearchRequest({ ...validRequest, path: "C:\\windows\\x" }).ok).toBe(false);
    expect(validateResearchRequest({ ...validRequest, path: "docs/intro.md" }).ok).toBe(true);
    expect(validateResearchRequest({ ...validRequest, path: "a/./b" }).ok).toBe(false);
    expect(validateResearchRequest({ ...validRequest, path: "x".repeat(1024) }).ok).toBe(true);
    expect(validateResearchRequest({ ...validRequest, path: "x".repeat(1025) }).ok).toBe(false);
  });

  it("bounds ref to 256 characters", () => {
    expect(validateResearchRequest({ ...validRequest, ref: "r".repeat(256) }).ok).toBe(true);
    expect(validateResearchRequest({ ...validRequest, ref: "r".repeat(257) }).ok).toBe(false);
  });

  it("validates the version pattern", () => {
    for (const version of ["4.3", "4", "4.3.2", "4.3-stable", "4.3.1.2", "3.x", "1.2.3-beta.1"]) {
      expect(validateResearchRequest({ ...validRequest, version }).ok, version).toBe(true);
    }
    for (const version of ["", "4.", ".4", "v4.3", "4.3.2.1.0", "4 3", "4.3-", "4.3 beta"]) {
      expect(validateResearchRequest({ ...validRequest, version }).ok, version).toBe(false);
    }
    expect(
      validateResearchRequest({ ...validRequest, version: "4.3-stable-extra-".repeat(6) }).ok,
    ).toBe(false);
  });

  it("rejects invalid sources and maxBytes", () => {
    expect(
      validateResearchRequest({ ...validRequest, source: { kind: "unknown", id: "x", label: "x" } })
        .ok,
    ).toBe(false);
    expect(
      validateResearchRequest({
        ...validRequest,
        source: { kind: "godot-docs", id: "", label: "x" },
      }).ok,
    ).toBe(false);
    expect(
      validateResearchRequest({
        ...validRequest,
        source: { kind: "godot-docs", id: "x".repeat(129), label: "x" },
      }).ok,
    ).toBe(false);
    expect(validateResearchRequest({ ...validRequest, maxBytes: -1 }).ok).toBe(false);
    expect(validateResearchRequest({ ...validRequest, maxBytes: 0 }).ok).toBe(false);
    expect(validateResearchRequest({ ...validRequest, maxBytes: 10_000 }).ok).toBe(true);
  });

  it("rejects non-object input and wrong-typed fields", () => {
    expect(validateResearchRequest(null).ok).toBe(false);
    expect(validateResearchRequest("query").ok).toBe(false);
    expect(validateResearchRequest({ ...validRequest, topic: 42 }).ok).toBe(false);
    expect(validateResearchRequest({ ...validRequest, ref: ["x"] }).ok).toBe(false);
  });
});

describe("research bounds", () => {
  it("defaults match RESEARCH_LIMITS", () => {
    expect(defaultResearchBounds()).toEqual({
      maxDownloadBytes: RESEARCH_LIMITS.maxDownloadBytes,
      maxDocumentBytes: RESEARCH_LIMITS.maxDocumentBytes,
      maxSections: RESEARCH_LIMITS.maxSections,
      maxLinks: RESEARCH_LIMITS.maxLinks,
      maxHeadingBytes: RESEARCH_LIMITS.maxHeadingBytes,
      maxSectionTextBytes: RESEARCH_LIMITS.maxSectionTextBytes,
      maxRedirects: RESEARCH_LIMITS.maxRedirects,
      timeoutMs: RESEARCH_LIMITS.timeoutMs,
      hardLifetimeMs: RESEARCH_LIMITS.hardLifetimeMs,
    });
  });
});

describe("research document id", () => {
  it("is deterministic and bounded", () => {
    const id = computeResearchDocumentId("godot-docs", "digest-a");
    expect(id.startsWith("rd_")).toBe(true);
    expect(id.length).toBe("rd_".length + 24);
    expect(computeResearchDocumentId("godot-docs", "digest-a")).toBe(id);
    expect(computeResearchDocumentId("godot-docs", "digest-b")).not.toBe(id);
  });
});
