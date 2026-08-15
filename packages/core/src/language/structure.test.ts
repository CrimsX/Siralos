import { describe, expect, it } from "vitest";
import {
  DEFAULT_STRUCTURE_LIMITS,
  DEFAULT_SUMMARY_MAX_BYTES,
  DEFAULT_SUMMARY_NOTABLE_DECLARATIONS,
  SUMMARY_FOOTER,
  SUMMARY_TRUNCATION_MARKER,
  buildStructuralSummary,
  declarationCount,
  normalizeStructuralDocument,
  parseStructuralKind,
  type StructuralDeclaration,
  type StructuralDocument,
  type StructuralIssue,
  type StructureOptions,
} from "./structure.js";
import { utf8ByteLength } from "./truncate.js";

/**
 * Focused reference-semantics tests for the generic structural-document
 * representation and advisory summary formatter (Stage 3R R5). These
 * prove the production TypeScript reference behavior that the
 * differential oracle probes exercise.
 */

function declaration(
  kind: StructuralDeclaration["kind"],
  name: string,
  line: number,
): StructuralDeclaration {
  return { kind, name, detail: null, line, attributes: [], children: [] };
}

function normalized(
  path: string,
  declarations: readonly StructuralDeclaration[],
  dependencies: readonly string[],
  issues: readonly StructuralIssue[],
  options: StructureOptions = DEFAULT_STRUCTURE_LIMITS,
): StructuralDocument {
  return normalizeStructuralDocument(path, declarations, dependencies, issues, options);
}

describe("structural kind vocabulary", () => {
  it("is a closed generic vocabulary without language-domain kinds", () => {
    expect(parseStructuralKind("signal")).toBeNull();
    expect(parseStructuralKind("class_name")).toBeNull();
    expect(parseStructuralKind("property")).toBeNull();
    expect(parseStructuralKind("type")).toBe("type");
    expect(parseStructuralKind("event")).toBe("event");
    expect(parseStructuralKind("other")).toBe("other");
    expect(parseStructuralKind(undefined)).toBeNull();
  });
});

describe("normalizeStructuralDocument", () => {
  it("preserves deterministic document order", () => {
    const document = normalized(
      "src/example.lang",
      [
        declaration("type", "Example", 1),
        declaration("function", "calculate", 5),
        declaration("constant", "LIMIT", 9),
      ],
      [],
      [],
    );
    expect(document.declarations.map((item) => item.name)).toEqual([
      "Example",
      "calculate",
      "LIMIT",
    ]);
    expect(declarationCount(document)).toBe(3);
  });

  it("binds nested declarations in document order", () => {
    const document = normalized(
      "src/example.lang",
      [
        {
          kind: "type",
          name: "Example",
          detail: null,
          line: 1,
          attributes: [],
          children: [declaration("method", "calculate", 3), declaration("field", "value", 4)],
        },
      ],
      [],
      [],
    );
    expect(declarationCount(document)).toBe(3);
    expect(document.declarations[0]?.children[0]?.kind).toBe("method");
  });

  it("derives partial status from typed issues", () => {
    const document = normalized(
      "src/broken.lang",
      [declaration("function", "run", 3)],
      [],
      [{ line: 2, message: "Unterminated string literal." }],
    );
    expect(document.status).toBe("partial");
    expect(document.issues).toHaveLength(1);
    const clean = normalized("src/clean.lang", [], [], []);
    expect(clean.status).toBe("complete");
  });

  it("bounds declarations, dependencies, and issues with explicit truncation", () => {
    const declarations = Array.from({ length: 6 }, (_, index) =>
      declaration("function", `f${index}`, index + 1),
    );
    const document = normalizeStructuralDocument(
      "src/big.lang",
      declarations,
      Array.from({ length: 8 }, (_, index) => `dep${index}`),
      Array.from({ length: 80 }, (_, index) => ({
        line: index + 1,
        message: `issue ${index}`,
      })),
      {
        maxDeclarations: 4,
        maxDepth: 16,
        maxDependencies: 3,
        maxIssues: 5,
      },
    );
    expect(document.truncated).toBe(true);
    expect(document.declarations).toHaveLength(4);
    expect(document.dependencies).toEqual(["dep0", "dep1", "dep2"]);
    expect(document.issues).toHaveLength(5);
    expect(document.status).toBe("partial");
  });

  it("excludes declarations deeper than the depth bound", () => {
    const document = normalizeStructuralDocument(
      "src/deep.lang",
      [
        {
          kind: "type",
          name: "Outer",
          detail: null,
          line: 1,
          attributes: [],
          children: [
            {
              kind: "type",
              name: "Inner",
              detail: null,
              line: 2,
              attributes: [],
              children: [declaration("method", "too_deep", 3)],
            },
          ],
        },
      ],
      [],
      [],
      { maxDeclarations: 256, maxDepth: 2, maxDependencies: 32, maxIssues: 64 },
    );
    expect(document.truncated).toBe(true);
    expect(declarationCount(document)).toBe(2);
    expect(document.declarations[0]?.children[0]?.children).toHaveLength(0);
  });

  it("never panics on huge declaration lists", () => {
    const many = Array.from({ length: 100_000 }, (_, index) =>
      declaration("variable", "", index + 1),
    );
    const bounded = normalizeStructuralDocument("src/huge.lang", many, [], []);
    expect(bounded.truncated).toBe(true);
    expect(bounded.declarations).toHaveLength(DEFAULT_STRUCTURE_LIMITS.maxDeclarations);
  });
});

