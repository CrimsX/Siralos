import type { ProcessOutputEvent } from "../security/sandbox-backend.js";
import type { CommandPreview, PreparedCommand } from "./command-runners.js";
import type { ToolDefinition, ToolExecutionContext, ToolExecutionResult } from "../tools/tool.js";

export type ProcessRunInput =
  | {
      readonly runner: "npm-script";
      readonly script: string;
      readonly arguments?: readonly string[];
      readonly workingDirectory?: string;
      readonly timeoutMs?: number;
    }
  | {
      readonly runner: "node-script";
      readonly path: string;
      readonly arguments?: readonly string[];
      readonly workingDirectory?: string;
      readonly timeoutMs?: number;
    };

export type CommandToolPreparationResult =
  | {
      readonly status: "ready";
      readonly command: PreparedCommand;
      readonly preview: CommandPreview;
      readonly digest: string;
      readonly commandId: string;
    }
  | {
      readonly status:
        "invalid_input" | "denied" | "conflict" | "failed" | "cancelled" | "unavailable";
      readonly message: string;
    };

export interface CommandToolExecutionContext {
  /** Digest the user approved; the tool must match it before executing. */
  readonly approvedDigest: string;
  readonly signal?: AbortSignal;
  /** Streaming sink for bounded decoded output events. */
  readonly onOutput?: (event: ProcessOutputEvent) => void;
}

export interface PreparedCommandTool {
  readonly kind: "prepared_command";
  readonly definition: ToolDefinition;
  readonly capability: "process.execute";

  prepare(input: unknown, context: ToolExecutionContext): Promise<CommandToolPreparationResult>;

  executePrepared(
    command: PreparedCommand,
    context: CommandToolExecutionContext,
  ): Promise<ToolExecutionResult>;
}

export const PROCESS_RUN_TOOL_NAME = "process.run";
