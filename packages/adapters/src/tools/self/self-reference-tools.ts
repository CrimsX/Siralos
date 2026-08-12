import type {
  SelfReferencePort,
  Tool,
  ToolDefinition,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@siralos/core";
import {
  readJsonObject,
  readOptionalString,
  readRequiredString,
  type ParsedValue,
} from "../workspace/validation.js";

/**
 * `self.read` / `self.search` tools (Stage 3 milestone 6): read-only
 * inspection of the built-in `@siralos` self-reference — the host-owned
 * description of the EXACT installed runtime (version, commands,
 * configuration surface, capabilities, sandbox profiles, tool surface,
 * Godot capability status, references/research configuration, Task
 * Runtime concepts).
 *
 * The self-reference is retrieved on demand (never injected into
 * prompts), carries no secrets and no absolute sensitive paths, and has
 * no mutation surface. Both tools are read-only in every built-in
 * profile; they grant no authority and are NOT external References
 * (which are untrusted supporting material).
 */

export type SelfReferenceTool = Tool & { readonly capability: "self.inspect" };

const MAX_READ_LINES = 250;
const MAX_SEARCH_SECTIONS = 8;
const MAX_SEARCH_LINES_PER_SECTION = 20;

const SELF_READ_DEFINITION: ToolDefinition = {
  name: "self.read",
  description:
    "Read one section of the built-in @siralos self-reference (installed runtime documentation): runtime, commands, configuration, capabilities, sandbox, workspace-tools, godot, references, research, tasks, doctor. Read-only; contains no secrets.",
  inputSchema: {
    type: "object",
    properties: {
      section: {
        type: "string",
        description: "Self-reference section id (e.g. commands, capabilities, sandbox).",
      },
    },
    required: ["section"],
    additionalProperties: false,
  },
};

const SELF_SEARCH_DEFINITION: ToolDefinition = {
  name: "self.search",
  description:
    "Search the built-in @siralos self-reference (installed runtime documentation) for a topic such as 'commands', 'sandbox profile', 'Godot LSP', or 'checkpoints'. Read-only; bounded results.",
  inputSchema: {
    type: "object",
    properties: {
      query: {
        type: "string",
        description: "Search terms (case-insensitive token match).",
      },
    },
    required: ["query"],
    additionalProperties: false,
  },
};

/**
 * Static self-tool metadata, usable WITHOUT a port — the composition root
 * registers these definitions into the self-reference's tool surface, then
 * wraps the built SelfReference into executable tools. No duplication: the
 * executable tools below reuse exactly these definitions.
 */
export const SELF_REFERENCE_TOOL_METADATA: readonly {
  readonly definition: ToolDefinition;
  readonly capability: "self.inspect";
}[] = [
  { definition: SELF_READ_DEFINITION, capability: "self.inspect" },
  { definition: SELF_SEARCH_DEFINITION, capability: "self.inspect" },
];

interface ReadInput {
  readonly section: string;
}

function parseReadInput(input: unknown): ParsedValue<ReadInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const section = readRequiredString(object.value, "section");
  if (!section.ok) {
    return section;
  }
  return { ok: true, value: { section: section.value } };
}

export function createSelfReferenceReadTool(port: SelfReferencePort): SelfReferenceTool {
  return {
    definition: SELF_READ_DEFINITION,
    capability: "self.inspect",
    execute(input: unknown, _context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseReadInput(input);
      if (!parsed.ok) {
        return Promise.resolve({ status: "invalid_input", message: parsed.message });
      }
      const section = port.readSection(parsed.value.section as never);
      if (section === null) {
        return Promise.resolve({
          status: "invalid_input",
          message:
            "Unknown self-reference section. Sections: runtime, commands, configuration, capabilities, sandbox, workspace-tools, godot, references, research, tasks, doctor.",
        });
      }
      return Promise.resolve({
        status: "success",
        summary: `Read self-reference section ${section.id} (${section.lines.length} lines).`,
        output: {
          name: port.name,
          sectionId: section.id,
          title: section.title,
          lines: section.lines
            .slice(0, MAX_READ_LINES)
            .map((entry) => ({ key: entry.key, value: entry.value })),
          truncated: section.lines.length > MAX_READ_LINES,
        },
      });
    },
  };
}

interface SearchInput {
  readonly query: string;
}

function parseSearchInput(input: unknown): ParsedValue<SearchInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const query = readOptionalString(object.value, "query");
  if (query !== undefined) {
    if (!query.ok) {
      return query;
    }
    return { ok: true, value: { query: query.value ?? "" } };
  }
  return { ok: true, value: { query: "" } };
}

export function createSelfReferenceSearchTool(port: SelfReferencePort): SelfReferenceTool {
  return {
    definition: SELF_SEARCH_DEFINITION,
    capability: "self.inspect",
    execute(input: unknown, _context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseSearchInput(input);
      if (!parsed.ok) {
        return Promise.resolve({ status: "invalid_input", message: parsed.message });
      }
      const matches = port.search(parsed.value.query).slice(0, MAX_SEARCH_SECTIONS);
      const total = matches.reduce((sum, match) => sum + match.lines.length, 0);
      return Promise.resolve({
        status: "success",
        summary: `Found ${total} matching line${total === 1 ? "" : "s"} across ${matches.length} self-reference section${matches.length === 1 ? "" : "s"}.`,
        output: {
          name: port.name,
          matches: matches.map((match) => ({
            sectionId: match.sectionId,
            title: match.title,
            lines: match.lines
              .slice(0, MAX_SEARCH_LINES_PER_SECTION)
              .map((entry) => ({ key: entry.key, value: entry.value })),
          })),
          truncated: total > MAX_SEARCH_SECTIONS * MAX_SEARCH_LINES_PER_SECTION,
        },
      });
    },
  };
}

export function createSelfReferenceTools(port: SelfReferencePort): readonly SelfReferenceTool[] {
  return [createSelfReferenceReadTool(port), createSelfReferenceSearchTool(port)];
}
