import { deepFreeze } from "../domain/deep-freeze.js";
import { pathMatchesPattern } from "./new-file-discipline.js";

/**
 * Documentation context selection (harness context optimization,
 * ADR 0023; milestone Parts O, S, and T).
 *
 * Executor guidance is selected deterministically — path/domain mapping,
 * never semantic search or recursive docs-tree ingestion:
 *
 *   root AGENTS.md
 *   -> applicable nested AGENTS.md (path-scoped)
 *   -> architecture index (mapped subsystem docs)
 *   -> mapped accepted ADRs
 *   -> additional docs only when evidence says necessary
 *
 * Superseded/deprecated documents and archived material are excluded from
 * ordinary selection. A documentation budget bounds the selection; when
 * over budget, applicable guidance survives before historical/background
 * material. The selection is DERIVED context — it never grants capability
 * and never becomes a policy store; the index below is documentation
 * metadata, not runtime policy.
 */

export type DocumentationKind =
  "root-agents" | "nested-agents" | "architecture" | "adr" | "development";

export type DocumentationStatus = "accepted" | "superseded" | "deprecated";

export interface DocumentationEntry {
  /** Stable entry id (e.g. `agents:root`, `adr:0021`). */
  readonly id: string;
  /** Repository-relative doc path. */
  readonly path: string;
  readonly kind: DocumentationKind;
  /** Deterministic concern tags this entry covers (selection key). */
  readonly concerns: readonly string[];
  readonly status: DocumentationStatus;
  /** Set when this entry was superseded (by id). */
  readonly supersededBy?: string;
  /** Source-path globs this entry is scoped to (nested AGENTS.md, ADR metadata). */
  readonly paths?: readonly string[];
  /** ADR metadata domains (kind "adr" entries). */
  readonly domains?: readonly string[];
}

export interface DocumentationSelection {
  readonly rootAgents: readonly string[];
  readonly nestedAgents: readonly string[];
  readonly architectureDocs: readonly string[];
  readonly adrs: readonly string[];
  readonly developmentDocs: readonly string[];
  /** Entries dropped by the budget (observability for tests/review). */
  readonly dropped: readonly string[];
}

/** Host-owned documentation budget (Part T §40). */
export const DOCUMENTATION_BUDGET = Object.freeze({
  maxNestedAgents: 4,
  maxArchitectureDocs: 2,
  maxAdrs: 4,
  maxDevelopmentDocs: 2,
  maxSelected: 12,
});

/** Machine-selectable ADR metadata (Part O §31), parsed from frontmatter. */
export interface AdrMetadata {
  readonly id: string;
  readonly status: DocumentationStatus;
  readonly domains: readonly string[];
  readonly paths: readonly string[];
  readonly supersedes: readonly string[];
  readonly supersededBy?: string;
}

/** Archived documentation is excluded from ordinary selection (Part R §37). */
export const ARCHIVE_DOCUMENTATION_PREFIX = "docs/archive/";

export function isArchivedDocumentationPath(path: string): boolean {
  return (
    path.startsWith(ARCHIVE_DOCUMENTATION_PREFIX) ||
    path.startsWith(`./${ARCHIVE_DOCUMENTATION_PREFIX}`)
  );
}

/** Host-owned hard bounds for ADR metadata parsing. */
export const ADR_METADATA_LIMITS = Object.freeze({
  maxFrontmatterScanBytes: 4096,
  maxListEntries: 32,
  maxEntryBytes: 256,
  maxPathBytes: 1024,
});

const ADR_ID_PATTERN = /^ADR-\d{4}$/;
const STATUSES: readonly DocumentationStatus[] = ["accepted", "superseded", "deprecated"];
const textEncoder = new TextEncoder();

