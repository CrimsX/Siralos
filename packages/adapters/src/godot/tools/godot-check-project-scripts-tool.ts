import type {
  GodotDiagnostics,
  GodotDiagnosticToolPreparationResult,
  PreparedDiagnosticTool,
  PreparedGDScriptCheck,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@siralos/core";
import { errorMessage } from "../../support/error-message.js";

/**
 * `godot.check_project_scripts` reviewable provider tool: one-time-approved,
 * read-only, bounded, strictly sequential GDScript check of the project's
 * `.gd` scripts (or an explicit bounded subset) against the selected
 * engine's `--check-only` parser in one disposable mirror. The provider
 * cannot select an unlimited subset and cannot choose Godot arguments.
 */
export function createGodotCheckProjectScriptsTool(
  diagnostics: GodotDiagnostics,
): PreparedDiagnosticTool {
  return {
    kind: "prepared_diagnostic",
    definition: {
      name: "godot.check_project_scripts",
      description:
        "Check the project's GDScript files (bounded, deterministic enumeration) with the selected Godot engine's parser (--check-only) inside one disposable project mirror, running strictly sequentially and aggregating normalized diagnostics. Read-only: gameplay, scenes, and scripts are never executed; network is denied; the source workspace is never opened by the engine. One-time approval is required. An optional bounded paths filter selects a subset.",
      inputSchema: {
        type: "object",
        additionalProperties: false,
        properties: {
          paths: {
            type: "array",
            items: { type: "string" },
            description:
              "Optional bounded subset of workspace-relative .gd paths; absent means the whole project.",
          },
        },
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
      const prepared = await diagnostics.prepare(
        parsed.paths === undefined ? {} : { paths: parsed.paths },
        context.signal,
      );
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
              validCount: result.validCount,
              invalidCount: result.invalidCount,
              diagnostics: result.diagnostics.map(toJsonDiagnostic),
              truncated: result.truncated,
            },
            summary: `${result.scriptsChecked} scripts checked, ${result.validCount} valid, ${result.invalidCount} invalid.`,
          };
        }
        return mapExecution(result);
      } catch (error: unknown) {
        return {
          status: "failed",
          message: errorMessage(error, "An unknown GDScript project check failure occurred."),
        };
      }
    },
  };
}

function parseInput(
  input: unknown,
):
  | { readonly ok: true; readonly paths?: readonly string[] }
  | { readonly ok: false; readonly message: string } {
  if (input === undefined) {
    return { ok: true };
  }
  if (typeof input !== "object" || input === null || Array.isArray(input)) {
    return { ok: false, message: "The input must be an object with an optional paths field." };
  }
  const paths = (input as Record<string, unknown>)["paths"];
  if (paths === undefined) {
    return { ok: true };
  }
  if (!Array.isArray(paths) || paths.some((entry) => typeof entry !== "string")) {
    return { ok: false, message: "The paths filter must be an array of strings." };
  }
  if (paths.length === 0) {
    return { ok: true };
  }
  return { ok: true, paths: paths as readonly string[] };
}

function mapPreparation(
  prepared: import("@siralos/core").GodotCheckPreparationResult,
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

function toJsonDiagnostic(diagnostic: import("@siralos/core").GodotGDScriptDiagnostic): {
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
  result: import("@siralos/core").GodotProjectCheckResult,
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
