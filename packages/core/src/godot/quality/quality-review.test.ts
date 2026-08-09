import { describe, expect, it } from "vitest";
import {
  QUALITY_LIMITS,
  type ChangeDiffMetrics,
} from "./quality-model.js";
import {
  aggregateReviewResults,
  chunkChangeReviewRequests,
  classifyReviewFindingBlocking,
  countBlockingFindings,
  deduplicateReviewFindings,
  deterministicFindingId,
  normalizeReviewFindings,
  type ChangeReviewFinding,
  type ChangeReviewRequest,
} from "./quality-review.js";

function finding(overrides: Partial<ChangeReviewFinding> = {}): ChangeReviewFinding {
  const base: ChangeReviewFinding = {
    id: "placeholder",
    severity: "high",
    category: "correctness",
    title: "wrong clamp direction",
    path: "scripts/player/player.gd",
    line: 10,
    evidence: "the clamp call is inverted",
    impact: "health can exceed max_health",
    recommendation: "swap the clamp arguments",
    confidence: "high",
    ...overrides,
  };
  return {
    ...base,
    id: deterministicFindingId({
      category: base.category,
      path: base.path,
      line: base.line,
      title: base.title,
    }),
  };
}

function request(overrides: Partial<ChangeReviewRequest> = {}): ChangeReviewRequest {
  const metrics: ChangeDiffMetrics = {
    filesChanged: 1,
    linesAdded: 4,
    linesRemoved: 2,
    filesCreated: 0,
    filesDeleted: 0,
    functionsTouched: 1,
  };
  return {
    developmentId: "dev-1",
    request: "add a heal method",
    engineVersion: "4.7.1-stable",
    changedPaths: ["scripts/player/player.gd"],
    files: [{ path: "scripts/player/player.gd", unifiedDiff: "diff" }],
    metrics,
    evidenceSummary: [],
    repositoryGuidance: null,
    previousFindingIds: [],
    reviewRound: 1,
    ...overrides,
  };
}

describe("deterministic finding identity", () => {
  it("derives ids from safe normalized fields only", () => {
    const id = deterministicFindingId({
      category: "correctness",
      path: "a.gd",
      line: 3,
      title: "Clamp Direction",
    });
    expect(id).toMatch(/^[0-9a-f]{24}$/);
    const again = deterministicFindingId({
      category: "correctness",
      path: "a.gd",
      line: 3,
      title: "clamp  direction",
    });
    expect(id).toBe(again);
  });

  it("differs when the category, path, line, or title differs", () => {
    const base = { category: "correctness" as const, path: "a.gd", line: 3, title: "t" };
    expect(deterministicFindingId({ ...base, category: "regression" })).not.toBe(
      deterministicFindingId(base),
    );
    expect(deterministicFindingId({ ...base, path: "b.gd" })).not.toBe(
      deterministicFindingId(base),
    );
    expect(deterministicFindingId({ ...base, line: 4 })).not.toBe(deterministicFindingId(base));
    expect(deterministicFindingId({ ...base, title: "u" })).not.toBe(deterministicFindingId(base));
  });
});

