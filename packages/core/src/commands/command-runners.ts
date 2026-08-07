export interface CommandRunnerDefinition {
  readonly id: string;
  readonly description: string;
}

export interface CommandPreparationContext {
  readonly workspaceRoot: string;
  readonly signal?: AbortSignal;
}

export interface CommandPreview {
  readonly runnerId: string;
  readonly displayName: string;
  /** npm package name where safely available. */
  readonly packageName?: string;
  /** npm script name for the npm-script runner. */
  readonly scriptName?: string;
  /** Normalized `/`-separated workspace-relative working directory. */
  readonly workingDirectory: string;
  /** Human-readable identity of the trusted executable, never an absolute path. */
  readonly executableIdentity: string;
  /** Effective argument array with clear boundaries (no concatenated shell string). */
  readonly arguments: readonly string[];
  /** The exact repository-defined npm script body, when applicable. */
  readonly repositoryScript?: string;
  readonly timeoutMs: number;
  readonly stdoutLimitBytes: number;
  readonly stderrLimitBytes: number;
  readonly workspaceAccess: "read-only";
  readonly networkAccess: "denied";
  readonly environmentPolicy: "minimal";
  readonly stdinPolicy: "closed";
  /** Notice that the runner uses its platform script shell (npm). */
  readonly scriptShellNotice?: string;
  /** Notice about automatically associated lifecycle hooks (npm pre/post). */
  readonly hooksNotice?: string;
  /**
   * Notice describing how the executed content is bound to the approved
   * bytes (for example an immutable private copy the script runs from).
   */
  readonly executionNotice?: string;
}

const preparedCommandBrand: unique symbol = Symbol("preparedCommandBrand");

/**
 * Opaque execution plan. Core never interprets the concrete plan; only the
 * runner that created it can turn it back into an execution request.
 */
export interface PreparedCommand {
  readonly [preparedCommandBrand]: true;
}

export function createPreparedCommand(): PreparedCommand {
  return { [preparedCommandBrand]: true };
}

export type CommandPreparationResult =
  | {
      readonly status: "ready";
      readonly command: PreparedCommand;
      readonly preview: CommandPreview;
      readonly digest: string;
      readonly commandId: string;
    }
  | {
      readonly status:
        "invalid_input" | "denied" | "conflict" | "failed" | "cancelled" | "unavailable";
      readonly message: string;
    };

export interface CommandRunPaths {
  /** Unique id of this command run. */
  readonly runId: string;
  /** The run's private root directory beneath the verified runs root. */
  readonly root: string;
  /** Sandbox-private run home directory. */
  readonly home: string;
  /** Sandbox-private run temp directory. */
  readonly temp: string;
  /** Sandbox-private npm cache directory. */
  readonly npmCache: string;
  /** Sandbox-private npm user configuration file. */
  readonly npmUserConfig: string;
  /**
   * Sandbox-private directory holding immutable copies of approved command
   * content (for example the exact approved Node script bytes).
   */
  readonly scriptCache: string;
}

export interface CommandExecutionContext {
  readonly approvedDigest: string;
  readonly signal?: AbortSignal;
  readonly runPaths: CommandRunPaths;
}

export interface CommandExecutionRequest {
  /** Trusted executable path, passed privately to the sandbox backend. */
  readonly executable: string;
  readonly executableIdentity: string;
  readonly executableVersion: string | null;
  readonly arguments: readonly string[];
  /** Canonical host working directory, passed privately to the backend. */
  readonly workingDirectory: string;
  readonly environment: Readonly<Record<string, string>>;
  /** Freshly recomputed digest over the revalidated plan. */
  readonly digest: string;
}

export type CommandExecutionRequestResult =
  | {
      readonly status: "ready";
      readonly request: CommandExecutionRequest;
    }
  | {
      readonly status: "conflict" | "unavailable" | "failed";
      readonly message: string;
    };

export interface CommandRunner {
  readonly definition: CommandRunnerDefinition;

  /**
   * Validate provider input and produce an opaque execution plan plus a
   * user-facing preview. The plan may be used once only and is not serialized.
   */
  prepare(input: unknown, context: CommandPreparationContext): Promise<CommandPreparationResult>;

  /**
   * Revalidate every precondition after approval and translate the plan into
   * an executable request. Fails with `conflict` when any validated file or
   * executable changed since preparation.
   */
  toExecutionRequest(
    command: PreparedCommand,
    context: CommandExecutionContext,
  ): Promise<CommandExecutionRequestResult>;

  /**
   * Report whether this runner can currently resolve its trusted executable.
   * Used for status display only; never launches a process.
   */
  isAvailable(): Promise<boolean>;
}