function boundedList(values: readonly string[], field: string): string[] {
  if (values.length > ADR_METADATA_LIMITS.maxListEntries) {
    throw new Error(
      `ADR metadata ${field} accepts at most ${ADR_METADATA_LIMITS.maxListEntries} entries.`,
    );
  }
  const seen = new Set<string>();
  return values.map((value) => {
    const text = value.trim();
    if (text.length === 0) {
      throw new Error(`ADR metadata ${field} entries must not be empty.`);
    }
    if (textEncoder.encode(text).length > ADR_METADATA_LIMITS.maxEntryBytes) {
      throw new Error(
        `ADR metadata ${field} entries exceed ${ADR_METADATA_LIMITS.maxEntryBytes} UTF-8 bytes.`,
      );
    }
    if (seen.has(text)) {
      throw new Error(`ADR metadata ${field} contains a duplicate: ${text}`);
    }
    seen.add(text);
    return text;
  });
}

function parseInlineList(value: string): string[] | null {
  const text = value.trim();
  if (!text.startsWith("[") || !text.endsWith("]")) {
    return null;
  }
  const inner = text.slice(1, -1).trim();
  if (inner.length === 0) {
    return [];
  }
  return inner.split(",").map((entry) => entry.trim());
}

/**
 * Deterministic ADR frontmatter parser. Accepts the repository's
 * metadata block:
 *
 *   ---
 *   id: ADR-0022
 *   status: accepted
 *   domains: [executor-briefing, context]
 *   paths: [packages/core/src/executor/**]
 *   supersedes: []
 *   ---
 *
 * Returns null when the document carries no frontmatter; throws on a
 * malformed block. Unknown keys are ignored (host-owned docs may grow
 * metadata); required keys are validated by `validateAdrMetadata`.
 */
export function parseAdrFrontmatter(markdown: string): AdrMetadata | null {
  const head = markdown.slice(0, ADR_METADATA_LIMITS.maxFrontmatterScanBytes);
  if (!head.startsWith("---")) {
    return null;
  }
  const firstLineEnd = head.indexOf("\n");
  if (firstLineEnd < 0 || head.slice(0, firstLineEnd).trim() !== "---") {
    return null;
  }
  const closeIndex = head.indexOf("\n---", firstLineEnd + 1);
  if (closeIndex < 0) {
    throw new Error("Malformed ADR frontmatter: no closing `---` line.");
  }
  const block = head.slice(firstLineEnd + 1, closeIndex);
  const fields = new Map<string, string[]>();
  let currentKey: string | null = null;
  for (const rawLine of block.split("\n")) {
    const line = rawLine.replace(/\r$/, "");
    if (line.trim().length === 0) {
      continue;
    }
    const listItem = line.match(/^\s*-\s+(.+)$/);
    if (listItem !== null) {
      if (currentKey === null) {
        throw new Error("Malformed ADR frontmatter: list item before any key.");
      }
      fields.get(currentKey)!.push(listItem[1]!.trim());
      continue;
    }
    const colon = line.indexOf(":");
    if (colon < 0) {
      throw new Error(`Malformed ADR frontmatter line: ${line}`);
    }
    const key = line.slice(0, colon).trim();
    const value = line.slice(colon + 1).trim();
    if (key.length === 0) {
      throw new Error("Malformed ADR frontmatter: empty key.");
    }
    currentKey = key;
    if (value.length === 0) {
      fields.set(key, []);
    } else {
      const inline = parseInlineList(value);
      fields.set(key, inline === null ? [value] : inline);
    }
  }
  const scalar = (key: string): string | undefined => {
    const values = fields.get(key);
    return values === undefined || values.length === 0 ? undefined : values[0];
  };
  const supersededBy = scalar("supersededBy");
  return validateAdrMetadata({
    id: scalar("id") ?? "",
    status: (scalar("status") ?? "accepted") as DocumentationStatus,
    domains: fields.get("domains") ?? [],
    paths: fields.get("paths") ?? [],
    supersedes: fields.get("supersedes") ?? [],
    ...(supersededBy === undefined ? {} : { supersededBy }),
  });
}