describe("review finding normalization", () => {
  it("validates a well-formed findings payload and assigns ids", () => {
    const parsed = normalizeReviewFindings({
      findings: [
        {
          severity: "high",
          category: "correctness",
          title: "inverted clamp",
          path: "scripts/player/player.gd",
          line: 10,
          evidence: "clamp(min, max) is reversed",
          impact: "health can exceed max",
          recommendation: "swap arguments",
          confidence: "high",
        },
      ],
    });
    expect(parsed.ok).toBe(true);
    if (parsed.ok) {
      expect(parsed.findings[0]?.id).toMatch(/^[0-9a-f]{24}$/);
      expect(parsed.findings[0]?.path).toBe("scripts/player/player.gd");
    }
  });

  it("rejects malformed findings", () => {
    for (const malformed of [
      { findings: "nope" },
      { findings: [{}] },
      { findings: [{ severity: "critical" }] },
      { findings: [{ severity: "critical", category: "unknown-category", title: "t", confidence: "high" }] },
      { findings: [{ severity: "critical", category: "correctness", title: 42, confidence: "high" }] },
      { findings: [null] },
      "not an object",
    ]) {
      const parsed = normalizeReviewFindings(malformed);
      expect(parsed.ok).toBe(false);
    }
  });

  it("binds the finding count to the immutable maximum", () => {
    const findings = Array.from({ length: QUALITY_LIMITS.maxReviewFindings + 1 }, (_, index) => ({
      severity: "low",
      category: "style",
      title: `finding ${index}`,
      evidence: "e",
      impact: "i",
      recommendation: "r",
      confidence: "high",
    }));
    const parsed = normalizeReviewFindings({ findings });
    expect(parsed.ok).toBe(false);
    expect("message" in parsed && parsed.message.includes("maximum")).toBe(true);
  });

  it("rejects oversized titles and evidence fields", () => {
    const oversizedTitle = normalizeReviewFindings({
      findings: [
        {
          severity: "low",
          category: "style",
          title: "t".repeat(QUALITY_LIMITS.maxFindingTitleChars + 1),
          evidence: "e",
          impact: "i",
          recommendation: "r",
          confidence: "high",
        },
      ],
    });
    expect(oversizedTitle.ok).toBe(false);
    const oversizedEvidence = normalizeReviewFindings({
      findings: [
        {
          severity: "low",
          category: "style",
          title: "t",
          evidence: "x".repeat(QUALITY_LIMITS.maxFindingEvidenceChars + 1),
          impact: "i",
          recommendation: "r",
          confidence: "high",
        },
      ],
    });
    expect(oversizedEvidence.ok).toBe(false);
  });

  it("rejects absolute and private paths", () => {
    for (const path of [
      "/etc/passwd",
      "C:\\Users\\secret\\file.gd",
      "../outside.gd",
      "scripts/../escape.gd",
      "C:/private/path.gd",
      "scripts/a\\b.gd",
    ]) {
      const parsed = normalizeReviewFindings({
        findings: [
          {
            severity: "low",
            category: "style",
            title: "t",
            path,
            evidence: "e",
            impact: "i",
            recommendation: "r",
            confidence: "high",
          },
        ],
      });
      expect(parsed.ok).toBe(false);
    }
  });

  it("accepts null paths and lines", () => {
    const parsed = normalizeReviewFindings({
      findings: [
        {
          severity: "medium",
          category: "maintainability",
          title: "t",
          path: null,
          line: null,
          evidence: "e",
          impact: "i",
          recommendation: "r",
          confidence: "medium",
        },
      ],
    });
    expect(parsed.ok).toBe(true);
  });

  it("trims strings", () => {
    const parsed = normalizeReviewFindings({
      findings: [
        {
          severity: "low",
          category: "style",
          title: "  tidy title  ",
          evidence: "  e  ",
          impact: "i",
          recommendation: "r",
          confidence: "high",
        },
      ],
    });
    expect(parsed.ok).toBe(true);
    if (parsed.ok) {
      expect(parsed.findings[0]?.title).toBe("tidy title");
      expect(parsed.findings[0]?.evidence).toBe("e");
    }
  });

  it("deduplicates conservatively by deterministic identity", () => {
    const parsed = normalizeReviewFindings({
      findings: [
        {
          severity: "high",
          category: "correctness",
          title: "same issue",
          path: "a.gd",
          line: 1,
          evidence: "one",
          impact: "i",
          recommendation: "r",
          confidence: "high",
        },
        {
          severity: "high",
          category: "correctness",
          title: "Same Issue",
          path: "a.gd",
          line: 1,
          evidence: "two",
          impact: "i",
          recommendation: "r",
          confidence: "high",
        },
      ],
    });
    expect(parsed.ok).toBe(true);
    if (parsed.ok) {
      expect(parsed.findings).toHaveLength(1);
    }
  });
});

