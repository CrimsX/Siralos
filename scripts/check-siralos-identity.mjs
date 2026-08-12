/**
 * Siralos identity ratchet.
 *
 * Deterministic check that no project-owned file contains the former
 * project identity "solaris" (any casing: Solaris / SOLARIS / solaris,
 * and therefore also derived forms such as @solaris, .solaris,
 * solaris-*, SOLARIS_*, and ~/.solaris).
 *
 * Exclusions are narrow and documented in EXCLUSIONS below:
 * - the rename-verification mechanism itself (this file and its test),
 *   which must name the old identity to search for it.
 * Unrelated external terminology (for example Oracle Solaris) would also
 * be excluded here if it existed in this repository; none does.
 *
 * The check walks the working tree (skipping ignored build/dependency
 * directories) rather than `git ls-files`, so untracked project-owned
 * files are covered too.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { pathToFileURL } from "node:url";

/** Former project identity, any casing. */
const OLD_IDENTITY_PATTERN = /solaris/i;

/** Directories never scanned: dependencies, build output, VCS metadata. */
const SKIPPED_DIRECTORIES = new Set([
  "node_modules",
  "dist",
  "target",
  "coverage",
  ".git",
  ".reasonix",
]);

/** Files never scanned: generated TypeScript build artifacts. */
const SKIPPED_FILE_SUFFIXES = [".tsbuildinfo"];

/**
 * Documented exclusions: only content that must name the old identity to
 * do its job. Paths are relative to the repository root.
 * - the rename-verification mechanism itself and its tests (search
 *   patterns);
 * - the migration decision record, which must name the former identity
 *   to record the rename;
 * - migration tests whose fixtures intentionally carry the old identity.
 */
const EXCLUSIONS = new Map([
  [
    "scripts/check-siralos-identity.mjs",
    "the rename-verification mechanism itself: it must name the old identity to search for it",
  ],
  [
    "scripts/check-siralos-identity.test.mjs",
    "tests of the rename-verification mechanism intentionally contain the old identity",
  ],
  [
    "docs/adr/0032-rust-migration-and-siralos-rename.md",
    "the migration decision record must name the former identity to document the rename",
  ],
  [
    "scripts/check-rust-architecture.test.mjs",
    "migration test fixtures intentionally use the former identity as a wrong-value example",
  ],
]);

/**
 * Walk the working tree beneath `root` and return project-owned file
 * paths relative to `root`.
 */
export function collectProjectFiles(root) {
  const files = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory)) {
      const full = join(directory, entry);
      const stat = statSync(full);
      if (stat.isDirectory()) {
        if (!SKIPPED_DIRECTORIES.has(entry)) {
          walk(full);
        }
      } else if (!SKIPPED_FILE_SUFFIXES.some((suffix) => entry.endsWith(suffix))) {
        // Normalize to forward slashes so exclusions and diagnostics are
        // platform-independent.
        files.push(relative(root, full).split(sep).join("/"));
      }
    }
  };
  walk(root);
  return files.sort();
}

/**
 * Scan `files` (paths relative to `root`) for the old identity.
 *
 * Returns violations as `{ file, line, text }` records, oldest identity
 * occurrence first per file.
 */
export function findOldIdentityViolations(root, files) {
  const violations = [];
  for (const file of files) {
    if (EXCLUSIONS.has(file)) {
      continue;
    }
    const content = readFileSync(join(root, file), "utf8");
    const lines = content.split("\n");
    for (let index = 0; index < lines.length; index += 1) {
      const text = lines[index];
      if (OLD_IDENTITY_PATTERN.test(text)) {
        violations.push({ file, line: index + 1, text: text.trim() });
      }
    }
  }
  return violations;
}

/** Run the identity check against `root`. */
export function runCheck(root) {
  const files = collectProjectFiles(root);
  const violations = findOldIdentityViolations(root, files);
  return { ok: violations.length === 0, violations };
}

function main() {
  const root = join(import.meta.dirname, "..");
  const { violations } = runCheck(root);
  if (violations.length > 0) {
    console.error("Identity violations: the former project identity remains:");
    for (const violation of violations) {
      console.error(`  - ${violation.file}:${violation.line}: ${violation.text}`);
    }
    process.exit(1);
  }
  console.log("Identity check passed: no project-owned file uses the former identity.");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