/** Validate and detach ADR metadata at a runtime boundary. */
export function validateAdrMetadata(input: AdrMetadata): AdrMetadata {
  if (!ADR_ID_PATTERN.test(input.id)) {
    throw new Error(`Invalid ADR metadata id: ${input.id}`);
  }
  if (!STATUSES.includes(input.status)) {
    throw new Error(`Invalid ADR metadata status: ${String(input.status)}`);
  }
  const domains = boundedList(input.domains, "domains");
  const paths = boundedList(input.paths, "paths").map((path) => {
    if (textEncoder.encode(path).length > ADR_METADATA_LIMITS.maxPathBytes) {
      throw new Error(`ADR metadata paths exceed ${ADR_METADATA_LIMITS.maxPathBytes} UTF-8 bytes.`);
    }
    return path;
  });
  const supersedes = boundedList(input.supersedes, "supersedes");
  const supersededBy = input.supersededBy?.trim();
  if (supersededBy !== undefined) {
    if (!ADR_ID_PATTERN.test(supersededBy)) {
      throw new Error(`Invalid ADR metadata supersededBy: ${supersededBy}`);
    }
  }
  return deepFreeze({
    id: input.id,
    status: input.status,
    domains,
    paths,
    supersedes,
    ...(supersededBy === undefined ? {} : { supersededBy }),
  });
}

/**
 * ADR entries in the documentation index, with machine-selectable
 * metadata (id, status, domains, source paths). This is the single
 * runtime source for ADR selection; the ADR files' frontmatter must stay
 * consistent with it (validated by the docs-consistency check).
 */
