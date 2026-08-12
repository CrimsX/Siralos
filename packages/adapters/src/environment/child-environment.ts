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
  /^SIRALOS_CONFIG$/i,
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
  // Godot executable/editor-data redirection
  /^GODOT_EDITOR_PATH$/i,
  /^GODOT4_EDITOR_PATH$/i,
  // dynamic-library and executable-loading injection (Linux/macOS; the
  // allowlist already rejects anything else platform-appropriately)
  /^LD_PRELOAD$/i,
  /^LD_LIBRARY_PATH$/i,
  /^DYLD_/,
];

const HOME_VARIABLES: readonly string[] = ["HOME", "USERPROFILE"];
const TEMP_VARIABLES: readonly string[] = ["TEMP", "TMP", "TMPDIR"];

/**
 * The Siralos-controlled environment keys. These variables are owned by
 * the sandbox boundary: their values are always the Siralos-controlled
 * sandbox home/temp paths, and neither the host parent environment nor the
 * sandbox wrapper may ever replace them or introduce an alternative
 * spelling of them. On Windows the comparison is case-insensitive
 * (`UserProfile` and `USERPROFILE` are the same variable); on POSIX keys
 * keep their platform case-sensitive semantics and the same names are
 * still protected.
 */
export const PROTECTED_ENVIRONMENT_KEYS: readonly string[] = [...HOME_VARIABLES, ...TEMP_VARIABLES];

/** True when `name` is a Siralos-controlled environment key. */
export function isProtectedEnvironmentKey(
  name: string,
  platform: NodeJS.Platform = process.platform,
): boolean {
  const key = environmentKeyOf(name, platform);
  return PROTECTED_ENVIRONMENT_KEYS.some(
    (protectedKey) => environmentKeyOf(protectedKey, platform) === key,
  );
}

/**
 * The canonical comparison spelling of one environment key. Windows
 * environment keys are case-insensitive (`Path` and `PATH` denote the same
 * variable), so every key comparison and deduplication funnels through this
 * helper on Windows; on POSIX the original casing is preserved.
 */
export function environmentKeyOf(
  name: string,
  platform: NodeJS.Platform = process.platform,
): string {
  return platform === "win32" ? name.toLowerCase() : name;
}

function findCaseInsensitive(
  parent: Readonly<Record<string, string>>,
  target: string,
  platform: NodeJS.Platform,
): string | undefined {
  const wanted = environmentKeyOf(target, platform);
  for (const [name, value] of Object.entries(parent)) {
    if (environmentKeyOf(name, platform) === wanted) {
      return value;
    }
  }
  return undefined;
}

/**
 * Builds the minimal allowlisted child environment.
 *
 * On Windows every allowed variable is matched case-insensitively, emitted
 * under ONE canonical spelling (the allowlist spelling), and deduplicated,
 * so `Path`/`PATH`, `ComSpec`/`COMSPEC`, and similar aliases collapse into a
 * single variable and no allowed variable is dropped for an alternate
 * casing. On POSIX matching stays case-sensitive. Denied variables are
 * always excluded regardless of casing, the Siralos-controlled home/temp
 * keys (HOME, USERPROFILE on Windows, TEMP, TMP, TMPDIR) always carry the
 * sandbox-controlled values under their canonical spellings, and the
 * returned environment never equals the parent verbatim.
 */
export function buildChildEnvironment(
  parent: Readonly<Record<string, string>>,
  paths: SandboxEnvironmentPaths,
  platform: NodeJS.Platform = process.platform,
): Readonly<Record<string, string>> {
  const environment: Record<string, string> = {};
  if (platform === "win32") {
    const taken = new Set<string>();
    for (const allowed of ALLOWED_VARIABLES) {
      const key = environmentKeyOf(allowed, platform);
      if (taken.has(key)) {
        continue;
      }
      const exact = parent[allowed];
      const value = exact ?? findCaseInsensitive(parent, allowed, platform);
      if (value === undefined) {
        continue;
      }
      if (isDeniedVariable(allowed)) {
        continue;
      }
      taken.add(key);
      environment[allowed] = value;
    }
  } else {
    for (const [name, value] of Object.entries(parent)) {
      if (!ALLOWED_VARIABLES.includes(name)) {
        continue;
      }
      if (isDeniedVariable(name)) {
        continue;
      }
      environment[name] = value;
    }
  }
  // Siralos-controlled home: on Windows both HOME and USERPROFILE are
  // emitted under their canonical spellings with the sandbox-home value
  // (Windows processes and tools may read either), on POSIX HOME is the
  // home variable. The parent can never influence these values.
  environment["HOME"] = paths.home;
  if (platform === "win32") {
    environment["USERPROFILE"] = paths.home;
  }
  // Siralos-controlled temp aliases: every supported platform spelling is
  // emitted with the sandbox-temp value; wrapper or parent values can
  // never replace them (enforced again at wrapper-merge time).
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
