/**
 * Documentation-truth gate for the 1.0 truth pass.
 *
 * SCOPE, STATED PLAINLY SO IT CANNOT DRIFT: this checks EXISTENCE AND
 * RESOLUTION ONLY. It never judges whether a sentence is true, and it cannot:
 * a checker that pretended to verify prose would be the very thing it polices.
 * What it does check:
 *
 *   1. an inline-code repo-relative path token in a live document resolves to
 *      a tracked path;
 *   2. an `npm run <script>` cited in prose names a real package.json script;
 *   3. commit-SHA / "Verified" milestone claims appear only in the canonical
 *      status file (ROADMAP.md) — the repository restated them in five places,
 *      which is why they rotted;
 *   4. no live document carries a single line above the readability bound.
 *
 * It refuses to check, by design: docs/wayfinder/** and docs/adr/** (a dated
 * decision record is history, not documentation), freeze evidence, archives,
 * external URLs, and any semantic claim whatsoever.
 *
 * It never rewrites anything; it reports and exits non-zero.
 *
 * THREE BOUNDARIES, STATED SO THEY CANNOT BECOME HIDING PLACES:
 *   1. Rule 3 (milestone claims) scans PROSE ONLY: fenced code blocks are
 *      blanked first, because a fenced block is quoted material or a
 *      machine-read contract rather than a claim. The live case is
 *      PROJECT_CONTEXT.md's header, which check-project-context.mjs
 *      exact-matches and which requires a full commit id — without this
 *      boundary the two gates contradict each other. The cost is that prose
 *      placed inside a fence is invisible to rule 3; keep claims out of fences.
 *   2. This gate reads tracked files only (git ls-files). An untracked
 *      document is invisible to it until it is committed, so a new document
 *      is unchecked until it lands.
 *   3. Rules 1 and 2 (path tokens, cited npm scripts) do scan fenced blocks —
 *      a dead path or a nonexistent script is wrong wherever it is written.
 */
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

/** The one document allowed to carry milestone status and commit claims. */
const CANONICAL_STATUS_FILE = "ROADMAP.md";

/** Directories whose documents are records, not live documentation. */
const REFUSED_PREFIXES = [
  "docs/wayfinder/",
  "docs/adr/",
  "docs/archive/",
  "tests/differential/evidence/",
  ".plan/",
];

/** Documents subject to the readability bound. */
const LINE_LENGTH_BOUND = 2000;
const LINE_LENGTH_FILES = ["README.md", "AGENTS.md", "docs/development/PROJECT_CONTEXT.md"];

/** Top-level directories a repo-relative token may begin with. */
const REPO_ROOTS = [
  "crates/",
  "docs/",
  "scripts/",
  "tests/",
  "harness/",
  "fuzz/",
  "experiments/",
  "schemas/",
  ".github/",
];

