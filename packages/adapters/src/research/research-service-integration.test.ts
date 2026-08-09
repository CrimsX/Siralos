import { describe, expect, it } from "vitest";
import {
  INSPECT_PROFILE,
  createDefaultPolicy,
  createResearchService,
  type CapabilityPolicy,
  type ResearchRequest,
  type ResearchService,
} from "@solaris/core";
import {
  createFakeGodotDocsSource,
  createFakeRepositorySource,
  type FakeRepositoryResearchFixture,
  type GodotDocsFixture,
} from "./fake-sources.js";

/**
 * Service-level integration: the deterministic fake sources work through the
 * core `createResearchService` exactly like any configured research source
 * (policy gate, request validation, evidence building). Network-free.
 */

const GODOT_DOCS_SOURCE = {
  kind: "godot-docs" as const,
  id: "godot-docs-fake",
  label: "Fake Godot docs",
};

const GITHUB_SOURCE = {
  kind: "repository" as const,
  id: "github-fake",
  label: "Fake GitHub repository research",
};

const DOCS_FIXTURE: GodotDocsFixture = {
  versions: {
    stable: {
      class_node: {
        title: "Node",
        sections: [{ heading: "Node", text: "Base class for the scene tree." }],
      },
    },
  },
};

const REPO_FIXTURE: FakeRepositoryResearchFixture = {
  "godotengine/godot": {
    releases: { "4.3": { body: "# v4.3" } },
    files: {
      HEAD: { "README.md": { contentType: "text/markdown", body: "# Godot\nEngine." } },
    },
  },
};

function allowPolicy(): CapabilityPolicy {
  const base = createDefaultPolicy("inspect");
  return { rules: { ...base.rules, "research.fetch": "allow" } };
}

function makeService(): ResearchService {
  return createResearchService({
    policy: allowPolicy(),
    profile: INSPECT_PROFILE,
    sources: [createFakeGodotDocsSource(DOCS_FIXTURE), createFakeRepositorySource(REPO_FIXTURE)],
  });
}

function request(overrides: Partial<ResearchRequest>): ResearchRequest {
  return {
    source: GODOT_DOCS_SOURCE,
    query: "node",
    topic: null,
    path: null,
    ref: null,
    version: null,
    maxBytes: null,
    ...overrides,
  };
}

describe("research service integration with fake sources", () => {
  it("fetches a fake Godot docs page through the service with evidence", async () => {
    const service = makeService();
    const result = await service.fetch(request({ topic: "class_node", query: "node" }));
    expect(result.status).toBe("document");
    if (result.status === "document") {
      expect(result.document.sections[0]?.heading).toBe("Node");
      expect(result.evidence.source).toEqual(GODOT_DOCS_SOURCE);
      expect(result.evidence.excerpt).toContain("Base class");
      expect(service.latestEvidence()).toHaveLength(1);
    }
  });

  it("fetches a fake repository file through the service", async () => {
    const service = makeService();
    const result = await service.fetch(
      request({ source: GITHUB_SOURCE, query: "godotengine/godot", path: "README.md" }),
    );
    expect(result.status).toBe("document");
    if (result.status === "document") {
      expect(result.document.sections[0]?.heading).toBe("Godot");
      expect(result.document.provenance.resource).toBe("files:HEAD:README.md");
    }
  });

  it("refuses when the policy denies research.fetch without invoking sources", async () => {
    const base = createDefaultPolicy("inspect");
    const service = createResearchService({
      policy: { rules: { ...base.rules, "research.fetch": "deny" } },
      profile: INSPECT_PROFILE,
      sources: [createFakeGodotDocsSource(DOCS_FIXTURE)],
    });
    const result = await service.fetch(request({ topic: "class_node" }));
    expect(result.status).toBe("refused");
    if (result.status === "refused") {
      expect(result.reason).toBe("network policy denies research");
    }
    expect(service.latestEvidence()).toHaveLength(0);
  });

  it("refuses unconfigured sources", async () => {
    const service = makeService();
    const result = await service.fetch(
      request({ source: { kind: "fake", id: "nope", label: "Nope" }, query: "x" }),
    );
    expect(result.status).toBe("refused");
    if (result.status === "refused") {
      expect(result.reason).toContain("Unknown research source");
    }
  });

  it("reports configured source kinds", () => {
    const service = makeService();
    expect(service.sourceKinds()).toEqual(["godot-docs", "repository"]);
  });
});