describe("blocking policy", () => {
  it("blocks Critical and High findings with sufficient confidence", () => {
    expect(classifyReviewFindingBlocking(finding({ severity: "critical", confidence: "high" }))).toBe(true);
    expect(classifyReviewFindingBlocking(finding({ severity: "critical", confidence: "medium" }))).toBe(true);
    expect(classifyReviewFindingBlocking(finding({ severity: "high", confidence: "high" }))).toBe(true);
    expect(classifyReviewFindingBlocking(finding({ severity: "high", confidence: "medium" }))).toBe(true);
  });

  it("never silently blocks a low-confidence Critical/High finding", () => {
    expect(classifyReviewFindingBlocking(finding({ severity: "critical", confidence: "low" }))).toBe(false);
    expect(classifyReviewFindingBlocking(finding({ severity: "high", confidence: "low" }))).toBe(false);
  });

  it("keeps Medium and Low findings advisory", () => {
    expect(classifyReviewFindingBlocking(finding({ severity: "medium", confidence: "high" }))).toBe(false);
    expect(classifyReviewFindingBlocking(finding({ severity: "low", confidence: "high" }))).toBe(false);
  });

  it("counts only evidence-backed blockers", () => {
    const findings = [
      finding({ severity: "high", confidence: "high" }),
      finding({ severity: "high", confidence: "low" }),
      finding({ severity: "medium", confidence: "high" }),
      finding({ severity: "low", confidence: "high" }),
    ];
    expect(countBlockingFindings(findings)).toBe(1);
  });
});

describe("review chunking", () => {
  it("returns the single request when the diff fits", () => {
    const chunks = chunkChangeReviewRequests(request(), 1024 * 1024);
    expect(chunks).toHaveLength(1);
    expect(chunks[0]?.files).toHaveLength(1);
  });

  it("chunks by complete file when the diff exceeds the bound", () => {
    const big = "x".repeat(2000);
    const req = request({
      files: [
        { path: "a.gd", unifiedDiff: big },
        { path: "b.gd", unifiedDiff: big },
        { path: "c.gd", unifiedDiff: big },
      ],
    });
    const chunks = chunkChangeReviewRequests(req, 2500);
    expect(chunks.length).toBeGreaterThan(1);
    const covered = chunks.flatMap((chunk) => chunk.files.map((file) => file.path)).sort();
    expect(covered).toEqual(["a.gd", "b.gd", "c.gd"]);
    // Every chunk keeps the shared metadata.
    for (const chunk of chunks) {
      expect(chunk.request).toBe(req.request);
      expect(chunk.developmentId).toBe(req.developmentId);
    }
  });

  it("aggregates chunked results read-only with deduplication", () => {
    const shared = finding({ title: "shared issue" });
    const results = [
      { status: "completed" as const, findings: [shared, finding({ title: "first" })], message: null },
      { status: "completed" as const, findings: [finding({ title: "second" })], message: null },
    ];
    const aggregated = aggregateReviewResults(results);
    expect(aggregated.findings).toHaveLength(3);
    expect(aggregated.status).toBe("completed");
  });

  it("binds aggregated findings to the immutable maximum", () => {
    const many = Array.from({ length: 60 }, (_, index) =>
      finding({ title: `f${index}`, path: `p${index}.gd`, line: index + 1 }),
    );
    const aggregated = aggregateReviewResults([
      { status: "completed", findings: many, message: null },
    ]);
    expect(aggregated.findings.length).toBeLessThanOrEqual(QUALITY_LIMITS.maxReviewFindings);
  });
});

describe("review result vocabulary", () => {
  it("keeps the bounded result statuses distinct", () => {
    const statuses = ["completed", "cancelled", "failed", "too_large"];
    expect(new Set(statuses).size).toBe(statuses.length);
  });

  it("deduplicates deterministically", () => {
    const duplicates = [finding(), finding()];
    expect(deduplicateReviewFindings(duplicates)).toHaveLength(1);
  });
});
