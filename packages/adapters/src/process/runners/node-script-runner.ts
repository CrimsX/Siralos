import type {
  CommandDigestService,
  CommandExecutionContext,
  CommandExecutionRequestResult,
  CommandPreparationContext,
  CommandPreparationResult,
  CommandRunner,
  PreparedCommand,
} from "@siralos/core";

export interface NodeScriptRunnerOptions {
  readonly digest: CommandDigestService;
}

const NODE_SCRIPT_UNAVAILABLE_MESSAGE =
  "The node-script runner is unavailable: the pinned Node runtime cannot mechanically restrict the approved script to executing only its own approved bytes. Node's internal process.binding surface (for example spawn_sync) is reachable from the script and spawns a fresh unconstrained interpreter, and the approved private copy can be substituted by a same-user process between final verification and launch; both bypass the approved-byte boundary, so every node-script request is refused instead of claiming exact approval.";

/**
 * The node-script runner fails closed. The approved-single-file boundary
 * would require that only the exact approved bytes execute, but the pinned
 * runtime offers no mechanical primitive for that: the script can reach
 * Node internals (process.binding, internal module surfaces) that spawn
 * unconstrained child interpreters, and the private staged copy sits in a
 * same-user writable directory where the verify-to-spawn window cannot be
 * closed without an exec-by-handle primitive the platform does not expose.
 * Because a changed post-approval file or a re-invoked interpreter could
 * otherwise execute unapproved code, every node-script request is refused
 * before approval instead of claiming a boundary the runtime cannot
 * enforce. Siralos never executes validation commands at this stage.
 */
export function createNodeScriptRunner(_options: NodeScriptRunnerOptions): CommandRunner {
  return {
    definition: {
      id: "node-script",
      description:
        "Unavailable: the pinned Node runtime cannot bind execution to the approved script bytes (internal process.binding surfaces and the verify-to-spawn window bypass the boundary).",
    },
    prepare(
      _input: unknown,
      context: CommandPreparationContext,
    ): Promise<CommandPreparationResult> {
      if (context.signal?.aborted) {
        return Promise.resolve({ status: "cancelled", message: "Preparation was cancelled." });
      }
      return Promise.resolve({
        status: "unavailable",
        message: NODE_SCRIPT_UNAVAILABLE_MESSAGE,
      });
    },

    toExecutionRequest(
      _command: PreparedCommand,
      _context: CommandExecutionContext,
    ): Promise<CommandExecutionRequestResult> {
      return Promise.resolve({ status: "unavailable", message: NODE_SCRIPT_UNAVAILABLE_MESSAGE });
    },

    isAvailable(): Promise<boolean> {
      return Promise.resolve(false);
    },
  };
}