export const ADR_DOCUMENTATION_ENTRIES: readonly DocumentationEntry[] = deepFreeze([
  {
    id: "adr:0001",
    path: "docs/adr/0001-modular-monolith.md",
    kind: "adr",
    concerns: ["architecture"],
    status: "accepted",
    domains: ["architecture"],
    paths: ["packages/**"],
  },
  {
    id: "adr:0002",
    path: "docs/adr/0002-provider-neutral-tool-loop.md",
    kind: "adr",
    concerns: ["provider", "tool-loop", "projection"],
    status: "accepted",
    domains: ["provider", "tool-loop"],
    paths: ["packages/core/src/ports/**", "packages/adapters/src/providers/**"],
  },
  {
    id: "adr:0004",
    path: "docs/adr/0004-sandbox-and-permission-boundary.md",
    kind: "adr",
    concerns: ["security", "sandbox", "capability"],
    status: "accepted",
    domains: ["security", "sandbox"],
    paths: ["packages/core/src/security/**"],
  },
  {
    id: "adr:0005",
    path: "docs/adr/0005-approved-workspace-mutations.md",
    kind: "adr",
    concerns: ["workspace", "mutations"],
    status: "accepted",
    domains: ["workspace", "mutations"],
    paths: ["packages/core/src/workspace/**", "packages/adapters/src/tools/workspace/**"],
  },
  {
    id: "adr:0006",
    path: "docs/adr/0006-git-inspection-and-file-checkpoints.md",
    kind: "adr",
    concerns: ["git", "checkpoints"],
    status: "accepted",
    domains: ["git", "checkpoints"],
    paths: ["packages/adapters/src/git/**", "packages/adapters/src/checkpoints/**"],
  },
  {
    id: "adr:0007",
    path: "docs/adr/0007-sandboxed-validation-command-runners.md",
    kind: "adr",
    concerns: ["process", "sandbox"],
    status: "accepted",
    domains: ["process", "sandbox"],
    paths: ["packages/adapters/src/process/**"],
  },
  {
    id: "adr:0008",
    path: "docs/adr/0008-godot-discovery-and-static-project-profiling.md",
    kind: "adr",
    concerns: ["godot", "discovery"],
    status: "accepted",
    domains: ["godot", "discovery"],
    paths: ["packages/adapters/src/godot/**"],
  },
  {
    id: "adr:0009",
    path: "docs/adr/0009-disposable-recovery-mode-project-probing.md",
    kind: "adr",
    concerns: ["godot", "recovery"],
    status: "accepted",
    domains: ["godot", "recovery"],
    paths: ["packages/adapters/src/godot/**"],
  },
  {
    id: "adr:0010",
    path: "docs/adr/0010-version-matched-godot-knowledge-and-gdscript-diagnostics.md",
    kind: "adr",
    concerns: ["godot", "knowledge", "diagnostics"],
    status: "accepted",
    domains: ["godot", "knowledge", "diagnostics"],
    paths: ["packages/core/src/godot/**", "packages/adapters/src/godot/**"],
  },
  {
    id: "adr:0011",
    path: "docs/adr/0011-bounded-godot-gdscript-lsp-client.md",
    kind: "adr",
    concerns: ["godot", "lsp"],
    status: "accepted",
    domains: ["godot", "lsp"],
    paths: ["packages/adapters/src/godot/**"],
  },
  {
    id: "adr:0012",
    path: "docs/adr/0012-gdscript-development-and-repair-loop.md",
    kind: "adr",
    concerns: ["godot", "development"],
    status: "accepted",
    domains: ["godot", "development"],
    paths: ["packages/core/src/godot/**", "packages/adapters/src/godot/**"],
  },
  {
    id: "adr:0013",
    path: "docs/adr/0013-gdscript-quality-gates-and-independent-review.md",
    kind: "adr",
    concerns: ["godot", "quality", "review"],
    status: "accepted",
    domains: ["godot", "quality", "review"],
    paths: ["packages/core/src/godot/**"],
  },
  {
    id: "adr:0014",
    path: "docs/adr/0014-task-runtime-foundation.md",
    kind: "adr",
    concerns: ["task-runtime", "evidence"],
    status: "accepted",
    domains: ["task-runtime", "evidence"],
    paths: ["packages/core/src/tasks/**"],
  },
  {
    id: "adr:0015",
    path: "docs/adr/0015-context-tool-evidence-projection.md",
    kind: "adr",
    concerns: ["projection", "context", "evidence"],
    status: "accepted",
    domains: ["projection", "context"],
    paths: ["packages/core/src/projection/**"],
  },
  {
    id: "adr:0016",
    path: "docs/adr/0016-workspace-revision-and-structural-reads.md",
    kind: "adr",
    concerns: ["workspace-revision", "workspace", "evidence"],
    status: "accepted",
    domains: ["workspace", "revisions"],
    paths: ["packages/core/src/workspace/**"],
  },
  {
    id: "adr:0017",
    path: "docs/adr/0017-project-instructions-and-knowledge.md",
    kind: "adr",
    concerns: ["instructions", "knowledge"],
    status: "accepted",
    domains: ["instructions", "knowledge"],
    paths: ["packages/core/src/instructions/**", "packages/core/src/knowledge/**"],
  },
  {
    id: "adr:0018",
    path: "docs/adr/0018-external-references-and-research-sources.md",
    kind: "adr",
    concerns: ["references", "research"],
    status: "accepted",
    domains: ["references", "research"],
    paths: ["packages/core/src/reference/**", "packages/core/src/research/**"],
  },
  {
    id: "adr:0019",
    path: "docs/adr/0019-self-reference-and-capability-diagnostics.md",
    kind: "adr",
    concerns: ["capability", "self-reference", "doctor"],
    status: "accepted",
    domains: ["self-reference", "capability", "doctor"],
    paths: ["packages/core/src/self/**", "packages/core/src/doctor/**"],
  },
  {
    id: "adr:0020",
    path: "docs/adr/0020-host-controlled-planning-foundation.md",
    kind: "adr",
    concerns: ["planning"],
    status: "accepted",
    domains: ["planning"],
    paths: ["packages/core/src/planning/**"],
  },
  {
    id: "adr:0021",
    path: "docs/adr/0021-read-only-godot-scene-resource-intelligence.md",
    kind: "adr",
    concerns: ["godot-static-inspection", "godot", "read-only"],
    status: "accepted",
    domains: ["godot", "static-inspection"],
    paths: ["packages/core/src/godot/**", "packages/adapters/src/godot/**"],
  },
  {
    id: "adr:0022",
    path: "docs/adr/0022-structured-executor-briefing-and-milestone-acceptance.md",
    kind: "adr",
    concerns: ["executor-briefing", "context"],
    status: "accepted",
    domains: ["executor-briefing", "context"],
    paths: ["packages/core/src/executor/**"],
  },
  {
    id: "adr:0023",
    path: "docs/adr/0023-workspace-scope-and-documentation-context-discipline.md",
    kind: "adr",
    concerns: ["executor-briefing", "context", "workspace-scope", "documentation"],
    status: "accepted",
    domains: ["executor-briefing", "context", "workspace-scope", "documentation"],
    paths: ["packages/core/src/executor/**"],
  },
  {
    id: "adr:0024",
    path: "docs/adr/0024-scoped-documentation-and-progressive-context.md",
    kind: "adr",
    concerns: ["documentation", "context", "executor-briefing"],
    status: "accepted",
    domains: ["documentation", "context", "executor-briefing"],
    paths: ["packages/core/src/executor/**", "docs/architecture/**", "AGENTS.md"],
  },
  {
    id: "adr:0025",
    path: "docs/adr/0025-godot-review-context-and-impact-intelligence.md",
    kind: "adr",
    concerns: ["godot", "godot-static-inspection", "review", "impact"],
    status: "accepted",
    domains: ["godot", "impact", "review"],
    paths: ["packages/core/src/godot/impact/**", "packages/adapters/src/godot/intelligence/**"],
  },
  {
    id: "adr:0026",
    path: "docs/adr/0026-approved-scene-and-resource-mutation.md",
    kind: "adr",
    concerns: ["godot", "godot-static-inspection", "workspace", "security"],
    status: "accepted",
    domains: ["godot", "mutation", "security"],
    paths: [
      "packages/core/src/godot/scene-mutation/**",
      "packages/adapters/src/godot/scene-mutation/**",
    ],
  },
  {
    id: "adr:0027",
    path: "docs/adr/0027-unified-godot-native-development-workflow.md",
    kind: "adr",
    concerns: ["godot", "workflow", "task-runtime", "security"],
    status: "accepted",
    domains: ["godot", "workflow", "mutation", "task-runtime", "security"],
    paths: [
      "packages/core/src/godot/development/development-surface.ts",
      "packages/core/src/godot/development/unified-change-set.ts",
      "packages/core/src/godot/development/unified-order.ts",
      "packages/core/src/godot/development/cross-surface-consistency.ts",
      "packages/core/src/godot/development/blocked-disposition.ts",
      "packages/adapters/src/godot/development/unified-development-service.ts",
      "packages/core/src/executor/s3m11-manifest.ts",
    ],
  },
]);

