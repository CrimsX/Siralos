import type { GDScriptLSPSessionPreview, PreparedGDScriptSession } from "../godot/lsp.js";
import type { ToolExecutionContext, ToolExecutionResult, ToolDefinition } from "./tool.js";

/**
 * Preparation result for the reviewable Godot LSP session tool. The preview
 * and digest freeze the exact session plan; approval binds to the digest
 * and startup refuses to run under any other digest. When session startup
 * is unavailable on this platform, preparation returns a typed
 * `unavailable` result before any approval is requested.
 */
export type GDScriptLSPSessionToolPreparationResult =
  | {
      readonly status: "ready";
      readonly session: PreparedGDScriptSession;
      readonly preview: GDScriptLSPSessionPreview;
      /** Full prepared-session digest; approval binds to exactly this. */
      readonly digest: string;
    }
  | {
      readonly status: "unavailable" | "unsupported" | "cancelled" | "invalid_input" | "failed";
      readonly message: string;
    };

/**
 * Reviewable Godot GDScript language-session tool. Like the project-probe
 * and diagnostic tool kinds, this kind supports the approval protocol: the
 * application prepares the session plan, asks for one-time approval when
 * the policy says `ask`, and only then starts the session with the
 * approved digest. One approval covers exactly one bounded session;
 * session restart requires a new approval.
 */
export interface PreparedLSPSessionTool {
  readonly kind: "prepared_lsp_session";
  readonly definition: ToolDefinition;
  readonly capability: "godot.lsp";

  prepare(
    input: unknown,
    context: ToolExecutionContext,
  ): Promise<GDScriptLSPSessionToolPreparationResult>;

  executePrepared(
    session: PreparedGDScriptSession,
    context: ToolExecutionContext,
  ): Promise<ToolExecutionResult>;
}

export function isPreparedLSPSessionTool(tool: unknown): tool is PreparedLSPSessionTool {
  return (
    typeof tool === "object" &&
    tool !== null &&
    "kind" in tool &&
    (tool as { readonly kind?: unknown }).kind === "prepared_lsp_session"
  );
}
