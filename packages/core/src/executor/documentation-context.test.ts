import { describe, expect, it } from "vitest";
import {
  DOCUMENTATION_INDEX,
  DOCUMENTATION_BUDGET,
  isArchivedDocumentationPath,
  parseAdrFrontmatter,
  selectDocumentationContext,
  validateAdrMetadata,
  type DocumentationEntry,
} from "./documentation-context.js";

const SAMPLE_ADR = `---
id: ADR-0021
status: accepted
domains: [godot, static-inspection]
paths: [packages/core/src/godot/**]
supersedes: []
---

# ADR 0021 — Read-Only Godot Scene and Resource Intelligence
`;

describe("ADR metadata", () => {
  it("parses machine-selectable frontmatter deterministically", () => {
    const metadata = parseAdrFrontmatter(SAMPLE_ADR);
    expect(metadata).toEqual({
      id: "ADR-0021",
      status: "accepted",
      domains: ["godot", "static-inspection"],
      paths: ["packages/core/src/godot/**"],
      supersedes: [],
    });
  });

  it("returns null for documents without frontmatter", () => {
    expect(parseAdrFrontmatter("# Plain document")).toBeNull();
  });

  it("rejects malformed frontmatter", () => {
    expect(() => parseAdrFrontmatter("---\nid: ADR-0001\n")).toThrow(/no closing/);
    expect(() => parseAdrFrontmatter("---\nid: nope\nstatus: accepted\n---")).toThrow(
      /Invalid ADR metadata id/,
    );
    expect(() => parseAdrFrontmatter("---\nid: ADR-0001\nstatus: obsolete\n---")).toThrow(
      /Invalid ADR metadata status/,
    );
  });

  it("accepts block-list frontmatter as well as inline lists", () => {
    const metadata = parseAdrFrontmatter(`---
id: ADR-0014
status: accepted
domains:
  - task-runtime
  - evidence
paths: [packages/core/src/tasks/**]
supersedes: []
---`);
    expect(metadata?.domains).toEqual(["task-runtime", "evidence"]);
  });

  it("validates superseded metadata including supersededBy", () => {
    const metadata = validateAdrMetadata({
      id: "ADR-0003",
      status: "superseded",
      domains: ["old"],
      paths: [],
      supersedes: [],
      supersededBy: "ADR-0005",
    });
    expect(metadata.status).toBe("superseded");
    expect(metadata.supersededBy).toBe("ADR-0005");
  });
});

