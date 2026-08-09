import { describe, expect, it } from "vitest";
import {
  defaultResearchBounds,
  type ResearchBounds,
  type ResearchProvenance,
  type ResearchSourceRef,
} from "@solaris/core";
import {
  TRUNCATION_MARKER,
  buildResearchDocument,
  classifyContentType,
  normalizeHtmlToSections,
  normalizeJsonToSections,
  normalizeMarkdownToSections,
} from "./normalization.js";

function bounds(overrides: Partial<ResearchBounds> = {}): ResearchBounds {
  return { ...defaultResearchBounds(), ...overrides };
}

const source: ResearchSourceRef = {
  kind: "repository",
  id: "github",
  label: "GitHub repository research",
};

function provenance(overrides: Partial<ResearchProvenance> = {}): ResearchProvenance {
  return {
    source,
    requestedRef: null,
    resolvedRevision: null,
    requestedVersion: null,
    usedVersion: null,
    fallback: false,
    fallbackReason: null,
    fetchedAtMs: 1_700_000_000_000,
    resource: "README.md",
    ...overrides,
  };
}

describe("normalizeMarkdownToSections", () => {
  it("splits ATX headings into sections with leading text as an untitled section", () => {
    const result = normalizeMarkdownToSections(
      "preamble\n# Intro\nhello\n## Details\nmore lines",
      bounds(),
    );
    expect(result.truncated).toBe(false);
    expect(result.reason).toBeNull();
    expect(result.sections).toEqual([
      { heading: null, text: "preamble", byteLength: 8 },
      { heading: "Intro", text: "hello", byteLength: 5 },
      { heading: "Details", text: "more lines", byteLength: 10 },
    ]);
  });

  it("treats setext underline headings as headings", () => {
    const result = normalizeMarkdownToSections("Intro\n=====\nbody\nSub\n---\nmore", bounds());
    expect(result.sections).toEqual([
      { heading: "Intro", text: "body", byteLength: 4 },
      { heading: "Sub", text: "more", byteLength: 4 },
    ]);
  });

  it("returns an empty result for empty input", () => {
    const result = normalizeMarkdownToSections("", bounds());
    expect(result.sections).toEqual([]);
    expect(result.truncated).toBe(false);
  });

  it("caps the section count and marks the last section with the truncation marker", () => {
    const result = normalizeMarkdownToSections(
      "# A\na\n# B\nb\n# C\nc",
      bounds({ maxSections: 2 }),
    );
    expect(result.truncated).toBe(true);
    expect(result.reason).toBe("document exceeds the section limit; the last section is truncated");
    expect(result.sections).toHaveLength(2);
    expect(result.sections[0]?.heading).toBe("A");
    expect(result.sections[1]?.heading).toBe("B");
    expect(result.sections[1]?.text).toBe(`b${TRUNCATION_MARKER}`);
  });

  it("caps per-section text with the truncation marker", () => {
    const longBody = "This is a very long section body that exceeds the byte cap.";
    const result = normalizeMarkdownToSections(
      `# H\n${longBody}`,
      bounds({ maxSectionTextBytes: 30 }),
    );
    expect(result.truncated).toBe(true);
    expect(result.reason).toBe("section text exceeds the byte limit; the section is truncated");
    const section = result.sections[0];
    expect(section?.heading).toBe("H");
    expect(section?.text.endsWith(TRUNCATION_MARKER)).toBe(true);
    expect(section?.byteLength).toBe(30);
  });

  it("caps heading bytes without splitting UTF-8 characters", () => {
    const result = normalizeMarkdownToSections(
      "# SuperLongHeading\nbody",
      bounds({ maxHeadingBytes: 8 }),
    );
    expect(result.truncated).toBe(true);
    expect(result.reason).toBe("heading exceeds the byte limit; the heading is truncated");
    expect(result.sections[0]?.heading).toBe("SuperLon");
    expect(result.sections[0]?.text).toBe("body");
  });
});

