#!/usr/bin/env node
/**
 * Nondeterminism audit (Stage 3 — Deterministic Execution &
 * Reproducibility, ADR 0029).
 *
 * Bounded static scan for ambient nondeterminism in core/application
 * decision paths. It flags UNCONTROLLED decision inputs, not normal
 * platform APIs repository-wide: not every occurrence is a violation.
 * Explicit adapter-bound or justified uses live in the allowlist below.
 *
 * Patterns scanned (source text, comment-aware):
 *   Date.now / new Date          ambient wall-clock time
 *   Math.random                  ambient randomness
 *   process.env / process.cwd    ambient environment
 *   readdir / readdirSync        filesystem enumeration order
 *   randomUUID                   random identity used as decision input
 *
 * The audit does not run tests and never mutates anything.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const SCAN_DIRECTORIES = [
  join(root, "packages", "core", "src"),
  join(root, "packages", "adapters", "src"),
  join(root, "apps", "cli", "src"),
  join(root, "tests"),
];

/**
 * Allowlist entries: package-relative path (forward slashes) + the
 * pattern(s) it is allowed to use. Justified uses are adapter boundaries,
 * test fixtures, the CLI boundary, or explicitly documented deterministic
 * seams (e.g. the explicit system clock in determinism/context.ts).
 *
 * The audit is strict only for CORE PRODUCTION files: adapters own
 * external nondeterminism (deadlines, enumeration, ids), the CLI is the
 * composition boundary, and tests are fixtures — those are allowed by
 * default. Any ambient pattern in core production (outside this list) is
 * an uncontrolled decision input.
 */
const CORE_PRODUCTION_PREFIX = join("packages", "core", "src");

const CORE_ALLOWLIST = [
  // Explicit clock/randomness seams (ADR 0029).
  {
    path: "packages/core/src/determinism/context.ts",
    patterns: ["Date.now", "Math.random", "crypto"],
  },
  // Real-time observation at the application boundary (command duration).
  { path: "packages/core/src/application/application.ts", patterns: ["Date.now"] },
  // Report timestamp (real-time observation, not a decision input).
  { path: "packages/core/src/doctor/capability-doctor.ts", patterns: ["Date.now"] },
  // Rendering a recorded retrieval timestamp.
  { path: "packages/core/src/research/research-service.ts", patterns: ["new Date"] },
  // Injected now() default for the task runtime (hosts pass explicit clocks).
  { path: "packages/core/src/tasks/task-runtime.ts", patterns: ["Date.now"] },
];

const PATTERNS = [
  { name: "Date.now", regex: /\bDate\.now\s*\(/ },
  { name: "new Date", regex: /\bnew\s+Date\s*\(/ },
  { name: "Math.random", regex: /\bMath\.random\s*\(/ },
  { name: "process.env", regex: /\bprocess\.env\b/ },
  { name: "process.cwd", regex: /\bprocess\.cwd\s*\(/ },
  { name: "readdir", regex: /\breaddir(?:Sync)?\s*\(/ },
  { name: "randomUUID", regex: /\brandomUUID\s*\(/ },
];

function collectSourceFiles(directory, output) {
  for (const entry of readdirSync(directory)) {
    const absolute = join(directory, entry);
    const stats = statSync(absolute);
    if (stats.isDirectory()) {
      if (entry === "node_modules" || entry === "dist") {
        continue;
      }
      collectSourceFiles(absolute, output);
    } else if (entry.endsWith(".ts") || entry.endsWith(".mjs")) {
      output.push(absolute);
    }
  }
}

function isAllowed(relativePath, patternName) {
  const normalized = relativePath.split(sep).join("/");
  const isCoreProduction =
    normalized.startsWith(`${CORE_PRODUCTION_PREFIX.split(sep).join("/")}/`) &&
    !normalized.endsWith(".test.ts");
  if (!isCoreProduction) {
    // Adapters own external nondeterminism; tests are fixtures; the CLI
    // is the composition boundary.
    return true;
  }
  return CORE_ALLOWLIST.some(
    (entry) =>
      (normalized === entry.path || normalized.startsWith(`${entry.path}/`)) &&
      (entry.patterns.length === 0 || entry.patterns.includes(patternName)),
  );
}

function runAudit() {
  const files = [];
  for (const directory of SCAN_DIRECTORIES) {
    collectSourceFiles(directory, files);
  }
  const findings = [];
  let scanned = 0;
  for (const file of files) {
    const relativePath = relative(root, file);
    const source = readFileSync(file, "utf8");
    scanned += 1;
    for (const pattern of PATTERNS) {
      if (!pattern.regex.test(source)) {
        continue;
      }
      if (isAllowed(relativePath, pattern.name)) {
        continue;
      }
      const lines = source.split("\n");
      const hits = [];
      for (let index = 0; index < lines.length; index += 1) {
        if (pattern.regex.test(lines[index])) {
          hits.push(index + 1);
        }
      }
      findings.push(`${relativePath}: ${pattern.name} at line(s) ${hits.join(", ")}`);
    }
  }
  if (findings.length > 0) {
    console.error(
      `Nondeterminism audit failed: ${findings.length} uncontrolled decision input(s).`,
    );
    for (const finding of findings) {
      console.error(`  ${finding}`);
    }
    process.exitCode = 1;
  } else {
    console.log(
      `Nondeterminism audit passed (${scanned} files scanned, no uncontrolled decision inputs).`,
    );
  }
}

runAudit();
