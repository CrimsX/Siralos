import {
  GODOT_LIMITS,
  type GodotKnowledge,
  type Tool,
  type ToolExecutionContext,
  type ToolExecutionResult,
} from "@solaris/core";
import { truncateUtf8Bytes } from "../knowledge/api-dump-with-docs.js";
import { errorMessage } from "../../support/error-message.js";

const MAX_SYMBOL_LENGTH = 1024;

/**
 * `godot.api_lookup` provider tool: exact-symbol lookup in the
 * version-matched API knowledge base. Unknown symbols return a structured
 * not-found result; the description is bounded to the immutable lookup
 * bound; the engine version is always included. The provider cannot request
 * raw index files and cannot change the engine profile.
 */
export function createGodotApiLookupTool(knowledge: GodotKnowledge): Tool {
  return {
    definition: {
      name: "godot.api_lookup",
      description:
        "Look up one exact API symbol in the selected engine's documentation by its deterministic symbol identity (e.g. class:Node/method:add_child, class:Node/property:owner, global:enum:Error, utility:lerp). Returns the signature, bounded description, owner, and inheritance. Unknown symbols return a structured not-found result.",
      inputSchema: {
        type: "object",
        additionalProperties: false,
        properties: {
          symbol: {
            type: "string",
            description: "Exact symbol identity, e.g. class:Node/method:add_child.",
          },
        },
        required: ["symbol"],
      },
    },
    capability: "godot.api",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      try {
        if (typeof input !== "object" || input === null || Array.isArray(input)) {
          return {
            status: "invalid_input",
            message: "The input must be an object with a symbol field.",
          };
        }
        const symbol = (input as Record<string, unknown>)["symbol"];
        if (typeof symbol !== "string" || symbol.trim().length === 0) {
          return { status: "invalid_input", message: "A non-empty symbol identity is required." };
        }
        if (symbol.length > MAX_SYMBOL_LENGTH) {
          return {
            status: "invalid_input",
            message: `The symbol identity exceeds the ${MAX_SYMBOL_LENGTH}-character bound.`,
          };
        }
        const result = await knowledge.lookup(symbol, context.signal);
        if (result.status === "ready") {
          const output = {
            symbol: result.result.symbol,
            engineVersion: result.engineVersion,
            kind: result.result.kind,
            name: result.result.name,
            owner: result.result.owner,
            inheritedFrom: result.result.inheritedFrom,
            signature: result.result.signature,
            description: boundLookupDescription(result.result.description),
            apiType: result.result.apiType,
            details: {
              ...(result.result.details.returnType === undefined
                ? {}
                : { returnType: result.result.details.returnType }),
              ...(result.result.details.parameters === undefined
                ? {}
                : {
                    parameters: result.result.details.parameters.map((parameter) => ({
                      name: parameter.name,
                      type: parameter.type,
                      defaultValue: parameter.defaultValue,
                    })),
                  }),
              ...(result.result.details.qualifiers === undefined
                ? {}
                : { qualifiers: [...result.result.details.qualifiers] }),
              ...(result.result.details.hash === undefined
                ? {}
                : { hash: result.result.details.hash }),
              ...(result.result.details.type === undefined
                ? {}
                : { type: result.result.details.type }),
              ...(result.result.details.setter === undefined
                ? {}
                : { setter: result.result.details.setter }),
              ...(result.result.details.getter === undefined
                ? {}
                : { getter: result.result.details.getter }),
              ...(result.result.details.value === undefined
                ? {}
                : { value: result.result.details.value }),
              ...(result.result.details.values === undefined
                ? {}
                : { values: result.result.details.values }),
            },
          };
          return {
            status: "success",
            output,
            summary: `${result.result.symbol}`,
          };
        }
        if (result.status === "not_found") {
          return {
            status: "success",
            output: {
              found: false,
              symbol,
              engineVersion: null,
              message: result.message,
            },
            summary: `Unknown API symbol ${symbol}`,
          };
        }
        return {
          status: result.status === "unsupported" ? "failed" : result.status,
          message: result.message,
        };
      } catch (error: unknown) {
        return {
          status: "failed",
          message: errorMessage(error, "An unknown Godot API lookup failure occurred."),
        };
      }
    },
  };
}

/** Enforces the immutable serialized lookup bound on the description. */
function boundLookupDescription(description: string | null): string | null {
  if (description === null) {
    return null;
  }
  const bound = GODOT_LIMITS.maxApiLookupResultBytes - 16 * 1024;
  return truncateUtf8Bytes(description, Math.max(bound, 0));
}