describe("normalizeHtmlToSections", () => {
  it("extracts h1-h4 headings and body text, decoding entities", () => {
    const result = normalizeHtmlToSections(
      "<h1>Intro</h1><p>Hello &amp; goodbye.</p><h2>More</h2><p>Second <b>body</b>.</p>",
      bounds(),
    );
    expect(result.isEmpty).toBe(false);
    expect(result.sections).toEqual([
      { heading: "Intro", text: "Hello & goodbye.", byteLength: 16 },
      { heading: "More", text: "Second body.", byteLength: 12 },
    ]);
  });

  it("strips script and style blocks before extracting", () => {
    const result = normalizeHtmlToSections(
      '<h1>T</h1><script>if (a < b) { document.write("<h2>fake</h2>"); }</script><style>.x { color: red; }</style><p>Real text.</p>',
      bounds(),
    );
    expect(result.sections).toEqual([{ heading: "T", text: "Real text.", byteLength: 10 }]);
  });

  it("decodes the minimal entity set", () => {
    const result = normalizeHtmlToSections(
      "<p>&lt;tag&gt; &amp; &quot;q&quot; &#39;s&#39; &nbsp;x</p>",
      bounds(),
    );
    expect(result.sections[0]?.text).toBe("<tag> & \"q\" 's' x");
  });

  it("collapses whitespace runs", () => {
    const result = normalizeHtmlToSections("<p>line1\n   line2\t line3</p>", bounds());
    expect(result.sections[0]?.text).toBe("line1 line2 line3");
  });

  it("reports isEmpty for pages with no extractable text", () => {
    const result = normalizeHtmlToSections(
      "<html><head><script>var x = 1;</script><style>.a {}</style></head><body></body></html>",
      bounds(),
    );
    expect(result.isEmpty).toBe(true);
    expect(result.sections).toEqual([]);
  });

  it("bounds the section count like markdown", () => {
    const result = normalizeHtmlToSections(
      "<h1>A</h1><p>a</p><h2>B</h2><p>b</p><h3>C</h3><p>c</p>",
      bounds({ maxSections: 2 }),
    );
    expect(result.truncated).toBe(true);
    expect(result.sections).toHaveLength(2);
  });
});

describe("normalizeJsonToSections", () => {
  it("extracts the top-level body field when present", () => {
    const result = normalizeJsonToSections(
      '{"body":"Signals connect objects.","extra":1}',
      bounds(),
    );
    expect(result.sections).toEqual([
      { heading: null, text: "Signals connect objects.", byteLength: 24 },
    ]);
    expect(result.truncated).toBe(false);
  });

  it("extracts the description field when body is absent", () => {
    const result = normalizeJsonToSections('{"description":"desc text"}', bounds());
    expect(result.sections[0]?.text).toBe("desc text");
  });

  it("renders other JSON as a bounded pretty-printed excerpt", () => {
    const result = normalizeJsonToSections('{"a":1,"b":[1,2]}', bounds());
    expect(result.sections).toHaveLength(1);
    expect(result.sections[0]?.heading).toBeNull();
    expect(result.sections[0]?.text).toContain('"a": 1');
    expect(result.sections[0]?.text).toContain('"b": [');
  });

  it("falls back to the raw text for invalid JSON", () => {
    const result = normalizeJsonToSections("not json at all", bounds());
    expect(result.sections[0]?.text).toBe("not json at all");
  });

  it("caps the excerpt with the truncation marker", () => {
    const result = normalizeJsonToSections(
      '{"body":"' + "x".repeat(200) + '"}',
      bounds({ maxSectionTextBytes: 60 }),
    );
    expect(result.truncated).toBe(true);
    expect(result.reason).toBe("the JSON excerpt exceeds the byte limit; it is truncated");
    expect(result.sections[0]?.text.endsWith(TRUNCATION_MARKER)).toBe(true);
  });
});

describe("classifyContentType", () => {
  it("classifies the allowlist with charset parameters", () => {
    expect(classifyContentType("text/markdown")).toBe("text/markdown");
    expect(classifyContentType("text/markdown; charset=utf-8")).toBe("text/markdown");
    expect(classifyContentType("text/plain")).toBe("text/plain");
    expect(classifyContentType("application/json")).toBe("application/json");
    expect(classifyContentType("application/json; charset=utf-8")).toBe("application/json");
    expect(classifyContentType("Text/HTML")).toBe("text/html");
    expect(classifyContentType("text/html; charset=UTF-8")).toBe("text/html");
  });

  it("rejects everything outside the allowlist", () => {
    expect(classifyContentType(null)).toBeNull();
    expect(classifyContentType("")).toBeNull();
    expect(classifyContentType("application/octet-stream")).toBeNull();
    expect(classifyContentType("application/pdf")).toBeNull();
    expect(classifyContentType("application/zip")).toBeNull();
    expect(classifyContentType("video/mp4")).toBeNull();
    expect(classifyContentType("image/png")).toBeNull();
  });
});

