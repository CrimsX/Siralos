import type { JsonObject, JsonValue } from "../domain/json.js";

export interface ToolDefinition {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: JsonObject;
}

export interface ToolExecutionContext {
  readonly signal?: AbortSignal;
}

export type ToolExecutionResult =
  | {
      readonly status: "success";
      readonly output: JsonValue;
      readonly summary: string;
    }
  | {
      readonly status: "invalid_input";
      readonly message: string;
    }
  | {
      readonly status: "denied";
      readonly message: string;
    }
  | {
      readonly status: "failed";
      readonly message: string;
    }
  | {
      readonly status: "cancelled";
      readonly message: string;
    };

export interface Tool {
  readonly definition: ToolDefinition;

  execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult>;
}
