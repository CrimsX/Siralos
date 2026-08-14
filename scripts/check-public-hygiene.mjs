/**
 * High-confidence public-repository hygiene ratchet.
 *
 * This is deliberately not a general-purpose secret or PII scanner. GitHub
 * secret scanning remains responsible for provider credential patterns. This
 * check owns the repository-specific publication mistakes that can be
 * detected deterministically without noisy heuristics: forbidden tracked
 * artifacts, raw conversation exports, private-key blocks, non-synthetic
 * email addresses, and non-synthetic home-directory examples.
 *
 * Diagnostics contain only category, path, and line. Matching values are
 * never returned or printed.
 */
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join, sep } from "node:path";
import { pathToFileURL } from "node:url";

const FORBIDDEN_DIRECTORY_NAMES = new Set([
  ".idea",
  ".reasonix",
  ".vs",
  ".vscode",
  "audit",
  "audits",
  "coverage",
  "dist",
  "node_modules",
  "reports",
  "target",
  "temp",
  "tmp",
]);

const FORBIDDEN_FILE_NAMES = new Set([".DS_Store", "Thumbs.db"]);
const FORBIDDEN_FILE_SUFFIXES = [
  ".bak",
  ".core",
  ".dmp",
  ".log",
  ".profdata",
  ".profraw",
  ".swp",
  ".swo",
  ".tmp",
];

const RAW_CONVERSATION_PATH =
  /(?:^|[-_. ])(?:chatgpt|chat[-_ ]?transcript|conversation[-_ ]?(?:export|transcript)|prompt[-_ ]?history|raw[-_ ]?handoff)(?:$|[-_. ])/i;

const EMAIL_PATTERN =
  /(?<![A-Z0-9._%+-])[A-Z0-9._%+-]+@(?<domain>[A-Z0-9.-]+\.[A-Z]{2,})(?![A-Z0-9.-])/giu;
const HOME_PATH_PATTERN =
  /(?:[A-Z]:(?:[\\/]+)Users(?:[\\/]+)(?<windows>[^\\/\s"'<>]+)|\/Users\/(?<mac>[^/\s"'<>]+)|\/home\/(?<posix>[^/\s"'<>]+))/giu;
const PRIVATE_KEY_PATTERN = new RegExp(
  ["-----BEGIN ", "(?:RSA |EC |OPENSSH |DSA |PGP )?", "PRIVATE", " KEY-----"].join(""),
  "giu",
);

const PUBLIC_EMAIL_DOMAINS = new Set([
  "example.com",
  "example.invalid",
  "github.com",
  "users.noreply.github.com",
]);

const SYNTHETIC_HOME_USERS = new Set([
  "\\xff\\xfe",
  "alice",
  "host-user",
  "me",
  "secret",
  "secret-user",
  "test",
  "tester",
  "testuser",
  "user",
  "x",
  "über-使用者",
]);

function lineAt(text, offset) {
  let line = 1;
  for (let index = 0; index < offset; index += 1) {
    if (text.charCodeAt(index) === 10) {
      line += 1;
    }
  }
  return line;
}

function normalizePath(path) {
  return path.split(sep).join("/");
}

function isEnvironmentFile(name) {
  return name === ".env" || (name.startsWith(".env.") && name !== ".env.example");
}

/** Return path-only violations for one tracked repository path. */
export function scanTrackedPath(path) {
  const normalized = normalizePath(path);
  const components = normalized.split("/");
  const name = components.at(-1) ?? "";
  const violations = [];

  if (components.some((component) => FORBIDDEN_DIRECTORY_NAMES.has(component))) {
    violations.push({ category: "tracked_generated_or_local_artifact", path: normalized });
  }
  if (
    FORBIDDEN_FILE_NAMES.has(name) ||
    FORBIDDEN_FILE_SUFFIXES.some((suffix) => name.toLowerCase().endsWith(suffix)) ||
    isEnvironmentFile(name)
  ) {
    violations.push({ category: "tracked_private_or_local_file", path: normalized });
  }
  if (RAW_CONVERSATION_PATH.test(name)) {
    violations.push({ category: "tracked_raw_conversation_export", path: normalized });
  }

  return violations;
}

