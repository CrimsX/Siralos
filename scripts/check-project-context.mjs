/** Deterministic completeness check for the public development bootstrap. */
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const CONTEXT_SECTIONS = [
  "## 1. Product definition",
  "## 2. Current implementation reality",
  "## 3. Current roadmap position",
  "## 4. Stage 3R migration track",
  "## 5. Rust engineering direction",
  "## 6. Architecture",
  "## 7. Permanent security model",
  "## 8. Mutation model",
  "## 9. Context model",
  "## 10. Provider model",
  "## 11. Godot domain policy",
  "## 12. Domain host boundary",
  "## 13. R2 differential contract",
  "## 14. H1 / H2 / ICM / H3",
  "## 15. Stage 4 direction",
  "## 16. Stage 5 / 6 guardrails",
  "## 17. Harness-derived design lessons",
  "## 18. Anti-patterns",
  "## 19. Verification model",
  "## 20. Authoritative documentation index",
  "## 21. Prompt / goal generation rules",
  "## 22. New-session bootstrap",
];

const REQUIRED_METADATA = new Map([
  ["Project", "Siralos"],
  ["Context schema", "1"],
  ["Status", "Active development"],
  ["Public stages", "6"],
  ["Migration track", "Stage 3R"],
  ["Current completed milestone", "R10"],
  ["Next milestone", "R11 - Full differential, effect-boundary, security, recovery, and cross-platform parity"],
  ["Canonical repository", "https://github.com/CrimsX/Siralos"],
]);

function expectedIds(prefix, count, width) {
  return Array.from(
    { length: count },
    (_, index) => `${prefix}-${String(index + 1).padStart(width, "0")}`,
  );
}

const REGISTER_EXPECTATIONS = [
  { key: "core", pattern: /^\|\s*(CORE-\d{3})\s*\|/gm, ids: expectedIds("CORE", 20, 3) },
  { key: "harness", pattern: /^\|\s*(HAR-\d{3})\s*\|/gm, ids: expectedIds("HAR", 56, 3) },
  { key: "anti-pattern", pattern: /^\|\s*(AP-\d{3})\s*\|/gm, ids: expectedIds("AP", 17, 3) },
  { key: "RFC", pattern: /^\|\s*(RFC-\d{4})\s*\|/gm, ids: expectedIds("RFC", 20, 4) },
  { key: "golden trace", pattern: /^\|\s*(GT-\d{3})\s*\|/gm, ids: expectedIds("GT", 19, 3) },
];

function parseMetadata(context) {
  const metadata = new Map();
  for (const line of context.split(/\r?\n/)) {
    const match = /^([^:]+):\s*(.+)$/.exec(line);
    if (match !== null) {
      metadata.set(match[1], match[2]);
    }
  }
  return metadata;
}

function collectIds(text, pattern) {
  pattern.lastIndex = 0;
  return [...text.matchAll(pattern)].map((match) => match[1]);
}

/** Validate public context and registry content without filesystem effects. */
export function validateProjectContext(input) {
  const violations = [];
  const metadata = parseMetadata(input.context);

  for (const [key, expected] of REQUIRED_METADATA) {
    if (metadata.get(key) !== expected) {
      violations.push(`metadata ${key} must equal ${expected}`);
    }
  }
  const verifiedCommit = metadata.get("Last verified commit") ?? "";
  if (!/^[0-9a-f]{40}$/.test(verifiedCommit)) {
    violations.push("Last verified commit must be a full lowercase Git object ID");
  } else if (!input.commitExists(verifiedCommit)) {
    violations.push("Last verified commit does not resolve to a commit");
  }

  let previousOffset = -1;
  for (const section of CONTEXT_SECTIONS) {
    const offset = input.context.indexOf(section);
    if (offset < 0) {
      violations.push(`missing context section: ${section}`);
    } else if (offset <= previousOffset) {
      violations.push(`context section is out of order: ${section}`);
    }
    previousOffset = offset;
  }

  if (!input.agents.includes("docs/development/PROJECT_CONTEXT.md")) {
    violations.push("AGENTS.md does not bootstrap PROJECT_CONTEXT.md");
  }
  if (
    !input.context.includes("R4      COMPLETE") ||
    !input.context.includes("R5      COMPLETE") ||
    !input.context.includes("R6      COMPLETE") ||
    !input.context.includes("R7      COMPLETE") ||
    !input.context.includes("R8      COMPLETE") ||
    !input.context.includes("R9      COMPLETE") ||
    !input.context.includes("R10     COMPLETE")
  ) {
    violations.push("project context does not record R4-R10 complete");
  }

  const sources = {
    core: input.requirements,
    harness: input.requirements,
    "anti-pattern": input.requirements,
    RFC: input.rfc,
    "golden trace": input.golden,
  };
  for (const expectation of REGISTER_EXPECTATIONS) {
    const actual = collectIds(sources[expectation.key], expectation.pattern);
    if (JSON.stringify(actual) !== JSON.stringify(expectation.ids)) {
      violations.push(`${expectation.key} registry must contain its exact ordered ID range once`);
    }
  }

  return violations;
}

function commitExists(root, objectId) {
  const result = spawnSync(
    "git",
    ["-c", `safe.directory=${root}`, "cat-file", "-e", `${objectId}^{commit}`],
    {
      cwd: root,
      shell: false,
      windowsHide: true,
      stdio: "ignore",
      env: {
        ...process.env,
        GIT_CONFIG_NOSYSTEM: "1",
        GIT_OPTIONAL_LOCKS: "0",
        GIT_TERMINAL_PROMPT: "0",
      },
    },
  );
  return result.status === 0 && result.error === undefined;
}

export function runCheck(root) {
  const read = (path) => readFileSync(join(root, path), "utf8");
  return validateProjectContext({
    context: read("docs/development/PROJECT_CONTEXT.md"),
    agents: read("AGENTS.md"),
    requirements: read("docs/requirements/REQUIREMENTS.md"),
    rfc: read("docs/architecture/RFC_INDEX.md"),
    golden: read("docs/development/GOLDEN_TRACES.md"),
    commitExists: (objectId) => commitExists(root, objectId),
  });
}

function main() {
  const root = join(import.meta.dirname, "..");
  let violations;
  try {
    violations = runCheck(root);
  } catch {
    console.error("Project-context check could not read its authoritative inputs.");
    process.exit(1);
  }
  if (violations.length > 0) {
    console.error("Project-context violations:");
    for (const violation of violations) {
      console.error(`  - ${violation}`);
    }
    process.exit(1);
  }
  console.log("Project-context check passed: bootstrap and registries are complete.");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
