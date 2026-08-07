/**
 * Deterministic canonical representation of every command execution detail.
 *
 * The digest is computed over the runner id, trusted executable identity and
 * version, the validated script or package identity and hash, the repository
 * script body, the argument array, the working directory, the effective
 * sandbox profile, the environment policy, the timeout, the output limits,
 * the stdin policy, and the network policy.
 *
 * Approval applies to this digest; any change between preview and execution
 * produces a conflict and nothing executes under an earlier approval.
 */
export interface CommandDigestParts {
  readonly runnerId: string;
  readonly executableIdentity: string;
  readonly executableVersion: string | null;
  /** npm script name or Node script workspace-relative path. */
  readonly script: string;
  /** SHA-256 of the validated package.json or script file. */
  readonly fileHash: string | null;
  /** Exact repository npm script body when applicable. */
  readonly repositoryScript: string | null;
  readonly arguments: readonly string[];
  readonly workingDirectory: string;
  readonly profileId: string;
  readonly environmentPolicy: string;
  readonly timeoutMs: number;
  readonly stdoutLimitBytes: number;
  readonly stderrLimitBytes: number;
  readonly stdinPolicy: string;
  readonly networkPolicy: string;
}

export function canonicalizeCommandDigest(parts: CommandDigestParts): string {
  return JSON.stringify({
    runnerId: parts.runnerId,
    executable: parts.executableIdentity,
    executableVersion: parts.executableVersion ?? null,
    script: parts.script,
    fileHash: parts.fileHash ?? null,
    repositoryScript: parts.repositoryScript ?? null,
    arguments: parts.arguments,
    workingDirectory: parts.workingDirectory,
    profile: parts.profileId,
    environmentPolicy: parts.environmentPolicy,
    timeoutMs: parts.timeoutMs,
    stdoutLimitBytes: parts.stdoutLimitBytes,
    stderrLimitBytes: parts.stderrLimitBytes,
    stdinPolicy: parts.stdinPolicy,
    networkPolicy: parts.networkPolicy,
  });
}

/**
 * Hashing port so core stays free of Node imports. Implemented by an approved
 * adapter using a trusted hash function.
 */
export interface CommandDigestService {
  compute(parts: CommandDigestParts): string;
}