/** Return redacted text violations for one UTF-8 tracked file. */
export function scanPublicText(path, text) {
  const violations = [];

  PRIVATE_KEY_PATTERN.lastIndex = 0;
  for (const match of text.matchAll(PRIVATE_KEY_PATTERN)) {
    violations.push({ category: "private_key_block", path, line: lineAt(text, match.index) });
  }

  EMAIL_PATTERN.lastIndex = 0;
  for (const match of text.matchAll(EMAIL_PATTERN)) {
    const domain = match.groups?.domain?.toLowerCase() ?? "";
    if (!PUBLIC_EMAIL_DOMAINS.has(domain)) {
      violations.push({ category: "non_public_email", path, line: lineAt(text, match.index) });
    }
  }

  HOME_PATH_PATTERN.lastIndex = 0;
  for (const match of text.matchAll(HOME_PATH_PATTERN)) {
    const user = (
      match.groups?.windows ??
      match.groups?.mac ??
      match.groups?.posix ??
      ""
    ).toLowerCase();
    if (!SYNTHETIC_HOME_USERS.has(user)) {
      violations.push({
        category: "non_synthetic_home_path",
        path,
        line: lineAt(text, match.index),
      });
    }
  }

  return violations;
}

/** Scan an explicit tracked-file list beneath `root`. */
export function runCheck(root, trackedFiles) {
  const violations = [];
  const decoder = new TextDecoder("utf-8", { fatal: true });

  for (const path of [...trackedFiles].map(normalizePath).sort()) {
    violations.push(...scanTrackedPath(path));
    let bytes;
    try {
      bytes = readFileSync(join(root, path));
    } catch {
      violations.push({ category: "tracked_file_unreadable", path });
      continue;
    }
    if (bytes.includes(0)) {
      continue;
    }
    try {
      violations.push(...scanPublicText(path, decoder.decode(bytes)));
    } catch {
      // Invalid UTF-8 is treated as binary for this focused text ratchet.
    }
  }

  return {
    ok: violations.length === 0,
    violations: violations.sort(
      (left, right) =>
        left.path.localeCompare(right.path) ||
        (left.line ?? 0) - (right.line ?? 0) ||
        left.category.localeCompare(right.category),
    ),
  };
}

/** Obtain the tracked path set without invoking a shell or content filters. */
export function collectTrackedFiles(root) {
  const environment = {};
  const allowedEnvironment = new Set([
    "COMSPEC",
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
    "WINDIR",
  ]);
  for (const [name, value] of Object.entries(process.env)) {
    if (value !== undefined && allowedEnvironment.has(name.toUpperCase())) {
      environment[name] = value;
    }
  }
  Object.assign(environment, {
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_OPTIONAL_LOCKS: "0",
    GIT_PAGER: "cat",
    GIT_TERMINAL_PROMPT: "0",
  });

  const result = spawnSync(
    "git",
    [
      "-c",
      "core.fsmonitor=false",
      "-c",
      "core.untrackedCache=false",
      "-c",
      `safe.directory=${root}`,
      "ls-files",
      "-z",
    ],
    {
      cwd: root,
      encoding: null,
      maxBuffer: 16 * 1024 * 1024,
      shell: false,
      windowsHide: true,
      env: environment,
    },
  );
  if (result.error !== undefined) {
    throw new Error(
      `unable to launch tracked-file enumeration (${result.error.code ?? "unknown error"})`,
    );
  }
  if (result.status !== 0) {
    throw new Error(`tracked-file enumeration exited ${result.status ?? "without status"}`);
  }
  return result.stdout
    .toString("utf8")
    .split("\0")
    .filter((path) => path.length > 0);
}

function main() {
  const root = join(import.meta.dirname, "..");
  let result;
  try {
    result = runCheck(root, collectTrackedFiles(root));
  } catch (error) {
    console.error(error instanceof Error ? error.message : "public-hygiene check failed");
    process.exit(1);
  }
  if (!result.ok) {
    console.error("Public-hygiene violations:");
    for (const violation of result.violations) {
      const location =
        violation.line === undefined ? violation.path : `${violation.path}:${violation.line}`;
      console.error(`  - ${violation.category}: ${location}`);
    }
    process.exit(1);
  }
  console.log("Public-hygiene check passed: no high-confidence publication violations found.");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
