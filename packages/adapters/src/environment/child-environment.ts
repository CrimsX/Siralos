export interface SandboxEnvironmentPaths {
  readonly home: string;
  readonly temp: string;
}

const ALLOWED_VARIABLES: readonly string[] = [
  "PATH",
  "SystemRoot",
  "WINDIR",
  "COMSPEC",
  "PATHEXT",
  "TEMP",
  "TMP",
  "TMPDIR",
  "LANG",
  "LC_ALL",
  "TERM",
];

const DENIED_PATTERNS: readonly RegExp[] = [
  /_API_KEY$/i,
  /_TOKEN$/i,
  /_SECRET$/i,
  /_PASSWORD$/i,
  /^AWS_/i,
  /^AZURE_/i,
  /^GOOGLE_/i,
  /^GITHUB_TOKEN$/i,
  /^GH_TOKEN$/i,
  /^SSH_AUTH_SOCK$/i,
  /^NPM_TOKEN$/i,
  /^NODE_AUTH_TOKEN$/i,
  /^OPENROUTER_API_KEY$/i,
  /^DEEPSEEK_API_KEY$/i,
  /^OPENCODE_API_KEY$/i,
  /^SOLARIS_CONFIG$/i,
  /^NODE_OPTIONS$/i,
  /^BASH_ENV$/i,
  /^ENV$/i,
  /^CDPATH$/i,
  /^GIT_DIR$/i,
  /^GIT_WORK_TREE$/i,
  /^GIT_INDEX_FILE$/i,
  /^GIT_CONFIG/,
  /^NPM_CONFIG_USERCONFIG$/i,
  /^NPM_CONFIG_SCRIPT_SHELL$/i,
  /^HTTP_PROXY$/i,
  /^HTTPS_PROXY$/i,
  /^ALL_PROXY$/i,
  /^NO_PROXY$/i,
];

const HOME_VARIABLES: readonly string[] = ["HOME", "USERPROFILE"];
const TEMP_VARIABLES: readonly string[] = ["TEMP", "TMP", "TMPDIR"];

export function buildChildEnvironment(
  parent: Readonly<Record<string, string>>,
  paths: SandboxEnvironmentPaths,
): Readonly<Record<string, string>> {
  const environment: Record<string, string> = {};
  for (const [name, value] of Object.entries(parent)) {
    if (!ALLOWED_VARIABLES.includes(name)) {
      continue;
    }
    if (isDeniedVariable(name)) {
      continue;
    }
    environment[name] = value;
  }
  const homeVariable = HOME_VARIABLES.find((name) => environment[name] !== undefined);
  if (homeVariable !== undefined) {
    environment[homeVariable] = paths.home;
  } else {
    environment[HOME_VARIABLES[0] ?? "HOME"] = paths.home;
  }
  for (const name of TEMP_VARIABLES) {
    environment[name] = paths.temp;
  }
  return environment;
}

export function readParentEnvironment(): Readonly<Record<string, string>> {
  return process.env as Readonly<Record<string, string>>;
}

export function isDeniedVariable(name: string): boolean {
  return DENIED_PATTERNS.some((pattern) => pattern.test(name));
}
