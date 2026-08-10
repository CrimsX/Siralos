import { describe, expect, it } from "vitest";
import {
  defaultResearchBounds,
  type ResearchBounds,
  type ResearchRequest,
  type ResearchSourceRef,
} from "@solaris/core";
import {
  buildDocsUrl,
  createGodotDocsResearchSource,
  resolveDocsVersion,
} from "./godot-docs-source.js";
import { createFakeTransport, type FakeTransportRoutes } from "./http-transport.js";
import { TRUNCATION_MARKER } from "./normalization.js";

const SOURCE: ResearchSourceRef = {
  kind: "godot-docs",
  id: "godot-docs",
  label: "Godot official documentation",
};

const NOW = 1_700_000_000_000;

function request(overrides: Partial<ResearchRequest> = {}): ResearchRequest {
  return {
    source: SOURCE,
    query: "how do signals work",
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
  return createGodotDocsResearchSource({ transport: createFakeTransport(routes), now: () => NOW });
}

const CLASS_PAGE_HTML = "<h1>CharacterBody2D</h1><p>Character body for 2D physics.</p>";

describe("resolveDocsVersion", () => {
  it("maps a requested version exactly when it is a published minor", () => {
    expect(resolveDocsVersion("4.7")).toEqual({
      usedVersion: "4.7",
      fallback: false,
      fallbackReason: null,
    });
    expect(resolveDocsVersion("4.7-stable")).toEqual({
      usedVersion: "4.7",
      fallback: false,
      fallbackReason: null,
    });
  });

  it("maps patch versions to the minor with an explicit fallback", () => {
    expect(resolveDocsVersion("4.7.1")).toEqual({
      usedVersion: "4.7",
      fallback: true,
      fallbackReason: "patch version not published; using minor docs",
    });
  });

  it("falls back to stable for one-segment and unpublished versions", () => {
    expect(resolveDocsVersion("4")).toEqual({
      usedVersion: "stable",
      fallback: true,
      fallbackReason: "docs are published per minor version; using stable",
    });
    expect(resolveDocsVersion("4.x")).toEqual({
      usedVersion: "stable",
      fallback: true,
      fallbackReason: "docs are published per minor version; using stable",
    });
    expect(resolveDocsVersion("4.7-beta").fallback).toBe(true);
    expect(resolveDocsVersion("4.7-beta").usedVersion).toBe("stable");
  });

  it("uses stable without fallback marking when no version was requested", () => {
    expect(resolveDocsVersion(null)).toEqual({
      usedVersion: "stable",
      fallback: false,
      fallbackReason: null,
    });
  });
});

describe("buildDocsUrl", () => {
  it("maps topics to class pages with the class_ prefix", () => {
    expect(buildDocsUrl("4.7", request({ topic: "CharacterBody2D" }))).toBe(
      "https://docs.godotengine.org/en/4.7/classes/class_characterbody2d.html",
    );
    expect(buildDocsUrl("stable", request({ topic: "class_node" }))).toBe(
      "https://docs.godotengine.org/en/stable/classes/class_node.html",
    );
  });

  it("uses the search page when no topic is given", () => {
    expect(buildDocsUrl("stable", request({ query: "signals & groups" }))).toBe(
      "https://docs.godotengine.org/en/stable/search.html?q=signals%20%26%20groups",
    );
  });
});

describe("createGodotDocsResearchSource", () => {
  it("honors a request-level download cap without exceeding the host bound", async () => {
    const url = "https://docs.godotengine.org/en/stable/search.html?q=how%20do%20signals%20work";
    const source = makeSource({
      [url]: { body: "x".repeat(32), contentType: "text/plain" },
    });
    const outcome = await source.fetch(
      request({ maxBytes: 16 }),
      bounds({ maxDownloadBytes: 1024 }),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("oversized");
  });

  it("serves the exact version with no fallback marking", async () => {
    const url = "https://docs.godotengine.org/en/4.7/classes/class_characterbody2d.html";
    const source = makeSource({ [url]: { body: CLASS_PAGE_HTML, contentType: "text/html" } });
    const outcome = await source.fetch(
      request({ topic: "CharacterBody2D", version: "4.7" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.sections[0]).toEqual({
        heading: "CharacterBody2D",
        text: "Character body for 2D physics.",
        byteLength: 30,
      });
      expect(outcome.document.provenance).toMatchObject({
        requestedVersion: "4.7",
        usedVersion: "4.7",
        fallback: false,
        fallbackReason: null,
        resource: url,
        requestedRef: null,
        resolvedRevision: null,
      });
    }
  });

  it("marks the fallback for a patch version and serves the minor docs", async () => {
    const url = "https://docs.godotengine.org/en/4.7/classes/class_characterbody2d.html";
    const source = makeSource({ [url]: { body: CLASS_PAGE_HTML, contentType: "text/html" } });
    const outcome = await source.fetch(
      request({ topic: "CharacterBody2D", version: "4.7.1" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.provenance.usedVersion).toBe("4.7");
      expect(outcome.document.provenance.fallback).toBe(true);
      expect(outcome.document.provenance.fallbackReason).toBe(
        "patch version not published; using minor docs",
      );
    }
  });

  it("uses stable when no version is requested", async () => {
    const url = "https://docs.godotengine.org/en/stable/classes/class_node.html";
    const source = makeSource({ [url]: { body: CLASS_PAGE_HTML, contentType: "text/html" } });
    const outcome = await source.fetch(
      request({ topic: "Node", version: null }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.provenance).toMatchObject({
        requestedVersion: null,
        usedVersion: "stable",
        fallback: false,
      });
    }
  });

  it("maps a missing page to failed not found", async () => {
    const url = "https://docs.godotengine.org/en/stable/classes/class_nope.html";
    const source = makeSource({ [url]: { body: "missing", statusCode: 404 } });
    const outcome = await source.fetch(
      request({ topic: "Nope" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome).toEqual({ status: "failed", reason: "not found" });
  });

  it("fails closed for pages with no extractable text", async () => {
    const url = "https://docs.godotengine.org/en/stable/classes/class_node.html";
    const source = makeSource({
      [url]: { body: "<html><body></body></html>", contentType: "text/html" },
    });
    const outcome = await source.fetch(
      request({ topic: "Node" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("unsupported-content");
  });

  it("truncates sections under tight bounds", async () => {
    const url = "https://docs.godotengine.org/en/stable/classes/class_node.html";
    const source = makeSource({
      [url]: { body: `<h1>Node</h1><p>${"x".repeat(200)}</p>`, contentType: "text/html" },
    });
    const outcome = await source.fetch(
      request({ topic: "Node" }),
      bounds({ maxSectionTextBytes: 60 }),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("document");
    if (outcome.status === "document") {
      expect(outcome.document.truncated).toBe(true);
      expect(outcome.document.sections[0]?.text.endsWith(TRUNCATION_MARKER)).toBe(true);
    }
  });

  it("returns cancelled for an already-aborted signal", async () => {
    const controller = new AbortController();
    controller.abort();
    const source = makeSource({});
    const outcome = await source.fetch(request({ topic: "Node" }), bounds(), controller.signal);
    expect(outcome.status).toBe("cancelled");
  });

  it("propagates transport failures", async () => {
    const source = makeSource({});
    const outcome = await source.fetch(
      request({ topic: "Node" }),
      bounds(),
      new AbortController().signal,
    );
    expect(outcome.status).toBe("failed");
    if (outcome.status === "failed") {
      expect(outcome.reason).toBe("no route");
    }
  });
});
