import type {
  ReferenceEvidenceView,
  ReferenceRegistry,
  ReferenceRevision,
  ResearchFetchResult,
  ResearchRequest,
  ResearchService,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@solaris/core";
import type { ReferenceTool } from "@solaris/adapters";

/**
 * CLI-owned reference/research composition helpers (Stage 3 milestone 5).
 *
 * `createReferenceEvidenceRing` is the CLI's small bounded ring of recent
 * reference tool observations (≤ 4 entries), fed at the tool-registration
 * seam by `observeReferenceTools`. The ring is what the projection service
 * consumes for the volatile `[Reference evidence]` context section — the
 * registry stays the SINGLE owner of reference identity; the CLI only
 * mirrors what the tools already did.
 *
 * `createResearchTools` builds the two research tools
 * (`research.repository`, `research.godot_docs`) over the real
 * `ResearchService`. They are registered always; the ToolProjector hides
 * them under the default deny policy for `research.fetch`, and the service
 * itself refuses every fetch when the policy does not allow — the source
 * ports are never invoked.
 */

/** Maximum reference evidence observations retained by the CLI ring. */
export const MAX_REFERENCE_EVIDENCE_VIEWS = 4;

export interface ReferenceEvidenceRing {
  /** Bounded recent observations, oldest first. */
  list(): readonly ReferenceEvidenceView[];
  record(view: ReferenceEvidenceView): void;
}

export function createReferenceEvidenceRing(
  maxEntries: number = MAX_REFERENCE_EVIDENCE_VIEWS,
): ReferenceEvidenceRing {
  const entries: ReferenceEvidenceView[] = [];
  return {
    list(): readonly ReferenceEvidenceView[] {
      return [...entries];
    },
    record(view: ReferenceEvidenceView): void {
      entries.push(view);
      if (entries.length > maxEntries) {
        entries.splice(0, entries.length - maxEntries);
      }
    },
  };
}

/**
 * Wrap the reference tools so every successful execution records a bounded
 * observation into the ring. The tools' outputs already carry the alias
 * (`@reference/<alias>`), the reference-relative path, and (for reads) the
 * mode and SHA-256; the full revision and reference id come from the
 * registry, which stays the single owner of reference identity. Nothing is
 * resolved or refreshed here.
 */
export function observeReferenceTools(
  tools: readonly ReferenceTool[],
  registry: ReferenceRegistry,
  ring: ReferenceEvidenceRing,
): readonly ReferenceTool[] {
  return tools.map((tool) => {
    const observed: ReferenceTool = {
      ...tool,
      async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
        const result = await tool.execute(input, context);
        if (result.status === "success") {
          recordObservation(tool.definition.name, result.output, registry, ring);
        }
        return result;
      },
    };
    return observed;
  });
}

function recordObservation(
  toolName: string,
  output: unknown,
  registry: ReferenceRegistry,
  ring: ReferenceEvidenceRing,
): void {
  if (typeof output !== "object" || output === null || Array.isArray(output)) {
    return;
  }
  const record = output as Record<string, unknown>;
  const referenceValue = record["reference"];
  if (typeof referenceValue !== "string" || !referenceValue.startsWith("@reference/")) {
    return;
  }
  const alias = referenceValue.slice("@reference/".length);
  const reference = registry.get(alias as Parameters<ReferenceRegistry["get"]>[0]);
  if (reference === undefined) {
    return;
  }
  const revision: ReferenceRevision | null = registry.revision(reference.id);
  if (revision === null) {
    return;
  }
  const operation =
    toolName === "reference.read"
      ? ("read" as const)
      : toolName === "reference.search"
        ? ("search" as const)
        : ("list" as const);
  const path =
    typeof record["path"] === "string" && record["path"].length > 0 ? record["path"] : ".";
  const mode = typeof record["mode"] === "string" ? record["mode"] : null;
  const sha256 = typeof record["sha256"] === "string" ? record["sha256"] : null;
  ring.record({
    referenceId: reference.id,
    alias: reference.alias,
    revision,
    path,
    operation,
    mode,
    sha256,
    evidenceId: null,
  });
}

