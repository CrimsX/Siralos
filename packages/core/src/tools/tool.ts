import type { Capability } from "../security/capability.js";
import type { JsonObject, JsonValue } from "../domain/json.js";

export interface ToolDefinition {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: JsonObject;
}

export interface ToolExecutionContext {
  readonly signal?: AbortSignal;
  /**
   * Digest of the immutable prepared plan that was reviewed (and, when
   * required, approved). Prepared mutation tools verify the plan they are
   * asked to apply against this digest and fail closed on mismatch.
   */
  readonly approvedDigest?: string;
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
      readonly status: "conflict";
      readonly message: string;
    }
  | {
      readonly status: "failed";
      readonly message: string;
    }
  | {
      readonly status: "cancelled";
      readonly message: string;
    }
  | {
      readonly status: "timed_out";
      readonly message: string;
    }
  | {
      readonly status: "output_limit";
      readonly message: string;
    }
  | {
      readonly status: "sandbox_denied";
      readonly message: string;
    }
  | {
      readonly status: "sandbox_unavailable";
      readonly message: string;
    }
  | {
      readonly status: "workspace_violation";
      readonly message: string;
    }
  | {
      readonly status: "unavailable";
      readonly message: string;
    };

export interface Tool {
  readonly definition: ToolDefinition;
  readonly capability?: Capability;

  execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult>;
}
