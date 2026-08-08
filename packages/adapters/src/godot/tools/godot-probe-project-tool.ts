import type {
  GodotProjectProbe,
  GodotRecoveryProbeResult,
  PreparedProjectProbeTool,
  ToolExecutionContext,
  ToolExecutionResult,
  GodotProbeToolPreparationResult,
} from "@solaris/core";

/**
 * Reviewable `godot.probe_project` provider tool.
 *
 * The provider cannot choose the executable, its arguments, the project
 * path, recovery-mode flags, sandbox configuration, network access,
 * timeouts, mirror location, exclusions, environment, or plugin state: the
 * tool accepts no input and delegates every decision to the probe service.
 * Approval is one-time and binds to the prepared probe digest.
 */
export function createGodotProbeProjectTool(probe: GodotProjectProbe): PreparedProjectProbeTool {
  return {
    kind: "prepared_probe",
    definition: {
      name: "godot.probe_project",
      description:
        "Run one recovery-mode Godot editor probe against a disposable mirror of the current project: the source workspace is never opened by Godot, network is denied, secrets are removed, generated cache stays inside the mirror, and the mirror is destroyed afterwards. Requires one-time approval bound to the current project risk manifest.",
      inputSchema: { type: "object", additionalProperties: false },
    },
    capability: "godot.probe_project",
    async prepare(
      input: unknown,
      context: ToolExecutionContext,
    ): Promise<GodotProbeToolPreparationResult> {
      if (!isEmptyObject(input)) {
        return {
          status: "invalid_input",
          message:
            "The project probe accepts no input; the provider cannot choose any probe option.",
        };
      }
      try {
        const prepared = await probe.prepare(context.signal);
        if (prepared.status !== "ready") {
          return { status: "failed", message: prepared.message };
        }
        return {
          status: "ready",
          probe: prepared.probe,
          preview: prepared.preview,
          digest: prepared.digest,
        };
      } catch (error: unknown) {
        return { status: "failed", message: describeError(error) };
      }
    },
    async executePrepared(
      prepared: import("@solaris/core").PreparedGodotProbe,
      context: ToolExecutionContext,
    ): Promise<ToolExecutionResult> {
      let result: GodotRecoveryProbeResult;
      try {
        result = await probe.execute(prepared, {
          approvedDigest: context.approvedDigest ?? "",
          ...(context.signal === undefined ? {} : { signal: context.signal }),
        });
      } catch (error: unknown) {
        return { status: "failed", message: describeError(error) };
      }
      return mapProbeResult(result);
    },
  };
}

function mapProbeResult(result: GodotRecoveryProbeResult): ToolExecutionResult {
  switch (result.status) {
    case "completed":
    case "completed_with_diagnostics": {
      return {
        status: "success",
        output: {
          status: result.status,
          engine: {
            installationId: result.engine.installationId,
            version: result.engine.version,
            executableFingerprint: result.engine.executableFingerprint,
          },
          recoveryMode: result.recoveryMode,
          sourceWorkspaceLoaded: false,
          mirror: {
            sourceFiles: result.mirror.sourceFiles,
            sourceBytes: result.mirror.sourceBytes,
            generatedGodotDirectory: result.mirror.generatedGodotDirectory,
            generatedBytes: result.mirror.generatedBytes,
            generatedFiles: result.mirror.generatedFiles,
            importState: result.mirror.importState,
          },
          diagnostics: {
            errors: result.diagnostics.errors.map((diagnostic) => ({
              severity: diagnostic.severity,
              category: diagnostic.category,
              message: diagnostic.message,
            })),
            warnings: result.diagnostics.warnings.map((diagnostic) => ({
              severity: diagnostic.severity,
              category: diagnostic.category,
              message: diagnostic.message,
            })),
            truncated: result.diagnostics.truncated,
          },
          process: {
            exitCode: result.process.exitCode,
            durationMs: result.process.durationMs,
            timedOut: result.process.timedOut,
          },
          workspaceIntegrity: {
            unchanged: result.workspaceIntegrity.unchanged,
            bounded: result.workspaceIntegrity.bounded,
          },
          cleanup: {
            completed: result.cleanup.completed,
          },
        },
        summary: summarizeResult(result),
      };
    }
    case "conflict":
      return { status: "conflict", message: result.message };
    case "timed_out":
      return { status: "timed_out", message: result.message };
    case "cancelled":
      return { status: "cancelled", message: result.message };
    case "sandbox_failed":
      return { status: "sandbox_unavailable", message: result.message };
    case "workspace_changed":
      return { status: "workspace_violation", message: result.message };
    case "denied":
    case "unsupported":
    case "mirror_too_large":
    case "failed":
      return { status: "failed", message: result.message };
  }
}

function summarizeResult(result: GodotRecoveryProbeResult): string {
  const errorCount = result.diagnostics.errors.length;
  const warningCount = result.diagnostics.warnings.length;
  const diagnostics =
    errorCount === 0 && warningCount === 0
      ? "no diagnostics"
      : `${errorCount} error${errorCount === 1 ? "" : "s"}, ${warningCount} warning${warningCount === 1 ? "" : "s"}`;
  return `Recovery-mode Godot project probe ${result.status}: ${diagnostics}; source workspace untouched, disposable mirror removed.`;
}

function isEmptyObject(input: unknown): boolean {
  return (
    typeof input === "object" &&
    input !== null &&
    !Array.isArray(input) &&
    Object.keys(input).length === 0
  );
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unknown Godot project probe failure occurred.";
}
