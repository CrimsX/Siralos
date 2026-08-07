import { buildChildEnvironment, type SandboxEnvironmentPaths } from "./child-environment.js";

/**
 * Per-run sandbox-private paths used by provider-accessible commands.
 */
export interface CommandEnvironmentPaths extends SandboxEnvironmentPaths {
  readonly npmCache: string;
  readonly npmUserConfig: string;
}

export interface CommandEnvironmentOptions {
  /** Add safe npm-specific configuration for the npm-script runner. */
  readonly npm: boolean;
}

const SAFE_COMMAND_VARIABLES: Readonly<Record<string, string>> = {
  NO_COLOR: "1",
  FORCE_COLOR: "0",
  TERM: "dumb",
  GIT_TERMINAL_PROMPT: "0",
};

const NPM_SAFETY_VARIABLES: Readonly<Record<string, string>> = {
  NPM_CONFIG_IGNORE_SCRIPTS: "true",
  NPM_CONFIG_AUDIT: "false",
  NPM_CONFIG_FUND: "false",
  NPM_CONFIG_UPDATE_NOTIFIER: "false",
  NPM_CONFIG_COLOR: "false",
};

/**
 * Minimal command environment: the established allowlisted child environment
 * plus fixed safe values. No parent variables pass through except the
 * allowlisted base set; provider credentials, proxies, Git overrides, and
 * Node startup options are always absent.
 */
export function buildCommandEnvironment(
  parent: Readonly<Record<string, string>>,
  paths: CommandEnvironmentPaths,
  options: CommandEnvironmentOptions,
): Readonly<Record<string, string>> {
  const environment: Record<string, string> = {
    ...buildChildEnvironment(parent, paths),
    ...SAFE_COMMAND_VARIABLES,
  };
  if (options.npm) {
    environment["NPM_CONFIG_CACHE"] = paths.npmCache;
    environment["NPM_CONFIG_USERCONFIG"] = paths.npmUserConfig;
    for (const [name, value] of Object.entries(NPM_SAFETY_VARIABLES)) {
      environment[name] = value;
    }
  }
  return environment;
}