describe("buildResearchDocument", () => {
  const now = 1_700_000_000_000;

  it("builds a bounded document with deterministic id and byteLength", () => {
    const document = buildResearchDocument({
      source,
      title: "Readme",
      contentType: "text/markdown",
      rawText: "# H\nbody",
      rawByteLength: 9,
      provenance: provenance(),
      bounds: bounds(),
      now,
    });
    expect(document.id.startsWith("rd_")).toBe(true);
    expect(document.id).toHaveLength(27);
    expect(document.fetchedAtMs).toBe(now);
    expect(document.title).toBe("Readme");
    expect(document.contentType).toBe("text/markdown");
    expect(document.sections).toEqual([{ heading: "H", text: "body", byteLength: 4 }]);
    // 128 fixed + 4 text + 1 heading + 64 per section.
    expect(document.byteLength).toBe(197);
    expect(document.truncated).toBe(false);
    expect(document.truncationReason).toBeNull();
    expect(document.links).toEqual([]);
  });

  it("produces the same id for the same resource and ref, different for others", () => {
    const base = {
      source,
      title: null,
      contentType: "text/markdown" as const,
      rawText: "x",
      rawByteLength: 1,
      bounds: bounds(),
      now,
    };
    const first = buildResearchDocument({ ...base, provenance: provenance({ resource: "a" }) });
    const second = buildResearchDocument({ ...base, provenance: provenance({ resource: "a" }) });
    const other = buildResearchDocument({ ...base, provenance: provenance({ resource: "b" }) });
    const otherRef = buildResearchDocument({
      ...base,
      provenance: provenance({ resource: "a", requestedRef: "main" }),
    });
    expect(second.id).toBe(first.id);
    expect(other.id).not.toBe(first.id);
    expect(otherRef.id).not.toBe(first.id);
  });

  it("preserves provenance fields", () => {
    const document = buildResearchDocument({
      source,
      title: null,
      contentType: "text/plain",
      rawText: "plain",
      rawByteLength: 5,
      provenance: provenance({ fallback: true, fallbackReason: "reason", usedVersion: "4.7" }),
      bounds: bounds(),
      now,
    });
    expect(document.provenance.fallback).toBe(true);
    expect(document.provenance.fallbackReason).toBe("reason");
    expect(document.provenance.usedVersion).toBe("4.7");
    expect(document.provenance.fetchedAtMs).toBe(now);
  });

  it("drops trailing sections when the serialized document exceeds maxDocumentBytes", () => {
    const rawText = Array.from(
      { length: 4 },
      (_, index) => `# Section ${index}\n${"x".repeat(200)}`,
    ).join("\n\n");
    const document = buildResearchDocument({
      source,
      title: null,
      contentType: "text/markdown",
      rawText,
      rawByteLength: new TextEncoder().encode(rawText).length,
      provenance: provenance(),
      bounds: bounds({ maxDocumentBytes: 900 }),
      now,
    });
    expect(document.truncated).toBe(true);
    expect(document.truncationReason).toBe(
      "the document exceeds the byte limit; trailing sections were dropped",
    );
    expect(document.sections.length).toBeLessThan(4);
    expect(JSON.stringify(document).length).toBeLessThanOrEqual(900);
  });

  it("trims the remaining section text when a single section still exceeds the cap", () => {
    const document = buildResearchDocument({
      source,
      title: null,
      contentType: "text/markdown",
      rawText: "# H\n" + "y".repeat(400),
      rawByteLength: 403,
      provenance: provenance(),
      bounds: bounds({ maxDocumentBytes: 700 }),
      now,
    });
    expect(document.truncated).toBe(true);
    expect(document.truncationReason).toBe(
      "the document exceeds the byte limit; the section text was truncated",
    );
    expect(document.sections).toHaveLength(1);
    expect(JSON.stringify(document).length).toBeLessThanOrEqual(700);
    expect(document.sections[0]?.text.endsWith(TRUNCATION_MARKER)).toBe(true);
  });

  it("keeps all sections under the default bounds", () => {
    const rawText = Array.from(
      { length: 4 },
      (_, index) => `# Section ${index}\n${"x".repeat(200)}`,
    ).join("\n\n");
    const document = buildResearchDocument({
      source,
      title: null,
      contentType: "text/markdown",
      rawText,
      rawByteLength: new TextEncoder().encode(rawText).length,
      provenance: provenance(),
      bounds: bounds(),
      now,
    });
    expect(document.sections).toHaveLength(4);
    expect(document.truncated).toBe(false);
  });
});