/**
 * The deterministic documentation index: root guidance, the architecture
 * documents, engineering rules, and the ADR set. This is documentation
 * metadata for selection — never a policy store and never authority.
 */
export const DOCUMENTATION_INDEX: readonly DocumentationEntry[] = deepFreeze([
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
    concerns: ["task-runtime", "workspace", "projection", "planning", "executor-briefing"],
    status: "accepted",
    paths: ["packages/core/**"],
  },
  {
    id: "agents:adapters",
    path: "packages/adapters/AGENTS.md",
    kind: "nested-agents",
    concerns: ["provider", "sandbox", "workspace", "git", "checkpoints", "references", "research"],
    status: "accepted",
    paths: ["packages/adapters/**"],
  },
  {
    id: "agents:godot",
    path: "packages/adapters/src/godot/AGENTS.md",
    kind: "nested-agents",
    concerns: [
      "godot",
      "godot-static-inspection",
      "discovery",
      "knowledge",
      "lsp",
      "development",
      "recovery",
    ],
    status: "accepted",
    paths: ["packages/adapters/src/godot/**"],
  },
  {
    id: "agents:cli",
    path: "apps/cli/AGENTS.md",
    kind: "nested-agents",
    concerns: ["cli", "composition-root"],
    status: "accepted",
    paths: ["apps/cli/**"],
  },
  {
    id: "arch:architecture",
    path: "ARCHITECTURE.md",
    kind: "architecture",
    concerns: [
      "architecture",
      "task-runtime",
      "workspace",
      "security",
      "sandbox",
      "capability",
      "provider",
      "tool-loop",
      "projection",
      "context",
      "godot",
      "godot-static-inspection",
      "knowledge",
      "references",
      "research",
      "planning",
      "executor-briefing",
    ],
    status: "accepted",
  },
  {
    id: "arch:security",
    path: "SECURITY.md",
    kind: "architecture",
    concerns: ["security", "sandbox", "capability"],
    status: "accepted",
  },
  {
    id: "dev:engineering",
    path: "ENGINEERING.md",
    kind: "development",
    concerns: [
      "engineering",
      "validation",
      "testing",
      "git",
      "task-runtime",
      "planning",
      "projection",
      "workspace",
      "instructions",
      "knowledge",
      "references",
      "research",
      "self-reference",
      "godot",
      "security",
    ],
    status: "accepted",
  },
  ...ADR_DOCUMENTATION_ENTRIES,
]);

