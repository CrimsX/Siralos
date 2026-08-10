import type {
  GodotKnowledge,
  GodotApiSearchKind,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@solaris/core";
import { errorMessage } from "../../support/error-message.js";

/**
 * `godot.api_search` provider tool: bounded literal/token search over the
 * version-matched API knowledge base. Exact name matches rank first, then
 * prefix, token, and document matches; ordering is deterministic. The
 * provider can never receive the raw API dump or the raw index; results are
 * bounded and the query cannot change the engine profile.
 */
export function createGodotApiSearchTool(knowledge: GodotKnowledge): Tool {
  return {
    definition: {
      name: "godot.api_search",
      description:
        "Search the exact selected engine's API documentation: classes, methods, properties, signals, constants, enums, utility functions, and built-in class members. Literal/token search only; no internet, no embeddings. Results are bounded and ranked (exact name first, then prefix, then token, then document).",
      inputSchema: {
        type: "object",
        additionalProperties: false,
        properties: {
          query: { type: "string", description: 'Required search text (e.g. "Node owner").' },
          kinds: {
            type: "array",
            items: {
              type: "string",
              enum: [
                "class",
                "method",
                "property",
                "signal",
                "constant",
                "enum",
                "utility",
                "operator",
              ],
            },
            description: 'Optional kind filter (e.g. ["class", "property", "method"]).',
          },
          limit: {
            type: "integer",
            minimum: 1,
            description: "Optional result bound; capped by the immutable global limit.",
          },
        },
        required: ["query"],
      },
    },
    capability: "godot.api",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      try {
        const request = parseInput(input);
        if (!request.ok) {
          return { status: "invalid_input", message: request.message };
        }
        const result = await knowledge.search(
          {
            query: request.value.query,
            ...(request.value.kinds === undefined
              ? {}
              : { kinds: request.value.kinds as readonly GodotApiSearchKind[] }),
            ...(request.value.limit === undefined ? {} : { limit: request.value.limit }),
          },
          context.signal,
        );
        if (result.status === "ready") {
          return {
            status: "success",
            output: {
              engineVersion: result.engineVersion,
              results: result.results.map((entry) => ({
                symbol: entry.symbol,
                kind: entry.kind,
                name: entry.name,
                owner: entry.owner,
                summary: entry.summary,
                rank: entry.rank,
                apiType: entry.apiType,
              })),
              truncated: result.truncated,
            },
            summary: `${result.results.length} API result${result.results.length === 1 ? "" : "s"} for "${request.value.query}"`,
          };
        }
        return {
          status: result.status === "unsupported" ? "failed" : result.status,
          message: result.message,
        };
      } catch (error: unknown) {
        return {
          status: "failed",
          message: errorMessage(error, "An unknown Godot API search failure occurred."),
        };
      }
    },
  };
}

function parseInput(input: unknown):
  | {
      readonly ok: true;
      readonly value: {
        readonly query: string;
        readonly kinds?: readonly string[];
        readonly limit?: number;
      };
    }
  | { readonly ok: false; readonly message: string } {
  if (typeof input !== "object" || input === null || Array.isArray(input)) {
    return { ok: false, message: "The input must be an object with a query." };
  }
  const record = input as Record<string, unknown>;
  const query = record["query"];
  if (typeof query !== "string") {
    return { ok: false, message: "A query string is required." };
  }
  if (query.trim().length === 0) {
    return { ok: false, message: "A non-empty query is required." };
  }
  const kinds = record["kinds"];
  if (kinds !== undefined && !Array.isArray(kinds)) {
    return { ok: false, message: "The kinds filter must be an array of symbol kinds." };
  }
  const limit = record["limit"];
  if (limit !== undefined && (typeof limit !== "number" || !Number.isInteger(limit) || limit < 1)) {
    return { ok: false, message: "The limit must be a positive integer." };
  }
  return {
    ok: true,
    value: {
      query,
      ...(kinds === undefined ? {} : { kinds: kinds as readonly string[] }),
      ...(limit === undefined ? {} : { limit }),
    },
  };
}