describe("buildStructuralSummary", () => {
  it("renders generic counts, top-level names, dependencies, and revision", () => {
    const document = normalized(
      "src/example.lang",
      [
        {
          kind: "type",
          name: "Example",
          detail: null,
          line: 1,
          attributes: [],
          children: [declaration("method", "calculate", 3)],
        },
        declaration("function", "run", 5),
        declaration("constant", "LIMIT", 9),
      ],
      ["lib/common"],
      [],
    );
    const summary = buildStructuralSummary({ ...document, revision: "rev_abc" });
    expect(summary.truncated).toBe(false);
    expect(summary.text).toContain("example.lang (summary @ rev_abc)");
    expect(summary.text).toContain(
      "- declarations: 4 (type: 1, function: 1, method: 1, constant: 1)",
    );
    expect(summary.text).toContain("- top-level: Example, run, LIMIT");
    expect(summary.text).toContain("- dependencies: lib/common");
    expect(summary.text).not.toContain("- extends");
    expect(summary.text).not.toContain("class_name");
    expect(summary.text).not.toContain("signals");
    expect(summary.text).toContain(SUMMARY_FOOTER);
  });

  it("never interprets opaque attributes as language semantics", () => {
    const document = normalized(
      "src/example.lang",
      [
        {
          kind: "field",
          name: "speed",
          detail: null,
          line: 14,
          attributes: ["export", "export_range"],
          children: [],
        },
      ],
      [],
      [],
    );
    const summary = buildStructuralSummary(document);
    expect(summary.text).not.toContain("export");
    expect(summary.text).not.toContain("signal");
  });

  it("keeps the footer and marks summary truncation explicitly", () => {
    const document = normalized(
      "src/big.lang",
      Array.from({ length: 30 }, (_, index) => ({
        kind: "function" as const,
        name: `function_${String(index).padStart(2, "0")}`,
        detail: null,
        line: index + 1,
        attributes: [],
        children: [],
      })),
      [],
      [],
    );
    // A 220-byte budget cannot fit the full body plus the footer, so
    // the body is byte-truncated with the explicit marker.
    const summary = buildStructuralSummary(
      { ...document, revision: "rev_abc" },
      {
        maxBytes: 220,
      },
    );
    expect(summary.truncated).toBe(true);
    expect(summary.bytes).toBeLessThanOrEqual(220);
    expect(summary.text).toContain(SUMMARY_TRUNCATION_MARKER);
    expect(summary.text.endsWith(SUMMARY_FOOTER)).toBe(true);
    expect(summary.text).toContain("- top-level: function_00");
    expect(summary.text).not.toContain(", ... (30 total)");
  });

  it("renders an empty document baseline without fabrication", () => {
    const document = normalized("src/empty.lang", [], [], []);
    const summary = buildStructuralSummary(document);
    expect(summary.truncated).toBe(false);
    expect(summary.text).toContain("empty.lang (summary no revision)");
    expect(summary.text).not.toContain("- declarations:");
    expect(summary.text).not.toContain("- top-level:");
    expect(summary.text.endsWith(SUMMARY_FOOTER)).toBe(true);
    expect(summary.bytes).toBe(utf8ByteLength(summary.text));
  });

  it("reports partial status with the bounded issue count", () => {
    const document = normalized(
      "src/broken.lang",
      [declaration("function", "run", 3)],
      [],
      [
        { line: 2, message: "Unterminated string literal." },
        { line: 8, message: "Malformed function declaration (no name)." },
      ],
    );
    const summary = buildStructuralSummary(document);
    expect(summary.text).toContain("- structural status: partial (2 issue(s))");
  });

  it("marks structural output truncation explicitly in the summary", () => {
    const document = normalizeStructuralDocument(
      "src/big.lang",
      [],
      Array.from({ length: 40 }, (_, index) => `d_${index}`),
      [],
    );
    expect(document.truncated).toBe(true);
    const summary = buildStructuralSummary(document);
    expect(summary.text).toContain("- structural output truncated (output bound reached)");
  });

  it("exposes the default budgets", () => {
    expect(DEFAULT_SUMMARY_MAX_BYTES).toBe(4096);
    expect(DEFAULT_SUMMARY_NOTABLE_DECLARATIONS).toBe(12);
    expect(DEFAULT_STRUCTURE_LIMITS).toEqual({
      maxDeclarations: 256,
      maxDepth: 16,
      maxDependencies: 32,
      maxIssues: 64,
    });
  });
});
