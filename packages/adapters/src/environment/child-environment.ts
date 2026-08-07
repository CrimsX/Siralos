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
  /_API_KEY$/,
  /_TOKEN$/,
  /_SECRET$/,
  /_PASSWORD$/,
  /^AWS_/,
  /^AZURE_/,
  /^GOOGLE_/,
  /^GITHUB_TOKEN$/,
  /^GH_TOKEN$/,
  /^SSH_AUTH_SOCK$/,
  /^NPM_TOKEN$/,
  /^NODE_AUTH_TOKEN$/,
  /^OPENROUTER_API_KEY$/,
  /^DEEPSEEK_API_KEY$/,
  /^OPENCODE_API_KEY$/,
  /^SOLARIS_CONFIG$/,
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

export function isDeniedVariable(name: string): boolean {
  return DENIED_PATTERNS.some((pattern) => pattern.test(name));
}
