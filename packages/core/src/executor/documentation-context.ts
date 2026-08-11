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
  const adrs = collect("adr", true);
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
  const boundedAdrs = drop(adrs, DOCUMENTATION_BUDGET.maxAdrs, "adr");
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
