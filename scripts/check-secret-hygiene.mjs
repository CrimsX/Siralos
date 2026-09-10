/**
 * Mechanical secret-hygiene gate for decision 68 section 4.
 *
 * The check fails if a credential-shaped value appears in portable config
 * surfaces or as a repo-wide secret pattern. Diagnostics contain only
 * path, line, and pattern name — never the matched text.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { basename, join, relative, sep } from "node:path";
import { pathToFileURL } from "node:url";

const SKIPPED_DIRECTORIES = new Set(["node_modules", "target", ".git", "dist"]);

const AWS_SAMPLE_KEY = "AKIAIOSFODNN7EXAMPLE";

const SURFACE_PATTERNS = [
  { name: "openai-key-shape", regex: /sk-[A-Za-z0-9_-]{16,}/g },
  { name: "aws-access-key-shape", regex: /AKIA[0-9A-Z]{16}/g },
  { name: "bearer-token-shape", regex: /Bearer\s+[A-Za-z0-9._-]{8,}/g },
  {
    name: "credential-assignment-shape",
    regex: /\b(api[_-]?key|secret|token|credential)\s*=\s*"(?!\$\{|env:)[^"]{8,}"/gi,
  },
];

const REPO_WIDE_PATTERNS = [
  { name: "private-key-block", regex: /-----BEGIN [A-Z ]*PRIVATE KEY-----/g },
  { name: "github-pat-classic-shape", regex: /ghp_[A-Za-z0-9]{30,}/g },
  { name: "github-pat-fine-grained-shape", regex: /github_pat_[A-Za-z0-9_]{20,}/g },
  { name: "slack-token-shape", regex: /xox[baprs]-[A-Za-z0-9-]{10,}/g },
  { name: "openai-project-key-shape", regex: /sk-(proj|live|svcacct)-[A-Za-z0-9_-]{20,}/g },
  { name: "aws-access-key-shape", regex: /AKIA[0-9A-Z]{16}/g },
];

function lineAt(text, offset) {
  let line = 1;
  for (let index = 0; index < offset; index += 1) {
    if (text.charCodeAt(index) === 10) {
      line += 1;
    }
  }
  return line;
}

function isSurfaceFile(fileName) {
  if (fileName === "Cargo.toml" || fileName === "Cargo.lock") {
    return false;
  }
  return fileName.endsWith(".toml") || fileName.endsWith(".lock");
}

function collectFiles(root) {
  const files = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.isSymbolicLink()) {
        continue;
      }
      const fullPath = join(directory, entry.name);
      if (entry.isDirectory()) {
        if (!SKIPPED_DIRECTORIES.has(entry.name)) {
          walk(fullPath);
        }
      } else if (entry.isFile()) {
        files.push(fullPath);
      }
    }
  };
  walk(root);
  return files;
}

/**
 * Resolve the git-ignored subset of the given root-relative paths with one
 * batched `git check-ignore --stdin` call, so git itself owns ignore
 * matching. A path a normal `git add` cannot publish must not fail this
 * publication guardrail; tracked files and untracked-but-not-ignored files
 * are never in the returned set and stay scanned.
 *
 * Fail closed: when git is unavailable or the workspace is not a repository
 * (launch failure or any exit other than 0/1), return an empty set so every
 * file is scanned as before — never fail open.
 */
function collectIgnoredPaths(root, relativePaths) {
  if (relativePaths.length === 0) {
    return new Set();
  }
  let result;
  try {
    result = spawnSync("git", ["check-ignore", "-z", "--stdin"], {
      cwd: root,
      input: `${relativePaths.join("\0")}\0`,
      encoding: null,
      maxBuffer: 16 * 1024 * 1024,
      shell: false,
      windowsHide: true,
    });
  } catch {
    return new Set();
  }
  if (result.error !== undefined || (result.status !== 0 && result.status !== 1)) {
    return new Set();
  }
  const ignored = new Set();
  for (const ignoredPath of result.stdout.toString("utf8").split("\0")) {
    if (ignoredPath.length > 0) {
      ignored.add(ignoredPath);
    }
  }
  return ignored;
}

export function runCheck(root) {
  const violations = [];
  const files = collectFiles(root);
  const relativeByFile = new Map(
    files.map((fullPath) => [fullPath, relative(root, fullPath).split(sep).join("/")]),
  );
  const ignored = collectIgnoredPaths(root, [...relativeByFile.values()]);

  for (const fullPath of files) {
    if (ignored.has(relativeByFile.get(fullPath))) {
      continue;
    }
    let stat;
    try {
      stat = statSync(fullPath);
    } catch {
      continue;
    }
    if (stat.size > 2 * 1024 * 1024) {
      continue;
    }

    let bytes;
    try {
      bytes = readFileSync(fullPath);
    } catch {
      continue;
    }

    const head = bytes.subarray(0, Math.min(bytes.length, 8192));
    if (head.includes(0)) {
      continue;
    }

    let text;
    try {
      text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch {
      continue;
    }

    const relativePath = relative(root, fullPath).split(sep).join("/");
    const fileName = basename(fullPath);
    const surface = isSurfaceFile(fileName);
    const patterns = surface ? SURFACE_PATTERNS : REPO_WIDE_PATTERNS;

    for (const pattern of patterns) {
      pattern.regex.lastIndex = 0;
      for (const match of text.matchAll(pattern.regex)) {
        const matched = match[0];
        if (pattern.name === "aws-access-key-shape" && matched.includes(AWS_SAMPLE_KEY)) {
          // Allow the exact documented AWS sample key.
          // The match itself is the key, so compare directly.
          if (matched === AWS_SAMPLE_KEY) {
            continue;
          }
          // Defensive: if the regex matched a substring containing the sample, skip it.
          // The sample is exactly 20 chars starting with AKIA, so the regex match
          // equals the sample when it is the sample.
        }
        violations.push({
          path: relativePath,
          line: lineAt(text, match.index ?? 0),
          pattern: pattern.name,
        });
      }
    }
  }

  violations.sort(
    (left, right) =>
      left.path.localeCompare(right.path) ||
      left.line - right.line ||
      left.pattern.localeCompare(right.pattern),
  );

  return { ok: violations.length === 0, violations };
}

function main() {
  const root = join(import.meta.dirname, "..");
  const result = runCheck(root);
  if (!result.ok) {
    console.error("Secret-hygiene violations:");
    for (const violation of result.violations) {
      console.error(`  - ${violation.path}:${violation.line}: ${violation.pattern}`);
    }
    process.exit(1);
  }
  console.log(
    "Secret-hygiene check passed: no credential-shaped values in portable or tracked surfaces.",
  );
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
