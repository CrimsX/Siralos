import type {
  CommandDigestService,
  CommandExecutionContext,
  CommandExecutionRequestResult,
  CommandPreparationContext,
  CommandPreparationResult,
  CommandRunner,
  PreparedCommand,
} from "@siralos/core";
import type { NpmCliResolution } from "../trusted-executables.js";

export interface NpmScriptRunnerOptions {
  readonly digest: CommandDigestService;
  /** Injectable trusted npm CLI resolver; defaults to the trusted resolver. */
  readonly npmResolver?: () => Promise<NpmCliResolution>;
}

const NPM_SCRIPT_UNAVAILABLE_MESSAGE =
  "The npm-script runner is unavailable: npm re-reads the mutable workspace package.json at its own execution time, and the pinned sandbox runtime cannot bind that read to the approved package bytes without copying the broader package state. Use the node-script runner for validated single-file execution.";

/**
 * The npm-script runner fails closed. `npm run` executes whatever script body
 * is present in the workspace package.json at npm's own read time, which
 * happens after Siralos's final revalidation; the pinned sandbox runtime
 * cannot substitute the approved package bytes at that read (content
 * override binds are Linux-only internals that degrade to read-denial on
 * macOS and are unsupported on Windows, and private-directory execution
 * breaks the cwd-relative behavior of normal repository scripts). Because a
 * changed post-approval script could otherwise execute, every npm-script
 * request is refused instead of claiming exact approval.
 */
export function createNpmScriptRunner(_options: NpmScriptRunnerOptions): CommandRunner {
  return {
    definition: {
      id: "npm-script",
      description:
        "Unavailable: npm execution cannot be bound to the approved package bytes under the pinned sandbox runtime.",
    },
    prepare(
      _input: unknown,
      context: CommandPreparationContext,
    ): Promise<CommandPreparationResult> {
      if (context.signal?.aborted) {
        return Promise.resolve({ status: "cancelled", message: "Preparation was cancelled." });
      }
      return Promise.resolve({ status: "unavailable", message: NPM_SCRIPT_UNAVAILABLE_MESSAGE });
    },

    toExecutionRequest(
      _command: PreparedCommand,
      _context: CommandExecutionContext,
    ): Promise<CommandExecutionRequestResult> {
      return Promise.resolve({ status: "unavailable", message: NPM_SCRIPT_UNAVAILABLE_MESSAGE });
    },

    isAvailable(): Promise<boolean> {
      return Promise.resolve(false);
    },
  };
}
