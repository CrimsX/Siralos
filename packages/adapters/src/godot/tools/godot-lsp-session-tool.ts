import type {
  GDScriptLanguageService,
  GDScriptLSPSessionToolPreparationResult,
  PreparedGDScriptSession,
  PreparedLSPSessionTool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@solaris/core";
import type { LanguageQueryGate } from "./godot-lsp-query-tools.js";
import { errorMessage } from "../../support/error-message.js";

/**
 * `godot.lsp_session` reviewable provider tool: one-time-approved startup
 * of the bounded Godot GDScript language session. The provider cannot
 * approve the session, select the port or host, send raw LSP methods, or
 * request workspace edits; the session runs a recovery-mode editor against
 * the disposable mirror over a Solaris-allocated loopback port. When
 * session startup is unavailable on this platform, preparation returns a
 * typed `unavailable` result before any approval is requested. While an
 * active development workflow owns the session lifecycle (its approval
 * covers LSP recreation after approved edits), the tool defers to the
 * workflow and refuses instead of starting a second session.
 */
export function createGodotLSPSessionTool(
  service: GDScriptLanguageService,
  workflowGate?: LanguageQueryGate,
): PreparedLSPSessionTool {
  return {
    kind: "prepared_lsp_session",
    definition: {
      name: "godot.lsp_session",
      description:
        "Start one bounded Godot GDScript language session: a headless recovery-mode editor runs against a disposable project mirror and serves diagnostics, hover, completion, and definition over a loopback-only LSP channel. External network is denied, source writes are denied, LSP mutations are disabled, and the session expires automatically. One-time approval covers exactly this session.",
      inputSchema: { type: "object", additionalProperties: false },
    },
    capability: "godot.lsp",
    async prepare(
      input: unknown,
      context: ToolExecutionContext,
    ): Promise<GDScriptLSPSessionToolPreparationResult> {
      const workflow = workflowGate?.() ?? { blocked: false, message: null };
      if (workflow.blocked) {
        return {
          status: "failed",
          message:
            workflow.message ??
            "The development workflow manages the language session lifecycle; its one-time approval covers LSP recreation after approved edits.",
        };
      }
      if (input !== undefined && !isEmptyObject(input)) {
        return {
          status: "invalid_input",
          message:
            "The language session accepts no input; the provider cannot choose the port, host, or any session option.",
        };
      }
      try {
        const prepared = await service.prepare(context.signal);
        if (prepared.status === "ready") {
          return {
            status: "ready",
            session: prepared.session,
            preview: prepared.preview,
            digest: prepared.digest,
          };
        }
        return { status: prepared.status, message: prepared.message };
      } catch (error: unknown) {
        if (isAbortError(error)) {
          return {
            status: "cancelled",
            message: "The language session preparation was cancelled.",
          };
        }
        return {
          status: "failed",
          message: errorMessage(error, "An unknown Godot language-session failure occurred."),
        };
      }
    },
    async executePrepared(
      session: PreparedGDScriptSession,
      context: ToolExecutionContext,
    ): Promise<ToolExecutionResult> {
      try {
        const result = await service.start(session, {
          approvedDigest: context.approvedDigest ?? "",
          ...(context.signal === undefined ? {} : { signal: context.signal }),
        });
        if (result.status === "ready") {
          return {
            status: "success",
            output: {
              sessionId: result.session.id,
              engineVersion: result.session.engineVersion,
            },
            summary: `Godot GDScript language session ${result.session.id} is ready.`,
          };
        }
        return mapStartResult(result);
      } catch (error: unknown) {
        if (isAbortError(error)) {
          return { status: "cancelled", message: "The language session startup was cancelled." };
        }
        return {
          status: "failed",
          message: errorMessage(error, "An unknown Godot language-session failure occurred."),
        };
      }
    },
  };
}

function isEmptyObject(value: unknown): boolean {
  return (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.keys(value).length === 0
  );
}

type GodotLSPSessionStartResult = import("@solaris/core").GDScriptSessionStartResult;

function mapStartResult(result: GodotLSPSessionStartResult): ToolExecutionResult {
  if (result.status === "ready") {
    return { status: "success", output: { sessionId: result.session.id }, summary: "ready" };
  }
  return {
    status: result.status === "unsupported" ? "failed" : result.status,
    message: result.message,
  };
}

function isAbortError(error: unknown): boolean {
  return error instanceof Error && (error.name === "AbortError" || error.name === "DOMException");
}