export interface SelectDocumentationContextInput {
  /** Deterministic concern tags (from the milestone manifest / task). */
  readonly concerns: readonly string[];
  /** Task-relevant workspace-relative paths for nested-AGENTS scoping. */
  readonly paths?: readonly string[];
  /** Index override (behavior fixtures inject their own doc trees). */
  readonly index?: readonly DocumentationEntry[];
}

/**
 * Deterministic selection in canonical order: root AGENTS.md always;
 * nested AGENTS.md scoped to the task's paths; architecture docs and
 * accepted ADRs matching the concerns; development docs last. Superseded,
 * deprecated, and archived material is excluded. Bounded by the
 * documentation budget; drops are recorded.
 */
export function selectDocumentationContext(
  input: SelectDocumentationContextInput,
): DocumentationSelection {
  const index = input.index ?? DOCUMENTATION_INDEX;
  const wanted = new Set(input.concerns);
  const taskPaths = input.paths ?? [];
  const collect = (kind: DocumentationKind, concernFilter: boolean): string[] => {
    const paths: string[] = [];
    for (const entry of index) {
      if (entry.kind !== kind) {
        continue;
      }
      if (entry.status !== "accepted" || isArchivedDocumentationPath(entry.path)) {
        continue;
      }
      if (concernFilter && !entry.concerns.some((concern) => wanted.has(concern))) {
        continue;
      }
      paths.push(entry.path);
    }
    return paths;
  };
  const rootAgents = collect("root-agents", false);
  const nestedAgents = collect("nested-agents", false).filter((path) => {
    const entry = index.find((candidate) => candidate.path === path);
    const patterns = entry?.paths ?? [];
    return (
      patterns.length === 0 ||
      taskPaths.some((taskPath) =>
        patterns.some((pattern) => pathMatchesPattern(taskPath, pattern)),
      )
    );
  });
  const architectureDocs = collect("architecture", true);
  // ADR candidates are ordered by concern overlap with the requested
  // concerns (most-specific first); ties keep the canonical index order.
  // The documentation budget then keeps the most relevant current ADRs.
  const adrCandidates = index.filter(
    (entry) =>
      entry.kind === "adr" &&
      entry.status === "accepted" &&
      !isArchivedDocumentationPath(entry.path) &&
      entry.concerns.some((concern) => wanted.has(concern)),
  );
  const adrOrdered = [...adrCandidates]
    .map((entry) => ({
      path: entry.path,
      overlap: entry.concerns.filter((concern) => wanted.has(concern)).length,
    }))
    .sort((a, b) => b.overlap - a.overlap)
    .map((entry) => entry.path);
  const developmentDocs = collect("development", true);
  const dropped: string[] = [];
  const drop = (list: string[], max: number, listName: string): string[] => {
    if (list.length <= max) {
      return list;
    }
    dropped.push(...list.slice(max).map((path) => `${listName}:${path}`));
    return list.slice(0, max);
  };
  const boundedNested = drop(nestedAgents, DOCUMENTATION_BUDGET.maxNestedAgents, "nested");
  const boundedArchitecture = drop(
    architectureDocs,
    DOCUMENTATION_BUDGET.maxArchitectureDocs,
    "architecture",
  );
  const boundedAdrs = drop(adrOrdered, DOCUMENTATION_BUDGET.maxAdrs, "adr");
  const boundedDevelopment = drop(
    developmentDocs,
    DOCUMENTATION_BUDGET.maxDevelopmentDocs,
    "development",
  );
  return deepFreeze({
    rootAgents,
    nestedAgents: boundedNested,
    architectureDocs: boundedArchitecture,
    adrs: boundedAdrs,
    developmentDocs: boundedDevelopment,
    dropped,
  });
}