const GITHUB_SOURCE: ResearchRequest["source"] = {
  kind: "repository",
  id: "github",
  label: "GitHub repository research",
};

const GODOT_DOCS_SOURCE: ResearchRequest["source"] = {
  kind: "godot-docs",
  id: "godot-docs",
  label: "Godot official documentation",
};

/**
 * `research.repository` — bounded fetch of one GitHub repository file
 * (or latest release notes) through the ResearchService. The service gates
 * on the `research.fetch` capability BEFORE any source port is invoked.
 */
export function createResearchRepositoryTool(service: ResearchService): Tool {
  return {
    definition: {
      name: "research.repository",
      description:
        "Fetch one file from a public GitHub repository (owner/repo), or its latest release notes when no path is given. Bounded and read-only; gated by the research.fetch capability, which the default policy denies.",
      inputSchema: {
        type: "object",
        properties: {
          origin: {
            type: "string",
            minLength: 1,
            maxLength: 2048,
            description: "Repository origin: owner/repo or https://github.com/owner/repo.",
          },
          path: {
            type: "string",
            maxLength: 1024,
            description: "Repository-relative file path (forward slashes, no leading slash).",
          },
          ref: {
            type: "string",
            maxLength: 256,
            description: "Optional git ref pin (commit/tag/branch).",
          },
        },
        required: ["origin"],
        additionalProperties: false,
      },
    },
    capability: "research.fetch",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseRepositoryInput(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const request: ResearchRequest = {
        source: GITHUB_SOURCE,
        query: parsed.value.origin,
        topic: null,
        path: parsed.value.path,
        ref: parsed.value.ref,
        version: null,
        maxBytes: null,
      };
      return mapResearchResult(
        await service.fetch(request, {
          ...(context.signal === undefined ? {} : { signal: context.signal }),
        }),
      );
    },
  };
}

/**
 * `research.godot_docs` — bounded fetch of one Godot documentation page
 * (class page via `topic`, or the site search page) through the
 * ResearchService, with explicit version fallback disclosure.
 */
export function createResearchGodotDocsTool(service: ResearchService): Tool {
  return {
    definition: {
      name: "research.godot_docs",
      description:
        "Fetch one page from the official Godot documentation (docs.godotengine.org): a class page when topic is given, otherwise the search page for the query. Version fallbacks are disclosed in provenance. Bounded and read-only; gated by the research.fetch capability, which the default policy denies.",
      inputSchema: {
        type: "object",
        properties: {
          query: {
            type: "string",
            minLength: 1,
            maxLength: 512,
            description: "Search query (or the class name when topic is given).",
          },
          topic: {
            type: "string",
            maxLength: 256,
            description: "Optional class topic, e.g. CharacterBody2D.",
          },
          version: {
            type: "string",
            maxLength: 64,
            description: "Optional documentation version pin, e.g. 4.3 or 4.3-stable.",
          },
        },
        required: ["query"],
        additionalProperties: false,
      },
    },
    capability: "research.fetch",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseDocsInput(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const request: ResearchRequest = {
        source: GODOT_DOCS_SOURCE,
        query: parsed.value.query,
        topic: parsed.value.topic,
        path: null,
        ref: null,
        version: parsed.value.version,
        maxBytes: null,
      };
      return mapResearchResult(
        await service.fetch(request, {
          ...(context.signal === undefined ? {} : { signal: context.signal }),
        }),
      );
    },
  };
}

export function createResearchTools(service: ResearchService): readonly Tool[] {
  return [createResearchRepositoryTool(service), createResearchGodotDocsTool(service)];
}

