import type {
  GDScriptDevelopmentService,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@solaris/core";

/**
 * `godot.development_status` — read-only bounded status of the active
 * GDScript development workflow (§16). The provider learns the current
 * phase, iteration and repair budgets, and the latest normalized
 * validation outcome without any hidden reasoning or private paths. The
 * workflow's own progress comes from tool results and events, never from
 * chain-of-thought.
 */
export function createGodotDevelopmentStatusTool(
  development: GDScriptDevelopmentService,
): Tool {
  return {
    definition: {
      name: "godot.development_status",
      description:
        "Bounded status of the active GDScript development workflow: phase, iteration and repair budgets, and the latest validation outcome. Read-only; the provider cannot alter the workflow through this tool.",
      inputSchema: { type: "object", additionalProperties: false },
    },
    capability: "godot.development",
    execute(
      input: unknown,
      _context: ToolExecutionContext,
    ): Promise<ToolExecutionResult> {
      if (input !== undefined && !isEmptyObject(input)) {
        return Promise.resolve({
          status: "invalid_input",
          message: "godot.development_status accepts no input.",
        });
      }
      const status = development.status();
      return Promise.resolve({
        status: "success",
        output: {
          support: status.support,
          session:
            status.session === null
              ? null
              : {
                  id: status.session.id,
                  request: status.session.request,
                  state: status.session.state,
                  iteration: status.session.iteration,
                  maxIterations: status.session.maxIterations,
                  repairProposalsRemaining: status.session.repairProposalsRemaining,
                  validation: status.session.validation,
                  appliedChangeSets: status.session.appliedChangeSets,
                  errors: status.session.errors,
                  warnings: status.session.warnings,
                },
        },
        summary:
          status.session === null
            ? "no active development workflow"
            : `development workflow ${status.session.state.kind === "active" ? status.session.state.phase : status.session.state.status} (iteration ${status.session.iteration}/${status.session.maxIterations})`,
      });
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
