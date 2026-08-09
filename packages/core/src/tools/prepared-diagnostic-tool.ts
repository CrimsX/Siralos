import type { GodotDiagnosticPreview, PreparedGDScriptCheck } from "../godot/gdscript.js";
import type { ToolExecutionContext, ToolExecutionResult, ToolDefinition } from "./tool.js";

/**
 * Preparation result for a reviewable GDScript diagnostic tool. The preview
 * and digest freeze the exact check-only plan; approval binds to the digest
 * and execution refuses to run under any other digest. When execution is
 * unavailable on this platform, preparation returns a typed `unavailable`
 * result before any approval is requested.
 */
export type GodotDiagnosticToolPreparationResult =
  | {
      readonly status: "ready";
      readonly check: PreparedGDScriptCheck;
      readonly preview: GodotDiagnosticPreview;
      /** Full prepared-check digest; approval binds to exactly this. */
      readonly digest: string;
    }
  | {
      readonly status: "unavailable" | "unsupported" | "cancelled" | "invalid_input" | "failed";
      readonly message: string;
    };

/**
 * Reviewable Godot GDScript diagnostic tool. Like the project-probe tool
 * kind, this kind supports the approval protocol: the application prepares
 * the check, asks for one-time approval when the policy says `ask`, and
 * only then executes with the approved digest. An ordinary `Tool` with an
 * `ask` capability can never execute outside this protocol.
 */
export interface PreparedDiagnosticTool {
  readonly kind: "prepared_diagnostic";
  readonly definition: ToolDefinition;
  readonly capability: "godot.diagnose";

  prepare(
    input: unknown,
    context: ToolExecutionContext,
  ): Promise<GodotDiagnosticToolPreparationResult>;

  executePrepared(
    check: PreparedGDScriptCheck,
    context: ToolExecutionContext,
  ): Promise<ToolExecutionResult>;
}

export function isPreparedDiagnosticTool(tool: unknown): tool is PreparedDiagnosticTool {
  return (
    typeof tool === "object" &&
    tool !== null &&
    "kind" in tool &&
    (tool as { readonly kind?: unknown }).kind === "prepared_diagnostic"
  );
}
