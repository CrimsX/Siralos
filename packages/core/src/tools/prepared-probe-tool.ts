import type { GodotProbePreview, PreparedGodotProbe } from "../godot/probe.js";
import type { ToolExecutionContext, ToolExecutionResult, ToolDefinition } from "./tool.js";

/**
 * Preparation result for a reviewable project-probe tool. The preview and
 * digest freeze the exact recovery plan; approval binds to the digest and
 * execution refuses to run under any other digest. When execution is
 * unavailable on this platform, preparation returns a typed `unavailable`
 * result before any approval is requested.
 */
export type GodotProbeToolPreparationResult =
  | {
      readonly status: "ready";
      readonly probe: PreparedGodotProbe;
      readonly preview: GodotProbePreview;
      /** Full prepared-probe digest; approval binds to exactly this. */
      readonly digest: string;
    }
  | {
      readonly status: "unavailable" | "unsupported" | "cancelled" | "invalid_input" | "failed";
      readonly message: string;
    };

/**
 * Reviewable Godot project-probe tool. Unlike plain tools, this kind
 * supports the approval protocol: the application prepares the probe, asks
 * for one-time approval when the policy says `ask`, and only then executes
 * with the approved digest. An ordinary `Tool` with an `ask` capability can
 * never execute outside this protocol.
 */
export interface PreparedProjectProbeTool {
  readonly kind: "prepared_probe";
  readonly definition: ToolDefinition;
  readonly capability: "godot.probe_project";

  prepare(input: unknown, context: ToolExecutionContext): Promise<GodotProbeToolPreparationResult>;

  executePrepared(
    probe: PreparedGodotProbe,
    context: ToolExecutionContext,
  ): Promise<ToolExecutionResult>;
}

export function isPreparedProbeTool(tool: unknown): tool is PreparedProjectProbeTool {
  return (
    typeof tool === "object" &&
    tool !== null &&
    "kind" in tool &&
    (tool as { readonly kind?: unknown }).kind === "prepared_probe"
  );
}
