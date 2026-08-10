import { describe, expect, it } from "vitest";
import {
  defaultResearchBounds,
  type ResearchBounds,
  type ResearchRequest,
  type ResearchSourceRef,
} from "@solaris/core";
import { createGitHubResearchSource } from "./github-source.js";
import { createFakeTransport, type FakeTransportRoutes } from "./http-transport.js";
import { TRUNCATION_MARKER } from "./normalization.js";

const SOURCE: ResearchSourceRef = {
  kind: "repository",
  id: "github",
  label: "GitHub repository research",
};

const COMMIT = "a".repeat(40);
const NOW = 1_700_000_000_000;

function request(overrides: Partial<ResearchRequest> = {}): ResearchRequest {
  return {
    source: SOURCE,
    query: "godotengine/godot",
    topic: null,
    path: null,
    ref: null,
    version: null,
    maxBytes: null,
    ...overrides,
  };
}

function bounds(overrides: Partial<ResearchBounds> = {}): ResearchBounds {
  return { ...defaultResearchBounds(), ...overrides };
}

function makeSource(routes: FakeTransportRoutes) {
  return createGitHubResearchSource({ transport: createFakeTransport(routes), now: () => NOW });
}

const RAW_README_URL = "https://raw.githubusercontent.com/godotengine/godot/HEAD/README.md";

