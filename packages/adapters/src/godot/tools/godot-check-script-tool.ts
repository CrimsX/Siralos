import type {
  GodotDiagnostics,
  GodotDiagnosticToolPreparationResult,
  PreparedDiagnosticTool,
  PreparedGDScriptCheck,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@solaris/core";
import { errorMessage } from "../../support/error-message.js";

/**
 * `godot.check_script` reviewable provider tool: one-time-approved,
 * read-only GDScript check of a single workspace-relative `.gd` script
 * against the selected engine's `--check-only` parser in the disposable
 * mirror. The provider cannot choose Godot arguments, the mirror location,
 * the sandbox configuration, or any limit; it cannot approve itself, and
 * its input is bounded by the immutable limits.
 */
export function createGodotCheckScriptTool(diagnostics: GodotDiagnostics): PreparedDiagnosticTool {
  return {
    kind: "prepared_diagnostic",
    definition: {
      name: "godot.check_script",
      description:
        "Check one workspace-relative .gd script with the selected Godot engine's parser (--check-only) inside a disposable project mirror. Read-only: gameplay, scenes, and scripts are never executed; network is denied; the source workspace is never opened by the engine. One-time approval is required.",
      inputSchema: {
        type: "object",
        additionalProperties: false,
        properties: {
          path: {
            type: "string",
            description: "Workspace-relative .gd script path, e.g. src/player/player.gd.",
          },
        },
        required: ["path"],
      },
    },
    capability: "godot.diagnose",
    async prepare(
      input: unknown,
      context: ToolExecutionContext,
    ): Promise<GodotDiagnosticToolPreparationResult> {
      const parsed = parseInput(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const prepared = await diagnostics.prepare({ paths: [parsed.path] }, context.signal);
      return mapPreparation(prepared);
    },
    async executePrepared(
      check: PreparedGDScriptCheck,
      context: ToolExecutionContext,
    ): Promise<ToolExecutionResult> {
      try {
        const result = await diagnostics.execute(check, {
          approvedDigest: context.approvedDigest ?? "",
          ...(context.signal === undefined ? {} : { signal: context.signal }),
        });
        if (result.status === "checked") {
          return {
            status: "success",
            output: {
              engineVersion: result.engineVersion,
              scriptsChecked: result.scriptsChecked,
              valid: result.invalidCount === 0,
              validCount: result.validCount,
              invalidCount: result.invalidCount,
              diagnostics: result.diagnostics.map(toJsonDiagnostic),
              truncated: result.truncated,
            },
            summary:
              result.invalidCount === 0
                ? "The script is valid GDScript."
                : `The script has ${result.diagnostics.length} diagnostic${result.diagnostics.length === 1 ? "" : "s"}.`,
          };
        }
        return mapExecution(result);
      } catch (error: unknown) {
        return {
          status: "failed",
          message: errorMessage(error, "An unknown GDScript check failure occurred."),
        };
      }
    },
  };
}

function parseInput(
  input: unknown,
): { readonly ok: true; readonly path: string } | { readonly ok: false; readonly message: string } {
  if (typeof input !== "object" || input === null || Array.isArray(input)) {
    return { ok: false, message: "The input must be an object with a path field." };
  }
  const path = (input as Record<string, unknown>)["path"];
  if (typeof path !== "string" || path.trim().length === 0) {
    return { ok: false, message: "A workspace-relative .gd script path is required." };
  }
  return { ok: true, path };
}

function mapPreparation(
  prepared: import("@solaris/core").GodotCheckPreparationResult,
): GodotDiagnosticToolPreparationResult {
  if (prepared.status === "ready") {
    return {
      status: "ready",
      check: prepared.check,
      preview: prepared.preview,
      digest: prepared.digest,
    };
  }
  if (prepared.status === "invalid_input") {
    return { status: "invalid_input", message: prepared.message };
  }
  return { status: prepared.status, message: prepared.message };
}

function toJsonDiagnostic(diagnostic: import("@solaris/core").GodotGDScriptDiagnostic): {
  readonly source: string;
  readonly severity: string;
  readonly path: string | null;
  readonly line: number | null;
  readonly column: number | null;
  readonly code: string | null;
  readonly message: string;
  readonly rawCategory: string | null;
} {
  return {
    source: diagnostic.source,
    severity: diagnostic.severity,
    path: diagnostic.path,
    line: diagnostic.line,
    column: diagnostic.column,
    code: diagnostic.code,
    message: diagnostic.message,
    rawCategory: diagnostic.rawCategory,
  };
}

function mapExecution(
  result: import("@solaris/core").GodotProjectCheckResult,
): ToolExecutionResult {
  if (result.status === "checked") {
    return { status: "success", output: {}, summary: "Check completed." };
  }
  return {
    status:
      result.status === "unsupported" || result.status === "sandbox_failed"
        ? "failed"
        : result.status,
    message: result.message,
  };
}