function mapResearchResult(result: ResearchFetchResult): ToolExecutionResult {
  switch (result.status) {
    case "document": {
      const { document, evidence } = result;
      return {
        status: "success",
        output: {
          source: { ...document.source },
          title: document.title,
          sections: document.sections.map((section) => ({ ...section })),
          links: document.links.map((link) => ({ ...link })),
          provenance: { ...document.provenance, source: { ...document.provenance.source } },
          truncated: document.truncated,
          truncationReason: document.truncationReason,
          byteLength: document.byteLength,
          evidence: {
            evidenceId: evidence.evidenceId,
            requestId: evidence.requestId,
            resolvedRevision: evidence.resolvedRevision,
            version: evidence.version,
            fallback: evidence.fallback,
          },
        },
        summary: `${document.sections.length} section${document.sections.length === 1 ? "" : "s"}${document.truncated ? " (truncated)" : ""}`,
      };
    }
    case "refused":
      return { status: "denied", message: result.reason };
    case "unavailable":
      return { status: "unavailable", message: result.reason };
    case "timeout":
      return { status: "timed_out", message: result.reason };
    case "cancelled":
      return { status: "cancelled", message: result.reason };
    case "unsupported-content":
    case "oversized":
    case "failed":
      return { status: "failed", message: result.reason };
  }
}

type FieldResult<T> =
  { readonly ok: true; readonly value: T } | { readonly ok: false; readonly message: string };

type Parsed<T> = FieldResult<T>;

function parseObject(input: unknown): Parsed<Record<string, unknown>> {
  if (typeof input !== "object" || input === null || Array.isArray(input)) {
    return { ok: false, message: "Tool input must be a JSON object." };
  }
  return { ok: true, value: input as Record<string, unknown> };
}

function readOptionalString(
  record: Record<string, unknown>,
  key: string,
  maxLength: number,
): FieldResult<string | null> {
  const value = record[key];
  if (value === undefined || value === null) {
    return { ok: true, value: null };
  }
  if (typeof value !== "string" || value.length === 0 || value.length > maxLength) {
    return {
      ok: false,
      message: `"${key}" must be a non-empty string of at most ${maxLength} characters.`,
    };
  }
  return { ok: true, value };
}

function readRequiredString(
  record: Record<string, unknown>,
  key: string,
  maxLength: number,
): FieldResult<string> {
  const optional = readOptionalString(record, key, maxLength);
  if (!optional.ok) {
    return optional;
  }
  if (optional.value === null) {
    return { ok: false, message: `"${key}" is required.` };
  }
  return { ok: true, value: optional.value };
}

function rejectUnknownKeys(
  record: Record<string, unknown>,
  allowed: readonly string[],
): { readonly ok: true } | { readonly ok: false; readonly message: string } {
  for (const key of Object.keys(record)) {
    if (!allowed.includes(key)) {
      return { ok: false, message: `Unknown tool input key: ${key}.` };
    }
  }
  return { ok: true };
}

function parseRepositoryInput(
  input: unknown,
): Parsed<{ origin: string; path: string | null; ref: string | null }> {
  const object = parseObject(input);
  if (!object.ok) {
    return object;
  }
  const unknown = rejectUnknownKeys(object.value, ["origin", "path", "ref"]);
  if (!unknown.ok) {
    return unknown;
  }
  const origin = readRequiredString(object.value, "origin", 2048);
  if (!origin.ok) {
    return origin;
  }
  const path = readOptionalString(object.value, "path", 1024);
  if (!path.ok) {
    return path;
  }
  const ref = readOptionalString(object.value, "ref", 256);
  if (!ref.ok) {
    return ref;
  }
  return { ok: true, value: { origin: origin.value, path: path.value, ref: ref.value } };
}

function parseDocsInput(
  input: unknown,
): Parsed<{ query: string; topic: string | null; version: string | null }> {
  const object = parseObject(input);
  if (!object.ok) {
    return object;
  }
  const unknown = rejectUnknownKeys(object.value, ["query", "topic", "version"]);
  if (!unknown.ok) {
    return unknown;
  }
  const query = readRequiredString(object.value, "query", 512);
  if (!query.ok) {
    return query;
  }
  const topic = readOptionalString(object.value, "topic", 256);
  if (!topic.ok) {
    return topic;
  }
  const version = readOptionalString(object.value, "version", 64);
  if (!version.ok) {
    return version;
  }
  return { ok: true, value: { query: query.value, topic: topic.value, version: version.value } };
}