describe("createGitHubResearchSource", () => {
  it("fetches known file content with provenance for an absent ref", async () => {
    const source = makeSource({
      [RAW_README_URL]: { body: "# Godot\n\nEngine.", contentType: "text/markdown" },
    });
    const outcome = await source.fetch(
      request({ path: "README.md" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.sections[0]?.heading).toBe("Godot");
      expect(outcome.document.provenance).toMatchObject({
        source: SOURCE,
        requestedRef: null,
        resolvedRevision: null,
        requestedVersion: null,
        usedVersion: null,
        fallback: false,
        fetchedAtMs: NOW,
        resource: RAW_README_URL,
      });
      expect(outcome.document.title).toBeNull();
    }
  });

  it("records a full commit sha as the resolved revision and fetches it directly", async () => {
    const url = `https://raw.githubusercontent.com/godotengine/godot/${COMMIT}/README.md`;
    const source = makeSource({ [url]: { body: "# At commit" } });
    const outcome = await source.fetch(
      request({ path: "README.md", ref: COMMIT }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.provenance.requestedRef).toBe(COMMIT);
      expect(outcome.document.provenance.resolvedRevision).toBe(COMMIT);
    }
  });

  it("never claims branch content as an immutable commit", async () => {
    const url = "https://raw.githubusercontent.com/godotengine/godot/main/README.md";
    const source = makeSource({ [url]: { body: "# On main" } });
    const outcome = await source.fetch(
      request({ path: "README.md", ref: "main" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.provenance.requestedRef).toBe("main");
      expect(outcome.document.provenance.resolvedRevision).toBeNull();
    }
  });

  it("fetches latest release notes when the request mentions release without a path", async () => {
    const url = "https://api.github.com/repos/godotengine/godot/releases/latest";
    const source = makeSource({
      [url]: { body: "# 4.3\n\nNew features.", contentType: "application/json" },
    });
    const outcome = await source.fetch(
      request({ topic: "release notes" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.provenance.resource).toBe(url);
      expect(outcome.document.provenance.resolvedRevision).toBeNull();
      expect(outcome.document.sections[0]?.text).toContain("New features.");
    }
  });

  it("maps binary content types to unsupported-content", async () => {
    const source = makeSource({
      [RAW_README_URL]: { body: "\u0000\u0001", contentType: "application/octet-stream" },
    });
    const outcome = await source.fetch(
      request({ path: "README.md" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("unsupported-content");
  });

  it("honors a request-level download cap without exceeding the host bound", async () => {
    const source = makeSource({
      [RAW_README_URL]: { body: "x".repeat(32), contentType: "text/plain" },
    });
    const outcome = await source.fetch(
      request({ path: "README.md", maxBytes: 16 }),
      bounds({ maxDownloadBytes: 1024 }),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("oversized");
  });

  it("maps 404 to failed resource not found", async () => {
    const source = makeSource({ [RAW_README_URL]: { body: "nope", statusCode: 404 } });
    const outcome = await source.fetch(
      request({ path: "README.md" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome).toEqual({ status: "failed", reason: "resource not found" });
  });

  it("maps 403 and 429 to failed rate limited", async () => {
    for (const statusCode of [403, 429]) {
      const source = makeSource({ [RAW_README_URL]: { body: "slow down", statusCode } });
      const outcome = await source.fetch(
        request({ path: "README.md" }),
        bounds(),
        new AbortController().signal,
      );
      expect(outcome).toEqual({ status: "failed", reason: "rate limited" });
    }
  });

  it("rejects path traversal and absolute paths before any fetch", async () => {
    const source = makeSource({});
    const traversal = await source.fetch(
      request({ path: "../secret" }),
      bounds(),
      new AbortController().signal,
    );
    expect(traversal.status).toBe("refused");
    if (traversal.status === "refused") {
      expect(traversal.reason).toContain('".."');
    }
    const absolute = await source.fetch(
      request({ path: "/etc/passwd" }),
      bounds(),
      new AbortController().signal,
    );
    expect(absolute.status).toBe("refused");
    const windows = await source.fetch(
      request({ path: "C:\\windows\\x" }),
      bounds(),
      new AbortController().signal,
    );
    expect(windows.status).toBe("refused");
  });

  it("rejects invalid repository origins", async () => {
    const source = makeSource({});
    const outcome = await source.fetch(
      request({ query: "http://github.com/godotengine/godot", path: "README.md" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("refused");
    if (outcome.status === "refused") {
      expect(outcome.reason).toContain("invalid repository origin");
    }
  });

  it("refuses requests with neither a path nor a release topic", async () => {
    const source = makeSource({});
    const outcome = await source.fetch(request(), bounds(), new AbortController().signal);
    expect(outcome.status).toBe("refused");
  });

  it("truncates document sections under tight bounds", async () => {
    const source = makeSource({
      [RAW_README_URL]: { body: `# Godot\n\n${"x".repeat(200)}`, contentType: "text/markdown" },
    });
    const outcome = await source.fetch(
      request({ path: "README.md" }),
      bounds({ maxSectionTextBytes: 60 }),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.truncated).toBe(true);
      expect(outcome.document.truncationReason).toContain("byte limit");
      expect(outcome.document.sections[0]?.text.endsWith(TRUNCATION_MARKER)).toBe(true);
    }
  });

  it("returns cancelled for an already-aborted signal", async () => {
    const controller = new AbortController();
    controller.abort();
    const source = makeSource({});
    const outcome = await source.fetch(request({ path: "README.md" }), bounds(), controller.signal);
    expect(outcome.status).toBe("cancelled");
  });

  it("propagates transport timeouts, oversized, and refusals", async () => {
    const slow = createGitHubResearchSource({
      transport: createFakeTransport({ [RAW_README_URL]: { body: "x", delayMs: 30 } }),
      now: () => NOW,
    });
    const timeout = await slow.fetch(
      request({ path: "README.md" }),
      bounds({ timeoutMs: 10 }),
      new AbortController().signal,
    );
    expect(timeout.status).toBe("timeout");

    const big = createGitHubResearchSource({
      transport: createFakeTransport({ [RAW_README_URL]: { body: "x".repeat(100) } }),
      now: () => NOW,
    });
    const oversized = await big.fetch(
      request({ path: "README.md" }),
      bounds({ maxDownloadBytes: 50 }),
      new AbortController().signal,
    );
    expect(oversized.status).toBe("oversized");

    const unconfigured = createGitHubResearchSource({
      transport: createFakeTransport({}),
      now: () => NOW,
    });
    const noRoute = await unconfigured.fetch(
      request({ path: "README.md" }),
      bounds(),
      new AbortController().signal,
    );
    expect(noRoute.status).toBe("failed");
    if (noRoute.status === "failed") {
      expect(noRoute.reason).toBe("no route");
    }
  });
});
