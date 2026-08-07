import type { CommandRunner, CommandRunnerDefinition } from "./command-runners.js";

export interface CommandRunnerRegistry {
  readonly definitions: readonly CommandRunnerDefinition[];
  get(id: string): CommandRunner | undefined;
}

/**
 * Immutable explicit registry. Runners are never loaded from directories and
 * providers can never register runners; concrete construction happens only in
 * the composition root.
 */
export function createCommandRunnerRegistry(
  runners: readonly CommandRunner[],
): CommandRunnerRegistry {
  const byId = new Map<string, CommandRunner>();
  const definitions: CommandRunnerDefinition[] = [];
  for (const runner of runners) {
    if (byId.has(runner.definition.id)) {
      throw new Error(`Duplicate command runner id: ${runner.definition.id}`);
    }
    byId.set(runner.definition.id, runner);
    definitions.push(runner.definition);
  }
  return {
    get definitions(): readonly CommandRunnerDefinition[] {
      return definitions;
    },
    get(id: string): CommandRunner | undefined {
      return byId.get(id);
    },
  };
}
