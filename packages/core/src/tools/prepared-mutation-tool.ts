import type { Capability } from "../security/capability.js";
import type { ChangePreview } from "../security/change-preview.js";
import type { Tool, ToolExecutionContext, ToolExecutionResult, ToolDefinition } from "./tool.js";

export type ToolPreparationResult =
  | {
      readonly status: "ready";
      readonly mutation: PreparedMutation;
      readonly preview: ChangePreview;
    }
  | {
      readonly status: "invalid_input" | "denied" | "conflict" | "failed" | "cancelled";
      readonly message: string;
    };

export interface PreparedMutationTool {
  readonly kind: "prepared_mutation";
  readonly definition: ToolDefinition;
  readonly capability: "workspace.write";

  prepare(input: unknown, context: ToolExecutionContext): Promise<ToolPreparationResult>;

  apply(prepared: PreparedMutation, context: ToolExecutionContext): Promise<ToolExecutionResult>;
}

export type RegisteredTool = Tool | PreparedMutationTool;

const preparedMutationBrand: unique symbol = Symbol("preparedMutationBrand");

export interface PreparedMutation {
  readonly [preparedMutationBrand]: true;
}

export function createPreparedMutation(): PreparedMutation {
  return { [preparedMutationBrand]: true };
}

export function isPreparedMutationTool(tool: RegisteredTool): tool is PreparedMutationTool {
  return "kind" in tool && tool.kind === "prepared_mutation";
}

export function toolCapability(tool: RegisteredTool): Capability {
  if (isPreparedMutationTool(tool)) {
    return tool.capability;
  }
  return tool.capability ?? "workspace.read";
}