describe("documentation context selection", () => {
  it("always selects root AGENTS.md and maps domain concerns to the right docs", () => {
    const selection = selectDocumentationContext({ concerns: ["task-runtime"] });
    expect(selection.rootAgents).toEqual(["AGENTS.md"]);
    expect(selection.architectureDocs).toContain("ARCHITECTURE.md");
    expect(selection.adrs).toContain("docs/adr/0014-task-runtime-foundation.md");
    expect(selection.adrs).not.toContain(
      "docs/adr/0018-external-references-and-research-sources.md",
    );
  });

  it("does not recursively ingest the docs tree: unrelated material is absent", () => {
    const selection = selectDocumentationContext({ concerns: ["planning"] });
    const selected = [
      ...selection.rootAgents,
      ...selection.nestedAgents,
      ...selection.architectureDocs,
      ...selection.adrs,
      ...selection.developmentDocs,
    ];
    expect(selected).toContain("AGENTS.md");
    expect(selected).toContain("docs/adr/0020-host-controlled-planning-foundation.md");
    // Unrelated ADRs (references/research) are not selected for a planning task.
    expect(selected).not.toContain("docs/adr/0018-external-references-and-research-sources.md");
  });

  it("selects nested AGENTS.md only when path-scoped to the task", () => {
    const index: readonly DocumentationEntry[] = [
      {
        id: "agents:root",
        path: "AGENTS.md",
        kind: "root-agents",
        concerns: [],
        status: "accepted",
      },
      {
        id: "agents:core",
        path: "packages/core/AGENTS.md",
        kind: "nested-agents",
        concerns: [],
        status: "accepted",
        paths: ["packages/core/**"],
      },
      {
        id: "agents:cli",
        path: "apps/cli/AGENTS.md",
        kind: "nested-agents",
        concerns: [],
        status: "accepted",
        paths: ["apps/cli/**"],
      },
    ];
    const forCore = selectDocumentationContext({
      concerns: [],
      paths: ["packages/core/src/executor/context-pack.ts"],
      index,
    });
    expect(forCore.nestedAgents).toEqual(["packages/core/AGENTS.md"]);
    const forCli = selectDocumentationContext({
      concerns: [],
      paths: ["apps/cli/src/index.ts"],
      index,
    });
    expect(forCli.nestedAgents).toEqual(["apps/cli/AGENTS.md"]);
  });

  it("excludes superseded and archived documentation by default", () => {
    const index: readonly DocumentationEntry[] = [
      {
        id: "agents:root",
        path: "AGENTS.md",
        kind: "root-agents",
        concerns: [],
        status: "accepted",
      },
      {
        id: "adr:old",
        path: "docs/adr/0003-superseded.md",
        kind: "adr",
        concerns: ["godot"],
        status: "superseded",
        supersededBy: "ADR-0005",
      },
      {
        id: "adr:archive",
        path: "docs/archive/historical-design.md",
        kind: "adr",
        concerns: ["godot"],
        status: "accepted",
      },
    ];
    const selection = selectDocumentationContext({ concerns: ["godot"], index });
    expect(selection.adrs).not.toContain("docs/adr/0003-superseded.md");
    expect(selection.adrs).not.toContain("docs/archive/historical-design.md");
    expect(isArchivedDocumentationPath("docs/archive/historical-design.md")).toBe(true);
    expect(
      isArchivedDocumentationPath("docs/adr/0021-read-only-godot-scene-resource-intelligence.md"),
    ).toBe(false);
  });

  it("applies the documentation budget: applicable guidance survives before background material", () => {
    const index: readonly DocumentationEntry[] = [
      {
        id: "agents:root",
        path: "AGENTS.md",
        kind: "root-agents",
        concerns: [],
        status: "accepted",
      },
      {
        id: "arch:main",
        path: "docs/architecture/main.md",
        kind: "architecture",
        concerns: ["godot"],
        status: "accepted",
      },
      ...Array.from({ length: 8 }, (_, i) => ({
        id: `adr:${String(i + 1).padStart(4, "0")}`,
        path: `docs/adr/00${i + 1}-unrelated-${i}.md`,
        kind: "adr" as const,
        concerns: ["godot"],
        status: "accepted" as const,
      })),
    ];
    const selection = selectDocumentationContext({ concerns: ["godot"], index });
    // Root guidance and direct architecture always survive.
    expect(selection.rootAgents).toEqual(["AGENTS.md"]);
    expect(selection.architectureDocs).toEqual(["docs/architecture/main.md"]);
    // ADRs are bounded; the overflow is recorded as dropped.
    expect(selection.adrs.length).toBe(DOCUMENTATION_BUDGET.maxAdrs);
    expect(selection.dropped.length).toBe(8 - DOCUMENTATION_BUDGET.maxAdrs);
  });

  it("is deterministic: identical inputs produce identical selections", () => {
    const a = selectDocumentationContext({
      concerns: ["security", "sandbox"],
      paths: ["packages/core/src/security/capability.ts"],
    });
    const b = selectDocumentationContext({
      concerns: ["security", "sandbox"],
      paths: ["packages/core/src/security/capability.ts"],
    });
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
  });

  it("the repository index is fully accepted documentation metadata, not policy", () => {
    for (const entry of DOCUMENTATION_INDEX) {
      expect(entry.status).toBe("accepted");
      expect(entry.path.length).toBeGreaterThan(0);
    }
    // ADR entries carry domains/paths metadata for deterministic mapping.
    const adrEntries = DOCUMENTATION_INDEX.filter((entry) => entry.kind === "adr");
    expect(adrEntries.length).toBeGreaterThanOrEqual(10);
    for (const entry of adrEntries) {
      expect(entry.domains?.length ?? 0).toBeGreaterThan(0);
      expect(entry.paths?.length ?? 0).toBeGreaterThan(0);
    }
  });
});