/** Every tracked path in the repository. */
function trackedPaths(root) {
  const output = execFileSync("git", ["ls-files", "-z"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  return new Set(output.split("\0").filter((entry) => entry.length > 0));
}

/** Tracked paths as a sorted list of markdown documents. */
function markdownDocuments(paths) {
  return [...paths].filter((path) => path.endsWith(".md")).sort();
}

/** Whether a document is a record rather than live documentation. */
function isRefused(path) {
  return REFUSED_PREFIXES.some((prefix) => path.startsWith(prefix));
}

/** Whether a token looks like a repository-relative path. */
function looksRepoRelative(token) {
  if (token.includes("://") || token.startsWith("@") || token.includes("*")) {
    return false;
  }
  if (REPO_ROOTS.some((root) => token.startsWith(root))) {
    return true;
  }
  return /^(README|AGENTS|ARCHITECTURE|ROADMAP|CONTRIBUTING|SECURITY|ENGINEERING)\.md$/.test(token);
}

/** Strip an anchor, a line suffix, or a trailing slash from a token. */
function normalizeToken(token) {
  return token.split("#")[0].split(":")[0].replace(/\/+$/, "");
}

/** Ignored paths: real locations a document may legitimately name. */
function ignoredPaths(root) {
  const output = execFileSync(
    "git",
    ["ls-files", "-z", "--others", "--ignored", "--exclude-standard"],
    { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  return new Set(output.split("\0").filter((entry) => entry.length > 0));
}

/** Resolve a path token against the tracked set (file or directory). */
function resolves(token, tracked) {
  if (tracked.has(token)) {
    return true;
  }
  const prefix = `${token}/`;
  for (const path of tracked) {
    if (path.startsWith(prefix)) {
      return true;
    }
  }
  return false;
}

/**
 * The document with fenced code blocks blanked out, offsets preserved.
 *
 * A fenced block is quoted material or a machine-read metadata contract, not a
 * prose claim: PROJECT_CONTEXT.md's header block is parsed and exact-matched by
 * `check-project-context.mjs`, which requires a full commit id there. Checking
 * prose only is what keeps this gate and that contract from contradicting each
 * other.
 */
function proseOnly(text) {
  let fenced = false;
  return text
    .split("\n")
    .map((line) => {
      if (/^\s*```/.test(line)) {
        fenced = !fenced;
        return " ".repeat(line.length);
      }
      return fenced ? " ".repeat(line.length) : line;
    })
    .join("\n");
}

/** A line number for a character offset. */
function lineAt(text, offset) {
  let line = 1;
  for (let index = 0; index < offset && index < text.length; index += 1) {
    if (text.charCodeAt(index) === 10) {
      line += 1;
    }
  }
  return line;
}

/** Run every documentation-truth check; returns a list of error strings. */
export function runCheck(root) {
  const errors = [];
  const tracked = trackedPaths(root);
  const ignored = ignoredPaths(root);
  const scripts = Object.keys(
    JSON.parse(readFileSync(join(root, "package.json"), "utf8")).scripts ?? {},
  );

  for (const path of markdownDocuments(tracked)) {
    if (isRefused(path)) {
      continue;
    }
    const text = readFileSync(join(root, path), "utf8");
    const lines = text.split("\n");

    for (const match of text.matchAll(/`([^`\n]+)`/g)) {
      const token = normalizeToken((match[1] ?? "").trim());
      if (!looksRepoRelative(token)) {
        continue;
      }
      // A tracked path or a gitignored one both name a real location; a
      // generated output directory (`tests/differential/out/`) is the latter.
      if (resolves(token, tracked) || resolves(token, ignored)) {
        continue;
      }
      errors.push(
        `${path}:${lineAt(text, match.index ?? 0)}: inline-code path \`${token}\` does not resolve to a tracked or ignored path`,
      );
    }

    for (const match of text.matchAll(/npm run ([a-z0-9:_-]+)/g)) {
      if (!scripts.includes(match[1])) {
        errors.push(
          `${path}:${lineAt(text, match.index ?? 0)}: cites npm script "${match[1]}" that package.json does not define`,
        );
      }
    }

    if (path !== CANONICAL_STATUS_FILE) {
      const prose = proseOnly(text);
      for (const match of prose.matchAll(/\b[0-9a-f]{40}\b/g)) {
        errors.push(
          `${path}:${lineAt(text, match.index ?? 0)}: 40-character commit SHA outside ${CANONICAL_STATUS_FILE}`,
        );
      }
      for (const match of prose.matchAll(/[Vv]erified\b[^.\n]{0,40}\b[0-9a-f]{7,39}\b/g)) {
        errors.push(
          `${path}:${lineAt(text, match.index ?? 0)}: milestone verification claim outside ${CANONICAL_STATUS_FILE}`,
        );
      }
    }

    if (LINE_LENGTH_FILES.includes(path)) {
      lines.forEach((line, index) => {
        if (line.length > LINE_LENGTH_BOUND) {
          errors.push(
            `${path}:${index + 1}: line is ${line.length} characters (bound ${LINE_LENGTH_BOUND})`,
          );
        }
      });
    }
  }

  return errors;
}

function main() {
  const root = join(import.meta.dirname, "..");
  if (!existsSync(join(root, "package.json"))) {
    console.error("documentation-truth: package.json is missing");
    process.exit(2);
  }
  const errors = runCheck(root);
  if (errors.length > 0) {
    console.error("Documentation-truth violations:");
    for (const error of errors) {
      console.error(`  - ${error}`);
    }
    console.error(
      `  ${errors.length} violation(s). This gate checks existence and resolution only; it never rewrites documents.`,
    );
    process.exit(1);
  }
  console.log(
    "Documentation-truth check passed: path tokens resolve, cited npm scripts exist, milestone claims live only in the canonical status file, and no live document exceeds the line bound.",
  );
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
