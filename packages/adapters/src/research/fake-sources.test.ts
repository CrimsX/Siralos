import { describe, expect, it } from "vitest";
import {
  defaultResearchBounds,
  type ResearchBounds,
  type ResearchRequest,
  type ResearchSourceRef,
} from "@siralos/core";
import {
  createFakeGodotDocsSource,
  createFakeRepositorySource,
  type FakeRepositoryResearchFixture,
  type GodotDocsFixture,
} from "./fake-sources.js";
import { TRUNCATION_MARKER } from "./normalization.js";

const NOW = 1_700_000_000_000;

function bounds(overrides: Partial<ResearchBounds> = {}): ResearchBounds {
  return { ...defaultResearchBounds(), ...overrides };
}

describe("createFakeGodotDocsSource", () => {
  const sourceRef: ResearchSourceRef = {
    kind: "godot-docs",
    id: "godot-docs-fake",
    label: "Fake Godot docs",
  };

  const fixture: GodotDocsFixture = {
    versions: {
      "4.7": {
        class_node: {
          title: "Node",
          sections: [
            { heading: "Node", text: "Base class for the scene tree." },
            { heading: "Properties", text: "Many." },
          ],
        },
      },
    },
    fallbacks: {
      "4.7.1": { usedVersion: "4.7", reason: "patch version not published; using minor docs" },
    },
  };

  function request(overrides: Partial<ResearchRequest> = {}): ResearchRequest {
    return {
      source: sourceRef,
      query: "node",
      topic: "Node",
      path: null,
      ref: null,
      version: null,
      maxBytes: null,
      ...overrides,
    };
  }

  it("serves version-matched topics with fixture title and provenance", async () => {
    const source = createFakeGodotDocsSource(fixture, { now: () => NOW });
    const outcome = await source.fetch(
      request({ version: "4.7", topic: "class_node" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.title).toBe("Node");
      expect(outcome.document.sections[0]?.heading).toBe("Node");
      expect(outcome.document.provenance).toMatchObject({
        source: sourceRef,
        requestedVersion: "4.7",
        usedVersion: "4.7",
        fallback: false,
        fallbackReason: null,
        requestedRef: null,
        resolvedRevision: null,
        fetchedAtMs: NOW,
        resource: "docs:4.7:class_node",
      });
    }
  });

  it("falls back through the chain with explicit fallback marking", async () => {
    const source = createFakeGodotDocsSource(fixture, { now: () => NOW });
    const outcome = await source.fetch(
      request({ version: "4.7.1", topic: "class_node" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.provenance).toMatchObject({
        requestedVersion: "4.7.1",
        usedVersion: "4.7",
        fallback: true,
        fallbackReason: "patch version not published; using minor docs",
      });
    }
  });

  it("is deterministic: identical requests produce identical document ids", async () => {
    const source = createFakeGodotDocsSource(fixture, { now: () => NOW });
    const first = await source.fetch(
      request({ topic: "class_node", version: "4.7" }),
      bounds(),
      new AbortController().signal,
    );
    const second = await source.fetch(
      request({ topic: "class_node", version: "4.7" }),
      bounds(),
      new AbortController().signal,
    );
    expect(first.status).toBe("document");
    expect(second.status).toBe("document");
    if (first.status === "document" && second.status === "document") {
      expect(second.document.id).toBe(first.document.id);
    }
  });

  it("fails with not found for unknown topics and versions", async () => {
    const source = createFakeGodotDocsSource(fixture);
    const unknownTopic = await source.fetch(
      request({ topic: "Nope" }),
      bounds(),
      new AbortController().signal,
    );
    expect(unknownTopic.status).toBe("failed");
    if (unknownTopic.status === "failed") {
      expect(unknownTopic.reason).toBe("not found");
    }
    const unknownVersion = await source.fetch(
      request({ topic: "class_node", version: "3.5" }),
      bounds(),
      new AbortController().signal,
    );
    expect(unknownVersion.status).toBe("failed");
  });

  it("returns cancelled for an already-aborted signal", async () => {
    const controller = new AbortController();
    controller.abort();
    const source = createFakeGodotDocsSource(fixture);
    const outcome = await source.fetch(request(), bounds(), controller.signal);
    expect(outcome.status).toBe("cancelled");
  });

  it("enforces bounds through the shared document builder", async () => {
    const source = createFakeGodotDocsSource(fixture);
    const outcome = await source.fetch(
      request({ topic: "class_node", version: "4.7" }),
      bounds({ maxSections: 1 }),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.truncated).toBe(true);
      expect(outcome.document.sections).toHaveLength(1);
      expect(outcome.document.sections[0]?.text.endsWith(TRUNCATION_MARKER)).toBe(true);
    }
  });
});

describe("createFakeRepositorySource", () => {
  const sourceRef: ResearchSourceRef = {
    kind: "repository",
    id: "github-fake",
    label: "Fake GitHub repository research",
  };

  const COMMIT = "b".repeat(40);

  const fixture: FakeRepositoryResearchFixture = {
    "godotengine/godot": {
      releases: {
        "4.0": { body: "# v4.0" },
        "4.9": { body: "# v4.9" },
        "4.10": { body: "# v4.10" },
      },
      files: {
        HEAD: { "README.md": { contentType: "text/markdown", body: "# Godot\nEngine." } },
        [COMMIT]: { "README.md": { contentType: "text/markdown", body: "# At commit" } },
        main: { "README.md": { contentType: "text/plain", body: "plain body" } },
        bin: { "logo.png": { contentType: "image/png", body: "not an image" } },
      },
    },
  };

  function request(overrides: Partial<ResearchRequest> = {}): ResearchRequest {
    return {
      source: sourceRef,
      query: "godotengine/godot",
      topic: null,
      path: null,
      ref: null,
      version: null,
      maxBytes: null,
      ...overrides,
    };
  }

  it("serves file content with provenance mirroring the real source", async () => {
    const source = createFakeRepositorySource(fixture, { now: () => NOW });
    const outcome = await source.fetch(
      request({ path: "README.md" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.sections[0]?.heading).toBe("Godot");
      expect(outcome.document.provenance).toMatchObject({
        source: sourceRef,
        requestedRef: null,
        resolvedRevision: null,
        requestedVersion: null,
        usedVersion: null,
        fallback: false,
        fetchedAtMs: NOW,
        resource: "files:HEAD:README.md",
      });
    }
  });

  it("records commit shas as resolvedRevision and branches as null", async () => {
    const source = createFakeRepositorySource(fixture);
    const commit = await source.fetch(
      request({ path: "README.md", ref: COMMIT }),
      bounds(),
      new AbortController().signal,
    );
    expect(commit.status).toBe("document");
    if (commit.status === "document") {
      expect(commit.document.provenance).toMatchObject({
        requestedRef: COMMIT,
        resolvedRevision: COMMIT,
        resource: `files:${COMMIT}:README.md`,
      });
    }
    const branch = await source.fetch(
      request({ path: "README.md", ref: "main" }),
      bounds(),
      new AbortController().signal,
    );
    expect(branch.status).toBe("document");
    if (branch.status === "document") {
      expect(branch.document.provenance).toMatchObject({
        requestedRef: "main",
        resolvedRevision: null,
      });
    }
  });

  it("serves the numerically-latest release", async () => {
    const source = createFakeRepositorySource(fixture);
    const outcome = await source.fetch(
      request({ topic: "release notes" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.provenance).toMatchObject({
        usedVersion: "4.10",
        resolvedRevision: null,
        resource: "releases:latest:4.10",
      });
      expect(outcome.document.sections[0]?.heading).toBe("v4.10");
    }
  });

  it("maps unclassifiable fixture content types to unsupported-content", async () => {
    const source = createFakeRepositorySource(fixture);
    const outcome = await source.fetch(
      request({ path: "logo.png", ref: "bin" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("unsupported-content");
  });

  it("fails with resource not found for missing files and repositories", async () => {
    const source = createFakeRepositorySource(fixture);
    const missingFile = await source.fetch(
      request({ path: "missing.md" }),
      bounds(),
      new AbortController().signal,
    );
    expect(missingFile.status).toBe("failed");
    if (missingFile.status === "failed") {
      expect(missingFile.reason).toBe("resource not found");
    }
    const missingRepo = await source.fetch(
      request({ query: "other/org", path: "README.md" }),
      bounds(),
      new AbortController().signal,
    );
    expect(missingRepo.status).toBe("failed");
  });

  it("rejects path traversal and invalid origins", async () => {
    const source = createFakeRepositorySource(fixture);
    const traversal = await source.fetch(
      request({ path: "../secret" }),
      bounds(),
      new AbortController().signal,
    );
    expect(traversal.status).toBe("refused");
    const badOrigin = await source.fetch(
      request({ query: "http://github.com/x/y", path: "README.md" }),
      bounds(),
      new AbortController().signal,
    );
    expect(badOrigin.status).toBe("refused");
  });

  it("returns cancelled for an already-aborted signal", async () => {
    const controller = new AbortController();
    controller.abort();
    const source = createFakeRepositorySource(fixture);
    const outcome = await source.fetch(request({ path: "README.md" }), bounds(), controller.signal);
    expect(outcome.status).toBe("cancelled");
  });
});
