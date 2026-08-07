/**
 * Immutable limits for provider-accessible development commands.
 *
 * Providers may request only a timeout within the allowed range; every other
 * limit is fixed by Solaris. No configuration may exceed these values.
 */
export const COMMAND_LIMITS = {
  /** Maximum number of script arguments. */
  maxArguments: 64,
  /** Maximum byte length of a single argument. */
  maxArgumentBytes: 8 * 1024,
  /** Maximum combined byte length of all arguments. */
  maxTotalArgumentBytes: 64 * 1024,
  /** Maximum byte length of an npm package.json read during preparation. */
  maxPackageJsonBytes: 1024 * 1024,
  /** Maximum byte length of an npm script body. */
  maxNpmScriptBytes: 32 * 1024,
  /** Maximum byte length of an npm script name. */
  maxNpmScriptNameBytes: 1024,
  /** Maximum byte length of a Node script file. */
  maxNodeScriptBytes: 4 * 1024 * 1024,
  /** Default command timeout. */
  defaultTimeoutMs: 120_000,
  /** Maximum provider-requested timeout. */
  maxTimeoutMs: 600_000,
  /** Minimum provider-requested timeout. */
  minTimeoutMs: 1_000,
  /** Hard stdout limit; exceeding it terminates the process. */
  stdoutHardLimitBytes: 1024 * 1024,
  /** Hard stderr limit; exceeding it terminates the process. */
  stderrHardLimitBytes: 1024 * 1024,
  /** Provider-visible stdout window. */
  providerStdoutReturnBytes: 256 * 1024,
  /** Provider-visible stderr window. */
  providerStderrReturnBytes: 256 * 1024,
  /** Maximum size of a single streamed output event. */
  maxSingleOutputEventBytes: 16 * 1024,
} as const;
