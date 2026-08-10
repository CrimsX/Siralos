import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";

export function collectWorkspacePackages(root) {
  const rootPackageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
  const workspaces = rootPackageJson.workspaces ?? [];
  const packages = [];
  for (const pattern of workspaces) {
    const directory = join(root, pattern.replace(/\/?\*$/, ""));
    if (!existsSync(directory)) {
      continue;
    }
    for (const entry of readdirSync(directory)) {
      const packageJsonPath = join(directory, entry, "package.json");
      if (!existsSync(packageJsonPath)) {
        continue;
      }
      const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8"));
      packages.push({ name: packageJson.name, path: join(directory, entry), packageJson });
    }
  }
  return packages;
}

export function listSourceFiles(directory) {
  const files = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir)) {
      const full = join(dir, entry);
      if (statSync(full).isDirectory()) {
        walk(full);
      } else if (entry.endsWith(".ts")) {
        files.push(full);
      }
    }
  };
  walk(directory);
  return files;
}

function parseSource(source) {
  return ts.createSourceFile("fixture.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
}

function stringLiteralOf(node) {
  return node !== undefined && ts.isStringLiteral(node) ? node.text : undefined;
}

/**
 * Structural import extraction. Covers static imports, re-exports
 * (`export ... from "..."`), static dynamic imports (`import("...")` with a
 * string literal), and `import x = require("...")`. Specifiers built at
 * runtime (template literals, variables) cannot be resolved statically and
 * are a documented limitation; the module-name rules below also catch the
 * canonical spellings of every dangerous module.
 */
export function extractImportSpecifiers(source) {
  const specifiers = new Set();
  const file = parseSource(source);
  const visit = (node) => {
    if (ts.isImportDeclaration(node)) {
      const specifier = stringLiteralOf(node.moduleSpecifier);
      if (specifier !== undefined) {
        specifiers.add(specifier);
      }
    } else if (ts.isExportDeclaration(node) && node.moduleSpecifier !== undefined) {
      const specifier = stringLiteralOf(node.moduleSpecifier);
      if (specifier !== undefined) {
        specifiers.add(specifier);
      }
    } else if (
      ts.isImportEqualsDeclaration(node) &&
      ts.isExternalModuleReference(node.moduleReference)
    ) {
      const specifier = stringLiteralOf(node.moduleReference.expression);
      if (specifier !== undefined) {
        specifiers.add(specifier);
      }
    } else if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) {
      const specifier = stringLiteralOf(node.arguments[0]);
      if (specifier !== undefined) {
        specifiers.add(specifier);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return [...specifiers];
}

/**
 * Equivalent Node module names normalize to one canonical form so
 * `node:child_process` and `child_process` cannot bypass a rule.
 */
function normalizeModuleName(specifier) {
  return specifier.startsWith("node:") ? specifier.slice("node:".length) : specifier;
}

function isUnder(target, root) {
  return target === root || target.startsWith(root + sep);
}

function isTestSupportFile(file) {
  return (
    file.endsWith(".test.ts") ||
    file.endsWith("workspace-fixtures.ts") ||
    file.endsWith("git-test-support.ts") ||
    file.endsWith("gdscript-development-testing.ts") ||
    file.endsWith("reference-test-support.ts")
  );
}

/**
 * The GDScript development workflow orchestrator (and the change-set
 * executor) must stay Node-infrastructure-free: filesystem reads belong
 * to the change-set preparation module, identity-bound application
 * belongs to the applier's primitives seam, and sockets/processes belong
 * to the LSP and sandbox adapters. The orchestrator only composes ports.
 */
function isDevelopmentWorkflowOrchestrator(packageRelativeFile) {
  return (
    packageRelativeFile ===
      join("src", "godot", "development", "gdscript-development-service.ts") ||
    packageRelativeFile === join("src", "godot", "development", "change-set-executor.ts")
  );
}

/**
 * The quality/reviewer adapter directory (ADR 0013 §71). Deterministic
 * quality gates and the independent reviewer are strictly read-only and
 * isolated from mutation, process, checkpoint, sandbox, and environment
 * machinery: the reviewer can never edit, execute, approve, checkpoint,
 * or alter sandbox rules or provider credentials. These boundaries are
 * enforced structurally in addition to the runtime read-only registry.
 */
const QUALITY_ADAPTER_DIRECTORY = join("src", "godot", "quality");

function isQualityAdapterModule(packageRelativeFile, file) {
  if (isTestSupportFile(file)) {
    return false;
  }
  return packageRelativeFile.startsWith(QUALITY_ADAPTER_DIRECTORY + sep);
}

/**
 * Stage 3 milestone 1: the task runtime is host-owned (single-owner state
 * rule). Core task modules stay provider-, sandbox-, and Godot-free; only
 * the development bridge may map the existing development workflow events
 * into the runtime, and provider adapters can never see or mutate the
 * task runtime surface.
 */
const TASK_RUNTIME_DIRECTORY = join("src", "tasks");

/** Task runtime identifiers adapters must never import. */
const TASK_RUNTIME_BANNED_IDENTIFIERS = new Set([
  "createTaskRuntime",
  "TaskRuntime",
  "TaskHandle",
  "TaskState",
  "TaskRuntimeSnapshot",
  "createTaskRuntimeSnapshot",
  "createTaskContract",
  "createDevelopmentTaskFlow",
  "createDevelopmentTaskContract",
  "TaskActivityEvent",
]);

/**
 * Stage 3 milestone 5 (Part Q #56): reference adapters (src/reference and
 * src/tools/reference) must never grant capabilities. Reference tools may
 * carry only the fixed "reference.inspect" capability string on their Tool
 * definitions; permission evaluation and default-policy construction stay
 * in the core security layer.
 */
const REFERENCE_CAPABILITY_BANNED_IDENTIFIERS = new Set([
  "evaluatePermission",
  "PermissionEvaluation",
  "createDefaultPolicy",
]);

/**
 * Stage 3 milestone 5 (Part Q #56): research adapters implement the
 * ResearchSource/transport ports and must never write project knowledge
 * directly; every fact mutation flows through the single-writer
 * KnowledgeCoordinator, which research adapters never reach.
 */
const KNOWLEDGE_SURFACE_IDENTIFIERS = new Set([
  "KNOWLEDGE_LIMITS",
  "KNOWLEDGE_RETRIEVAL_SCORING",
  "KNOWLEDGE_STATE_VERSION",
  "SUBJECT_KEY_PATTERN",
  "computeKnowledgeFactId",
  "computeKnowledgeStateRevision",
  "freshnessScore",
  "isValidSubjectKey",
  "normalizeFactContent",
  "rejectPolicyShapedContent",
  "tokenizeFactText",
  "KnowledgeActivation",
  "KnowledgeCandidate",
  "KnowledgeConfidence",
  "KnowledgeFactId",
  "KnowledgeFactType",
  "KnowledgeProvenanceRef",
  "KnowledgeRetrievalQuery",
  "KnowledgeRetrievalResult",
  "KnowledgeRetrievalSelection",
  "KnowledgeRetrievalTrace",
  "KnowledgeScope",
  "KnowledgeVolatility",
  "ProjectKnowledgeFact",
  "createKnowledgeCoordinator",
  "KnowledgeCoordinator",
  "KnowledgeCoordinatorOptions",
  "KnowledgeProposalResult",
  "buildGodotProjectKnowledgeCandidates",
  "GodotProjectKnowledgeSeed",
  "KNOWLEDGE_FRAMING_LINE",
  "renderPinnedKnowledge",
  "renderRetrievedKnowledge",
]);

/**
 * Stage 3 milestone 5 (Part Q #56): provider adapters never fetch research
 * directly; the research service surface is consumed only by the tool layer
 * and the composition root.
 */
const RESEARCH_SERVICE_IDENTIFIERS = new Set([
  "createResearchService",
  "ResearchService",
  "ResearchServiceOptions",
  "ResearchEvidence",
  "ResearchFetchResult",
  "ResearchRevisionBound",
  "DEFAULT_RESEARCH_VIEW_MAX_BYTES",
  "formatResearchEvidenceView",
]);

/** Node modules that perform raw network I/O (HTTP client/server, TCP, fetch). */
const NETWORK_IO_MODULES = new Set(["https", "http", "net", "fetch"]);

function isCoreTaskModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(TASK_RUNTIME_DIRECTORY + sep);
}

/**
 * Stage 3 milestone 2: projection modules are provider-neutral pure
 * transformations. They never import provider ports (wire types), the task
 * runtime mutation surface (projectors receive snapshots through injected
 * getters and can never mutate TaskState), or sandbox implementations.
 */
const PROJECTION_DIRECTORY = join("src", "projection");

function isCoreProjectionModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(PROJECTION_DIRECTORY + sep);
}

/**
 * Stage 3 milestone 3: workspace revision identity (handles, registry) and
 * GDScript structural extraction live in core below the CLI and provider
 * adapters. The workspace layer is pure identity/parsing: no provider
 * ports, no task-runtime mutation surface, no sandbox implementations, no
 * checkpoint/mutation machinery (a summary can never mutate), and no Godot
 * modules (the generic digest utility is allowed).
 */
const WORKSPACE_DIRECTORY = join("src", "workspace");

function isCoreWorkspaceModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(WORKSPACE_DIRECTORY + sep);
}

/** The core module that may bridge the development workflow into tasks. */
function isDevelopmentTaskBridge(packageRelativeFile) {
  return (
    packageRelativeFile === join("src", "tasks", "task-development.ts") ||
    packageRelativeFile === join("src", "tasks", "task-development.test.ts")
  );
}

/**
 * Stage 3 milestone 4: instruction modules own instruction model +
 * resolution semantics and stay provider-neutral: no provider ports, no
 * sandbox implementations, no mutation/checkpoint machinery, and no Godot
 * modules beyond the generic digest utility.
 */
const INSTRUCTIONS_DIRECTORY = join("src", "instructions");

function isCoreInstructionModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(INSTRUCTIONS_DIRECTORY + sep);
}

/**
 * Stage 3 milestone 4: knowledge modules are the single-writer knowledge
 * store (model + coordinator + seeding + projection rendering). They never
 * depend on provider ports, sandbox implementations, mutation/checkpoint
 * machinery, workspace mutation surfaces, the projection layer (projection
 * consumes knowledge, never the reverse), or future semantic/vector
 * services. The KnowledgeCoordinator can never invoke workspace mutation.
 */
const KNOWLEDGE_DIRECTORY = join("src", "knowledge");

function isCoreKnowledgeModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(KNOWLEDGE_DIRECTORY + sep);
}

/**
 * Stage 3 milestone 5 (Part Q #56): first-class external read-only
 * References and the ResearchSource abstraction. Reference and research
 * core modules are pure domain models (identity, trust, provenance,
 * bounded evidence); network retrieval happens only inside adapter-owned
 * transports, never in core.
 */
const REFERENCE_DIRECTORY = join("src", "reference");
const RESEARCH_DIRECTORY = join("src", "research");

function isCoreReferenceModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(REFERENCE_DIRECTORY + sep);
}

function isCoreResearchModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(RESEARCH_DIRECTORY + sep);
}

/**
 * Stage 3 milestone 6: self-reference and doctor modules. The built-in
 * @solaris self-reference is host-owned installed-runtime documentation;
 * the CapabilityDoctor is a deterministic read-only orchestrator. Both
 * are pure core domain surfaces: they must never reach the network, never
 * touch files, never import mutation/checkpoint/undo machinery, never
 * re-derive capability resolution (ToolProjector + security stay
 * authoritative), and never depend on CLI rendering (core can never
 * import apps, enforced structurally).
 */
const DOCTOR_DIRECTORY = join("src", "doctor");
const SELF_DIRECTORY = join("src", "self");
const SELF_TOOLS_DIRECTORY = join("src", "tools", "self");

function isCoreDoctorModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(DOCTOR_DIRECTORY + sep);
}

function isCoreSelfModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(SELF_DIRECTORY + sep);
}

function isSelfToolAdapterModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(SELF_TOOLS_DIRECTORY + sep);
}

/** Capability-resolution authority identifiers banned in doctor/self modules. */
const DOCTOR_CAPABILITY_BANNED_IDENTIFIERS = new Set([
  "evaluatePermission",
  "PermissionEvaluation",
  "createDefaultPolicy",
  "createToolProjector",
]);

/** Adapter modules that implement or surface the reference surface. */
function isReferenceAdapterModule(packageRelativeFile) {
  return (
    packageRelativeFile.startsWith(REFERENCE_DIRECTORY + sep) ||
    packageRelativeFile.startsWith(join("src", "tools", "reference") + sep)
  );
}

/** Adapter modules that implement research sources and transports. */
function isResearchAdapterModule(packageRelativeFile) {
  return packageRelativeFile.startsWith(RESEARCH_DIRECTORY + sep);
}

/** Adapter subtrees the quality/reviewer adapter must never import. */
const QUALITY_FORBIDDEN_IMPORT_ROOTS = [
  { root: join("src", "tools", "workspace", "mutations"), label: "workspace mutation adapters" },
  { root: join("src", "process"), label: "process adapters" },
  { root: join("src", "checkpoints"), label: "checkpoint adapters" },
  { root: join("src", "sandbox"), label: "sandbox adapters" },
  { root: join("src", "environment"), label: "child-environment adapters" },
];

const CHILD_PROCESS_MODULE = "child_process";

/** Destructive filesystem APIs tracked structurally. */
const DESTRUCTIVE_FS_APIS = new Set([
  "writeFile",
  "writeFileSync",
  "appendFile",
  "appendFileSync",
  "createWriteStream",
  "unlink",
  "unlinkSync",
  "rename",
  "renameSync",
  "rm",
  "rmSync",
  "rmdir",
  "rmdirSync",
  "copyFile",
  "copyFileSync",
  "truncate",
  "truncateSync",
]);

/**
 * Path-based recursive deletion. Node offers no directory-handle-relative
 * deletion primitive, so recursive removal cannot be made identity-bound.
 * The recursive rule is STRICTER than the destructive-API location rule:
 * `rm`/`rmSync` with `recursive: true` is prohibited in all production
 * code with only the documented test-support and host-side conformance
 * exemptions below, even inside approved mutation directories. A
 * non-recursive `rm(path, { force: true })` is an unlink and stays
 * governed by the destructive-API location rule.
 */
const RECURSIVE_DELETION_APIS = new Set(["rm", "rmSync"]);

/** Exact files exempt from the recursive-deletion rule: the host-side
 * conformance runner's own-artifact cleanup (test support is exempt via
 * isTestSupportFile). Narrow by exact file, never by directory. */
const RECURSIVE_DELETION_EXEMPTIONS = [join("src", "sandbox", "conformance", "run-conformance.ts")];

function isExemptFromRecursiveDeletion(packageRelativeFile) {
  return RECURSIVE_DELETION_EXEMPTIONS.some(
    (exemption) =>
      packageRelativeFile === exemption || packageRelativeFile.startsWith(exemption + sep),
  );
}

function hasRecursiveFlag(argumentTexts) {
  return argumentTexts.some((text) => /recursive\s*:\s*true/.test(text));
}

const FS_MODULES = new Set(["fs", "fs/promises"]);

const CHILD_PROCESS_FUNCTIONS = new Set([
  "exec",
  "execSync",
  "spawn",
  "spawnSync",
  "fork",
  "execFile",
  "execFileSync",
]);

/** Git mutation verbs recognized in spawn argument lists and strings. */
const GIT_MUTATION_VERBS = new Set([
  "add",
  "commit",
  "merge",
  "push",
  "pull",
  "rebase",
  "revert",
  "cherry-pick",
  "reset",
  "restore",
  "checkout",
  "clean",
  "stash",
  "rm",
  "mv",
  "tag",
  "update-ref",
  "branch",
]);

const APPROVED_CHILD_PROCESS_DIRECTORIES = [join("src", "sandbox")];

const APPROVED_MUTATION_DIRECTORIES = [
  join("src", "tools", "workspace", "mutations"),
  join("src", "sandbox", "conformance"),
  join("src", "checkpoints", "filesystem"),
  join("src", "process"),
  // the probe executable-copy staging writes only the verified private
  // executable copy inside the Solaris-created run directory
  join("src", "godot", "process", "executable-copy.ts"),
];

/**
 * Prohibited raw process-execution patterns kept as textual fallbacks for
 * constructs the structural pass cannot represent (e.g. destructured
 * `const { exec } = require(...)`).
 */
const PROHIBITED_PROCESS_PATTERNS = [
  { pattern: /shell:\s*true/, label: "shell: true" },
  { pattern: /execSync\(/, label: "execSync(" },
  { pattern: /spawnSync\(/, label: "spawnSync(" },
  { pattern: /(?<!\.)exec\(/, label: "exec(" },
];

const PROHIBITED_PROCESS_EXEMPTIONS = [
  // embedded probe fixture sources that exercise prohibited operations
  join("src", "sandbox", "conformance"),
];

function containsProcessEnvAccess(source, packageRelativeFile, file) {
  const withoutStrings = source.replace(
    /'(?:[^'\\]|\\.)*'|"(?:[^"\\]|\\.)*"|`(?:[^`\\]|\\.)*`/g,
    "",
  );
  if (!withoutStrings.includes("process.env")) {
    return false;
  }
  if (isTestSupportFile(file)) {
    return false;
  }
  if (packageRelativeFile.startsWith(join("src", "environment"))) {
    return false;
  }
  return true;
}

function isApprovedWriteApiLocation(packageRelativeFile, file) {
  if (isTestSupportFile(file)) {
    return true;
  }
  return APPROVED_MUTATION_DIRECTORIES.some(
    (directory) =>
      packageRelativeFile === directory || packageRelativeFile.startsWith(directory + sep),
  );
}

/** Git mutation command strings prohibited in runtime code. */
const FORBIDDEN_GIT_WRITE_TOKENS = [
  "git add",
  "git commit",
  "git merge",
  "git push",
  "git pull",
  "git rebase",
  "git revert",
  "git cherry-pick",
  "git reset",
  "git restore",
  "git checkout",
  "git clean",
  "git stash",
  "git rm",
  "git mv",
  "git tag",
  "git update-ref",
];

function containsProhibitedProcessPattern(source) {
  return PROHIBITED_PROCESS_PATTERNS.some((entry) => entry.pattern.test(source));
}

function containsForbiddenGitMutationToken(source) {
  return FORBIDDEN_GIT_WRITE_TOKENS.some((token) => source.includes(token));
}

/**
 * Godot probe invocation guardrail. Solaris Godot probes pass exactly one
 * fixed argument (`--version`, `--help`, or `--dump-extension-api`); the
 * probe invocation module must never carry project-affecting option tokens.
 * The check is scoped to the invocation module (runtime files under
 * `src/godot/process` that are not `*-parser.ts` and not tests), so the
 * capability parser, help fixtures, documentation, and tests can still
 * reference those option names.
 */
const FORBIDDEN_GODOT_PROJECT_ARGUMENTS = [
  "--path",
  "--upwards",
  "--import",
  "--editor",
  "--recovery-mode",
  "--scene",
  "--script",
];

function containsForbiddenGodotProjectArgument(source) {
  return FORBIDDEN_GODOT_PROJECT_ARGUMENTS.some((token) => source.includes(token));
}

function isGodotProbeInvocationModule(packageRelativeFile, file) {
  if (isTestSupportFile(file)) {
    return false;
  }
  // The recovery runner is the one legitimate operational user of the
  // project path option and is governed by its own pairing rules below;
  // the check-only runner and the API documentation runner are governed by
  // their own pairing rules as well.
  if (
    packageRelativeFile === GODOT_RECOVERY_RUNNER_FILE ||
    packageRelativeFile === GODOT_CHECK_ONLY_RUNNER_FILE ||
    packageRelativeFile === GODOT_KNOWLEDGE_RUNNER_FILE ||
    packageRelativeFile === GODOT_LSP_RUNNER_FILE
  ) {
    return false;
  }
  if (!packageRelativeFile.startsWith(join("src", "godot", "process") + sep)) {
    return false;
  }
  if (packageRelativeFile.endsWith("-parser.ts")) {
    return false;
  }
  return true;
}

/** The only argument tuples a Solaris Godot probe may pass. */
const ALLOWED_GODOT_PROBE_ARGUMENTS = ["--version", "--help", "--dump-extension-api"];

const GODOT_PROBE_RUNNER_FILE = join("src", "godot", "process", "godot-probe-runner.ts");

/**
 * Fixed Godot probe tuple guardrail. The probe adapter constructs every
 * probe argument array through one narrow `fixedProbeArguments` constructor
 * private to the adapter; the guardrail is the developer-side structural
 * mirror of that runtime boundary (the runtime boundary is the private
 * constructor itself). It detects alternate construction through:
 * non-fixed `--` tokens, string concatenation, arrays imported from moved
 * modules, and tuple construction in any probe module other than the fixed
 * runner.
 */
function checkGodotProbeTupleDiscipline(
  packageRelativeFile,
  file,
  source,
  location,
  analysis,
  errors,
) {
  if (!isGodotProbeInvocationModule(packageRelativeFile, file)) {
    return;
  }
  for (const match of source.matchAll(/"--[a-z][a-z0-9-]*"/g)) {
    const token = match[0].slice(1, -1);
    if (!ALLOWED_GODOT_PROBE_ARGUMENTS.includes(token)) {
      errors.push(
        `${location}: non-fixed Godot probe argument ${match[0]} is prohibited; probes pass exactly one of --version, --help, or --dump-extension-api`,
      );
    }
  }
  if (/\+?\s*"--"|"--"\s*\+/.test(source)) {
    errors.push(
      `${location}: Godot probe arguments must not be constructed by string concatenation`,
    );
  }
  for (const imported of analysis.importedNames) {
    if (/Arguments$|_ARGS$/i.test(imported.originalName)) {
      errors.push(
        `${location}: probe argument arrays must not be imported (${imported.originalName} from ${imported.module}); the fixed constructor in the probe runner is the only builder`,
      );
    }
  }
  if (packageRelativeFile === GODOT_PROBE_RUNNER_FILE) {
    // Engine probing is fail-closed at this stage: the runner never spawns
    // and never constructs an argument tuple, so the fixedProbeArguments
    // requirement is waived for the fail-closed runner only.
    const failClosedRunner =
      /isAvailable\s*\(\s*\)\s*:\s*Promise<boolean>\s*\{\s*return\s+Promise\.resolve\(false\)/.test(
        source,
      ) || /isAvailable\s*\(\)\s*\{\s*return\s+Promise\.resolve\(false\)/.test(source);
    if (!failClosedRunner && !/\bfunction\s+fixedProbeArguments\b/.test(source)) {
      errors.push(
        `${location}: the Godot probe adapter must construct every probe argument tuple through the single fixedProbeArguments constructor`,
      );
    }
  } else if (/"--[a-z]/.test(source)) {
    errors.push(
      `${location}: Godot probe argument construction is allowed only inside the fixedProbeArguments constructor in godot-probe-runner.ts`,
    );
  }
}

/**
 * The recovery-mode editor probe is the one legitimate operational user of
 * `--path`, scoped to a single module. The recovery module must pair the
 * project path with `--editor`, `--headless`, and `--recovery-mode`, must
 * never carry script/scene/import/export/LSP/DAP/debug options, must take
 * the path value from the prepared mirror (never a literal string and never
 * the workspace root), and must never construct argument arrays by string
 * concatenation or import them from another module. These rules are
 * enforced structurally over the module's array literals and string
 * literal concatenation chains, so imported constants, concatenated
 * strings, and spread arrays are caught, not just lexical spellings.
 */
const GODOT_RECOVERY_RUNNER_FILE = join("src", "godot", "process", "godot-recovery-runner.ts");

const GODOT_CHECK_ONLY_RUNNER_FILE = join("src", "godot", "process", "godot-check-only-runner.ts");

const GODOT_KNOWLEDGE_RUNNER_FILE = join("src", "godot", "process", "godot-knowledge-runner.ts");

const GODOT_LSP_RUNNER_FILE = join("src", "godot", "process", "godot-lsp-runner.ts");

const FORBIDDEN_GODOT_RECOVERY_ARGUMENTS = [
  "--script",
  "--scene",
  "--import",
  "--upwards",
  "--export",
  "--build-solutions",
  "--lsp",
  "--dap",
  "--debug-server",
  "--write-movie",
  "--benchmark",
  "--doctool",
  "--main-pack",
];

const REQUIRED_GODOT_RECOVERY_ARGUMENTS = ["--editor", "--headless", "--recovery-mode"];

/**
 * Option tokens present in one source file: every string literal starting
 * with `--` plus every adjacent string-literal concatenation chain starting
 * with `--` (so `"--scr" + "ipt"` still surfaces as `--script`).
 */
function collectOptionTokens(file) {
  const tokens = new Set();
  const visit = (node) => {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      if (node.text.startsWith("--")) {
        tokens.add(node.text);
      }
    } else if (
      ts.isBinaryExpression(node) &&
      node.operatorToken.kind === ts.SyntaxKind.PlusToken &&
      ts.isStringLiteral(node.left)
    ) {
      const chain = [];
      let current = node;
      while (
        ts.isBinaryExpression(current) &&
        current.operatorToken.kind === ts.SyntaxKind.PlusToken
      ) {
        const right = current.right;
        if (ts.isStringLiteral(right) || ts.isNoSubstitutionTemplateLiteral(right)) {
          chain.push(right.text);
        } else {
          chain.length = 0;
          break;
        }
        current = current.left;
      }
      if (chain.length > 0) {
        const head =
          ts.isStringLiteral(current) || ts.isNoSubstitutionTemplateLiteral(current)
            ? current.text
            : "";
        const joined = `${head}${[...chain].reverse().join("")}`;
        if (joined.startsWith("--")) {
          tokens.add(joined);
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return tokens;
}

function checkGodotRecoveryRunner(packageRelativeFile, file, source, location, analysis, errors) {
  if (packageRelativeFile !== GODOT_RECOVERY_RUNNER_FILE) {
    return;
  }
  const parsed = parseSource(source);
  const tokens = collectOptionTokens(parsed);
  for (const token of REQUIRED_GODOT_RECOVERY_ARGUMENTS) {
    if (!tokens.has(token)) {
      errors.push(
        `${location}: the Godot recovery runner must pair the project path with ${token}`,
      );
    }
  }
  for (const token of FORBIDDEN_GODOT_RECOVERY_ARGUMENTS) {
    if (tokens.has(token)) {
      errors.push(
        `${location}: the Godot recovery runner must not pass ${token}; recovery-mode probing is the only allowed editor invocation`,
      );
    }
  }
  if (!tokens.has("--path")) {
    errors.push(`${location}: the Godot recovery runner must pass --path to the disposable mirror`);
  }
  if (/["'`][^"'`\n]*["'`]\s*\+/.test(source) || /\+\s*["'`]/.test(source)) {
    errors.push(
      `${location}: the Godot recovery arguments must not be constructed by string concatenation`,
    );
  }
  for (const imported of analysis.importedNames) {
    if (/Arguments$|_ARGS$/i.test(imported.originalName)) {
      errors.push(
        `${location}: probe argument arrays must not be imported (${imported.originalName} from ${imported.module}); the fixed recovery tuple must be built in the recovery runner module`,
      );
    }
  }
  const visit = (node) => {
    if (ts.isArrayLiteralExpression(node)) {
      const strings = new Set();
      const spreads = [];
      let pathValue = null;
      const elements = node.elements;
      for (let index = 0; index < elements.length; index += 1) {
        const element = elements[index];
        if (ts.isStringLiteral(element) || ts.isNoSubstitutionTemplateLiteral(element)) {
          strings.add(element.text);
          if (element.text === "--path") {
            pathValue = elements[index + 1];
          }
        } else if (ts.isSpreadElement(element)) {
          spreads.push(element.expression.getText(parsed));
        }
      }
      if (strings.has("--path")) {
        for (const spread of spreads) {
          if (analysis.importedNames.some((entry) => entry.local === spread)) {
            errors.push(
              `${location}: recovery argument arrays must not be composed from imported constants (${spread}); the fixed tuple must be built in the recovery runner module`,
            );
          }
        }
        if (pathValue !== null) {
          if (ts.isStringLiteral(pathValue) || ts.isNoSubstitutionTemplateLiteral(pathValue)) {
            errors.push(
              `${location}: the Godot project path must come from the prepared disposable mirror, never from a literal path`,
            );
          } else {
            const valueText = pathValue.getText(parsed);
            if (/\bworkspaceRoot\b/.test(valueText)) {
              errors.push(
                `${location}: the Godot project path must never be the source workspace root`,
              );
            }
          }
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(parsed);
}

/** Modules that may use the disposable mirror or the recovery runner. */
const GODOT_RECOVERY_APPROVED_USERS = [
  join("src", "godot", "probe"),
  join("src", "godot", "mirror"),
  join("src", "godot", "process"),
  join("src", "godot", "diagnostics"),
  join("src", "godot", "lsp"),
];

/**
 * The GDScript check-only runner is the ONLY legitimate `--script`
 * invocation in Solaris. The module must pair `--script` with
 * `--check-only` (the security-relevant invariant), `--path` (only to the
 * disposable mirror), and `--headless`; must never carry scene, editor,
 * import, LSP/DAP, recovery, export, or quit options; must take the path
 * and script values from the prepared mirror (never literals and never the
 * source workspace root); and must never construct argument arrays by
 * string concatenation or import them from another module.
 */
const REQUIRED_GODOT_CHECK_ONLY_ARGUMENTS = ["--headless", "--path", "--script", "--check-only"];

const FORBIDDEN_GODOT_CHECK_ONLY_ARGUMENTS = [
  "--scene",
  "--editor",
  "--import",
  "--upwards",
  "--export",
  "--build-solutions",
  "--lsp",
  "--dap",
  "--debug-server",
  "--recovery-mode",
  "--write-movie",
  "--benchmark",
  "--doctool",
  "--main-pack",
  "--quit",
  "--quit-after",
];

function checkGodotCheckOnlyRunner(packageRelativeFile, file, source, location, analysis, errors) {
  if (packageRelativeFile !== GODOT_CHECK_ONLY_RUNNER_FILE) {
    return;
  }
  const parsed = parseSource(source);
  const tokens = collectOptionTokens(parsed);
  for (const token of REQUIRED_GODOT_CHECK_ONLY_ARGUMENTS) {
    if (!tokens.has(token)) {
      errors.push(`${location}: the GDScript check-only runner must pass ${token}`);
    }
  }
  for (const token of FORBIDDEN_GODOT_CHECK_ONLY_ARGUMENTS) {
    if (tokens.has(token)) {
      errors.push(
        `${location}: the GDScript check-only runner must not pass ${token}; --check-only parsing is the only allowed diagnostic invocation`,
      );
    }
  }
  if (/["'`][^"'`\n]*["'`]\s*\+/.test(source) || /\+\s*["'`]/.test(source)) {
    errors.push(
      `${location}: the GDScript check-only arguments must not be constructed by string concatenation`,
    );
  }
  for (const imported of analysis.importedNames) {
    if (/Arguments$|_ARGS$/i.test(imported.originalName)) {
      errors.push(
        `${location}: check-only argument arrays must not be imported (${imported.originalName} from ${imported.module}); the fixed tuple must be built in the check-only runner module`,
      );
    }
  }
  const visit = (node) => {
    if (ts.isArrayLiteralExpression(node)) {
      const spreads = [];
      let pathValue = null;
      let scriptValue = null;
      const elements = node.elements;
      for (let index = 0; index < elements.length; index += 1) {
        const element = elements[index];
        if (ts.isStringLiteral(element) || ts.isNoSubstitutionTemplateLiteral(element)) {
          if (element.text === "--path") {
            pathValue = elements[index + 1];
          }
          if (element.text === "--script") {
            scriptValue = elements[index + 1];
          }
        } else if (ts.isSpreadElement(element)) {
          spreads.push(element.expression.getText(parsed));
        }
      }
      for (const spread of spreads) {
        if (analysis.importedNames.some((entry) => entry.local === spread)) {
          errors.push(
            `${location}: check-only argument arrays must not be composed from imported constants (${spread}); the fixed tuple must be built in the check-only runner module`,
          );
        }
      }
      for (const value of [pathValue, scriptValue]) {
        if (value !== null) {
          if (ts.isStringLiteral(value) || ts.isNoSubstitutionTemplateLiteral(value)) {
            errors.push(
              `${location}: the GDScript check-only --path and --script values must come from the prepared disposable mirror, never from literal paths`,
            );
          } else {
            const valueText = value.getText(parsed);
            if (/\bworkspaceRoot\b/.test(valueText)) {
              errors.push(
                `${location}: the GDScript check-only --path and --script values must never be the source workspace`,
              );
            }
          }
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(parsed);
}

/**
 * The Godot LSP session runner is the only runtime module that may pair
 * `--lsp-port` with `--recovery-mode`. The module must pass the full fixed
 * headless recovery editor tuple with the mirror project path and the
 * Solaris-allocated loopback port; must never carry scene, script, import,
 * DAP/debug-server, export, or quit options; must take the path and port
 * values from Solaris-owned inputs (never literals and never the source
 * workspace root); and must never construct argument arrays by string
 * concatenation or import them from another module.
 */
const REQUIRED_GODOT_LSP_ARGUMENTS = [
  "--headless",
  "--editor",
  "--recovery-mode",
  "--path",
  "--lsp-port",
];

const FORBIDDEN_GODOT_LSP_ARGUMENTS = [
  "--scene",
  "--script",
  "--import",
  "--upwards",
  "--export",
  "--build-solutions",
  "--dap-port",
  "--debug-server",
  "--write-movie",
  "--benchmark",
  "--doctool",
  "--main-pack",
  "--quit",
  "--quit-after",
];

function checkGodotLSPServerRunner(packageRelativeFile, file, source, location, analysis, errors) {
  if (packageRelativeFile !== GODOT_LSP_RUNNER_FILE) {
    return;
  }
  const parsed = parseSource(source);
  const tokens = collectOptionTokens(parsed);
  for (const token of REQUIRED_GODOT_LSP_ARGUMENTS) {
    if (!tokens.has(token)) {
      errors.push(`${location}: the Godot LSP runner must pass ${token}`);
    }
  }
  for (const token of FORBIDDEN_GODOT_LSP_ARGUMENTS) {
    if (tokens.has(token)) {
      errors.push(
        `${location}: the Godot LSP runner must not pass ${token}; the session is a headless recovery editor over loopback-only LSP`,
      );
    }
  }
  if (/["'`][^"'`\n]*["'`]\s*\+/.test(source) || /\+\s*["'`]/.test(source)) {
    errors.push(
      `${location}: the Godot LSP arguments must not be constructed by string concatenation`,
    );
  }
  for (const imported of analysis.importedNames) {
    if (/Arguments$|_ARGS$/i.test(imported.originalName)) {
      errors.push(
        `${location}: LSP argument arrays must not be imported (${imported.originalName} from ${imported.module}); the fixed tuple must be built in the LSP runner module`,
      );
    }
  }
  const visit = (node) => {
    if (ts.isArrayLiteralExpression(node)) {
      const spreads = [];
      let pathValue = null;
      let portValue = null;
      const elements = node.elements;
      for (let index = 0; index < elements.length; index += 1) {
        const element = elements[index];
        if (ts.isStringLiteral(element) || ts.isNoSubstitutionTemplateLiteral(element)) {
          if (element.text === "--path") {
            pathValue = elements[index + 1];
          }
          if (element.text === "--lsp-port") {
            portValue = elements[index + 1];
          }
        } else if (ts.isSpreadElement(element)) {
          spreads.push(element.expression.getText(parsed));
        }
      }
      for (const spread of spreads) {
        if (analysis.importedNames.some((entry) => entry.local === spread)) {
          errors.push(
            `${location}: LSP argument arrays must not be composed from imported constants (${spread}); the fixed tuple must be built in the LSP runner module`,
          );
        }
      }
      for (const value of [pathValue, portValue]) {
        if (value !== null) {
          if (ts.isStringLiteral(value) || ts.isNoSubstitutionTemplateLiteral(value)) {
            errors.push(
              `${location}: the Godot LSP --path and --lsp-port values must come from the disposable mirror and the Solaris allocator, never from literal values`,
            );
          } else {
            const valueText = value.getText(parsed);
            if (/\bworkspaceRoot\b/.test(valueText)) {
              errors.push(
                `${location}: the Godot LSP project path must never be the source workspace`,
              );
            }
          }
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(parsed);
}

/**
 * The API documentation runner passes exactly one fixed option,
 * `--dump-extension-api-with-docs`, and never anything project-affecting.
 * An ordinary `--dump-extension-api` result is never substituted for the
 * with-docs profile.
 */
function checkGodotKnowledgeRunner(packageRelativeFile, file, source, location, analysis, errors) {
  if (packageRelativeFile !== GODOT_KNOWLEDGE_RUNNER_FILE) {
    return;
  }
  const parsed = parseSource(source);
  const tokens = collectOptionTokens(parsed);
  if (!tokens.has("--dump-extension-api-with-docs")) {
    errors.push(
      `${location}: the API documentation runner must pass exactly --dump-extension-api-with-docs`,
    );
  }
  for (const token of tokens) {
    if (token !== "--dump-extension-api-with-docs") {
      errors.push(
        `${location}: the API documentation runner must pass exactly --dump-extension-api-with-docs (found ${token})`,
      );
    }
  }
  for (const token of FORBIDDEN_GODOT_PROJECT_ARGUMENTS) {
    if (tokens.has(token)) {
      errors.push(
        `${location}: the API documentation runner must not pass ${token}; API generation is project-independent`,
      );
    }
  }
  if (/["'`][^"'`\n]*["'`]\s*\+/.test(source) || /\+\s*["'`]/.test(source)) {
    errors.push(
      `${location}: the API documentation arguments must not be constructed by string concatenation`,
    );
  }
  for (const imported of analysis.importedNames) {
    if (/Arguments$|_ARGS$/i.test(imported.originalName)) {
      errors.push(
        `${location}: API documentation argument arrays must not be imported (${imported.originalName} from ${imported.module}); the fixed tuple must be built in the knowledge runner module`,
      );
    }
  }
}

/**
 * Structural scan of one source file. Returns import bindings (named,
 * namespace, and default) and a list of call targets: for every
 * CallExpression, the resolved module (if the callee comes from an import)
 * and the original imported name, plus the callee text for local/global
 * calls. Aliased imports and renamed functions resolve through the bindings,
 * so `import { rename as evil } from "node:fs/promises"` is caught.
 */
function analyzeSource(source) {
  const file = parseSource(source);
  const bindings = new Map(); // local name -> { module, originalName }
  const namespaceImports = new Map(); // local name -> module
  const calls = []; // { module, api, calleeText }
  const spawnCalls = []; // { calleeText, argumentTexts, shellTrue }
  const destructiveFsImports = []; // { module, api } imported from fs modules
  const importedNames = []; // { local, originalName, module } for every named binding
  const imports = new Set();

  const addCall = (module, api, calleeText, argumentTexts) => {
    calls.push({ module, api, calleeText, argumentTexts });
  };

  const visit = (node) => {
    if (ts.isImportDeclaration(node)) {
      const specifier = stringLiteralOf(node.moduleSpecifier);
      if (specifier !== undefined) {
        imports.add(specifier);
        const module = normalizeModuleName(specifier);
        const clause = node.importClause;
        if (clause !== undefined) {
          if (clause.name !== undefined) {
            bindings.set(clause.name.text, { module, originalName: "default" });
          }
          if (clause.namedBindings !== undefined) {
            if (ts.isNamespaceImport(clause.namedBindings)) {
              namespaceImports.set(clause.namedBindings.name.text, module);
              if (FS_MODULES.has(module)) {
                destructiveFsImports.push({ module, api: "*" });
              }
            } else {
              for (const element of clause.namedBindings.elements) {
                const local = element.name.text;
                const imported = element.propertyName?.text ?? local;
                bindings.set(local, { module, originalName: imported });
                importedNames.push({ local, originalName: imported, module });
                if (FS_MODULES.has(module) && DESTRUCTIVE_FS_APIS.has(imported)) {
                  destructiveFsImports.push({ module, api: imported });
                }
              }
            }
          }
        }
      }
    } else if (ts.isExportDeclaration(node) && node.moduleSpecifier !== undefined) {
      const specifier = stringLiteralOf(node.moduleSpecifier);
      if (specifier !== undefined) {
        imports.add(specifier);
      }
    } else if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) {
      const specifier = stringLiteralOf(node.arguments[0]);
      if (specifier !== undefined) {
        imports.add(specifier);
      }
    } else if (ts.isCallExpression(node)) {
      const argumentTexts = node.arguments.map((argument) => argument.getText(file));
      const shellTrue = argumentTexts.some(
        (text) => text.includes("shell:") && /shell:\s*true/.test(text),
      );
      let calleeText = node.expression.getText(file);
      if (ts.isIdentifier(node.expression)) {
        const binding = bindings.get(node.expression.text);
        if (binding !== undefined) {
          addCall(binding.module, binding.originalName, node.expression.text, argumentTexts);
          if (binding.module === CHILD_PROCESS_MODULE) {
            spawnCalls.push({ calleeText: node.expression.text, argumentTexts, shellTrue });
          }
          if (FS_MODULES.has(binding.module)) {
            spawnCalls.push({ calleeText: node.expression.text, argumentTexts, shellTrue });
          }
        } else {
          spawnCalls.push({ calleeText: node.expression.text, argumentTexts, shellTrue });
        }
      } else if (ts.isPropertyAccessExpression(node.expression)) {
        const objectText = node.expression.expression.getText(file);
        const namespace = namespaceImports.get(objectText);
        if (namespace !== undefined) {
          addCall(namespace, node.expression.name.text, calleeText, argumentTexts);
          if (namespace === CHILD_PROCESS_MODULE || FS_MODULES.has(namespace)) {
            spawnCalls.push({ calleeText, argumentTexts, shellTrue });
          }
        } else {
          spawnCalls.push({ calleeText, argumentTexts, shellTrue });
        }
      } else {
        spawnCalls.push({ calleeText, argumentTexts, shellTrue });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return {
    imports,
    calls,
    spawnCalls,
    destructiveFsImports,
    importedNames,
  };
}

function isGitMutationCall(call) {
  if (!CHILD_PROCESS_FUNCTIONS.has(call.calleeText)) {
    return false;
  }
  const hasGitExecutable = call.argumentTexts.some((text) => /\bgit(?:\.exe)?\b/i.test(text));
  if (!hasGitExecutable) {
    return false;
  }
  return call.argumentTexts.some((text) => {
    const quoted = /["'`]([a-z][a-z-]*)["'`]/gi.exec(text);
    return quoted !== null && GIT_MUTATION_VERBS.has(quoted[1].toLowerCase());
  });
}

export function runChecks(root) {
  const errors = [];
  const packages = collectWorkspacePackages(root);
  const packagesByName = new Map(packages.map((pkg) => [pkg.name, pkg]));

  for (const pkg of packages) {
    const sourceRoot = join(pkg.path, "src");
    if (existsSync(sourceRoot)) {
      for (const file of listSourceFiles(sourceRoot)) {
        const source = readFileSync(file, "utf8");
        const location = relative(root, file).split(sep).join("/");
        const packageRelativeFile = relative(pkg.path, file);
        const analysis = analyzeSource(source);
        if (containsProcessEnvAccess(source, packageRelativeFile, file)) {
          errors.push(
            `${location}: process.env inspection is prohibited in package source; build child environments from an explicit allowlist`,
          );
        }
        if (isTestSupportFile(file) === false) {
          const exempt = PROHIBITED_PROCESS_EXEMPTIONS.some((directory) =>
            packageRelativeFile.startsWith(directory + sep),
          );
          if (containsProhibitedProcessPattern(source) && !exempt) {
            errors.push(
              `${location}: raw process execution (exec, execSync, spawnSync, shell: true) is prohibited outside documented test fixtures`,
            );
          }
          if (
            isGodotProbeInvocationModule(packageRelativeFile, file) &&
            containsForbiddenGodotProjectArgument(source)
          ) {
            errors.push(
              `${location}: project-affecting Godot arguments (--path, --upwards, --import, --editor, --recovery-mode, --scene, --script) are prohibited in probe invocation code`,
            );
          }
          checkGodotProbeTupleDiscipline(
            packageRelativeFile,
            file,
            source,
            location,
            analysis,
            errors,
          );
          checkGodotRecoveryRunner(packageRelativeFile, file, source, location, analysis, errors);
          checkGodotCheckOnlyRunner(packageRelativeFile, file, source, location, analysis, errors);
          checkGodotKnowledgeRunner(packageRelativeFile, file, source, location, analysis, errors);
          checkGodotLSPServerRunner(packageRelativeFile, file, source, location, analysis, errors);
          if (
            pkg.name === "@solaris/adapters" &&
            !isTestSupportFile(file) &&
            isCoreTaskModule(packageRelativeFile) === false
          ) {
            for (const binding of analysis.importedNames) {
              if (
                binding.module === "@solaris/core" &&
                TASK_RUNTIME_BANNED_IDENTIFIERS.has(binding.originalName)
              ) {
                errors.push(
                  `${location}: provider adapters must not import the task runtime surface (${binding.originalName}); TaskState has exactly one authoritative owner — the core TaskRuntime`,
                );
              }
            }
          }
          if (
            pkg.name === "@solaris/adapters" &&
            !isTestSupportFile(file) &&
            isReferenceAdapterModule(packageRelativeFile)
          ) {
            for (const binding of analysis.importedNames) {
              if (
                binding.module === "@solaris/core" &&
                REFERENCE_CAPABILITY_BANNED_IDENTIFIERS.has(binding.originalName)
              ) {
                errors.push(
                  `${location}: reference adapters must not import capability-granting policy (${binding.originalName}); reference tools may carry only the fixed reference.inspect capability string on their Tool definitions`,
                );
              }
            }
          }
          if (
            pkg.name === "@solaris/core" &&
            !isTestSupportFile(file) &&
            (isCoreDoctorModule(packageRelativeFile) || isCoreSelfModule(packageRelativeFile))
          ) {
            for (const binding of analysis.importedNames) {
              if (DOCTOR_CAPABILITY_BANNED_IDENTIFIERS.has(binding.originalName)) {
                errors.push(
                  `${location}: doctor and self-reference modules must not import capability-granting/resolution policy (${binding.originalName}); capability rules arrive through injected sources and ToolProjector stays the authority`,
                );
              }
            }
          }
          if (
            pkg.name === "@solaris/adapters" &&
            !isTestSupportFile(file) &&
            isSelfToolAdapterModule(packageRelativeFile)
          ) {
            for (const binding of analysis.importedNames) {
              if (
                binding.module === "@solaris/core" &&
                DOCTOR_CAPABILITY_BANNED_IDENTIFIERS.has(binding.originalName)
              ) {
                errors.push(
                  `${location}: self-reference tool adapters must not import capability-granting policy (${binding.originalName}); self tools may carry only the fixed self.inspect capability string on their Tool definitions`,
                );
              }
            }
          }
          if (
            pkg.name === "@solaris/adapters" &&
            !isTestSupportFile(file) &&
            isResearchAdapterModule(packageRelativeFile)
          ) {
            for (const binding of analysis.importedNames) {
              if (
                binding.module === "@solaris/core" &&
                KNOWLEDGE_SURFACE_IDENTIFIERS.has(binding.originalName)
              ) {
                errors.push(
                  `${location}: research adapters must not import the project knowledge surface (${binding.originalName}); research adapters never write project knowledge directly`,
                );
              }
            }
          }
          if (
            pkg.name === "@solaris/adapters" &&
            !isTestSupportFile(file) &&
            packageRelativeFile.startsWith(join("src", "providers") + sep)
          ) {
            for (const binding of analysis.importedNames) {
              if (
                binding.module === "@solaris/core" &&
                RESEARCH_SERVICE_IDENTIFIERS.has(binding.originalName)
              ) {
                errors.push(
                  `${location}: provider adapters must not import the research service surface (${binding.originalName}); providers never fetch research directly`,
                );
              }
            }
          }
          for (const imported of analysis.destructiveFsImports) {
            if (!isApprovedWriteApiLocation(packageRelativeFile, file)) {
              errors.push(
                `${location}: direct file write APIs are prohibited: ${imported.api} imported from ${imported.module} outside approved workspace mutation modules and tests`,
              );
            }
          }
          if (
            pkg.name === "@solaris/adapters" &&
            !isTestSupportFile(file) &&
            packageRelativeFile.startsWith(join("src", "godot", "lsp") + sep) &&
            (source.includes("workspace/applyEdit") || source.includes("workspace/executeCommand"))
          ) {
            errors.push(
              `${location}: LSP mutation methods must never be implemented; applyEdit/executeCommand are rejected at the server-request boundary and never referenced in runtime adapter code`,
            );
          }
          if (
            pkg.name === "@solaris/core" &&
            isCoreProjectionModule(packageRelativeFile) &&
            !isTestSupportFile(file) &&
            /\bfetch\s*\(/.test(source)
          ) {
            errors.push(
              `${location}: projection modules must not perform network calls; fetch( is prohibited — ContextProjector builds pure deterministic model context with no network I/O`,
            );
          }
          if (
            pkg.name === "@solaris/core" &&
            (isCoreDoctorModule(packageRelativeFile) || isCoreSelfModule(packageRelativeFile)) &&
            !isTestSupportFile(file) &&
            /\bfetch\s*\(/.test(source)
          ) {
            errors.push(
              `${location}: doctor and self-reference modules must not perform network calls; fetch( is prohibited — the default doctor is offline by construction (a global fetch( needs no import)`,
            );
          }
          if (isDevelopmentWorkflowOrchestrator(packageRelativeFile)) {
            for (const specifier of analysis.imports) {
              if (
                specifier === "node:fs" ||
                specifier === "node:fs/promises" ||
                specifier === "node:net" ||
                specifier === "node:child_process" ||
                specifier === "node:path"
              ) {
                errors.push(
                  `${location}: the GDScript development workflow orchestrator must not import ${specifier}; filesystem, socket, and process concerns stay in the approved change-set preparation and applier modules`,
                );
              }
            }
          }
          if (containsForbiddenGitMutationToken(source)) {
            errors.push(
              `${location}: Git mutation commands (add, commit, reset, restore, checkout, clean, stash, ...) are prohibited in runtime code`,
            );
          }
          for (const call of analysis.calls) {
            if (FS_MODULES.has(call.module) && DESTRUCTIVE_FS_APIS.has(call.api)) {
              if (!isApprovedWriteApiLocation(packageRelativeFile, file)) {
                errors.push(
                  `${location}: direct file write APIs are prohibited: ${call.api} from ${call.module} outside approved workspace mutation modules and tests`,
                );
              }
            }
            if (
              FS_MODULES.has(call.module) &&
              RECURSIVE_DELETION_APIS.has(call.api) &&
              hasRecursiveFlag(call.argumentTexts ?? []) &&
              !isExemptFromRecursiveDeletion(packageRelativeFile)
            ) {
              errors.push(
                `${location}: path-based recursive deletion is prohibited in production code: ${call.api} from ${call.module} with recursive: true; Node offers no directory-handle-relative deletion primitive, so recursive removal cannot be identity-bound and is never offered`,
              );
            }
            if (call.module === CHILD_PROCESS_MODULE) {
              const inApprovedDirectory = APPROVED_CHILD_PROCESS_DIRECTORIES.some((directory) =>
                packageRelativeFile.startsWith(directory + sep),
              );
              if (!inApprovedDirectory) {
                errors.push(
                  `${location}: unsandboxed process spawning is prohibited outside approved sandbox and git modules`,
                );
              }
            }
          }
          for (const call of analysis.spawnCalls) {
            if (call.shellTrue && !exempt) {
              errors.push(
                `${location}: raw process execution with shell: true is prohibited outside documented test fixtures`,
              );
            }
            if (isGitMutationCall(call)) {
              errors.push(
                `${location}: Git mutation commands (add, commit, reset, restore, checkout, clean, stash, ...) are prohibited in runtime code`,
              );
            }
          }
        }
        for (const specifier of analysis.imports) {
          const normalized = normalizeModuleName(specifier);
          if (specifier.startsWith("@anthropic-ai/")) {
            const inRuntimeAdapter = packageRelativeFile.startsWith(
              join("src", "sandbox", "anthropic-runtime"),
            );
            if (!inRuntimeAdapter) {
              errors.push(
                `${location}: Sandbox Runtime may only be imported by the anthropic runtime adapter`,
              );
            }
          }
          if (normalized === CHILD_PROCESS_MODULE && isTestSupportFile(file) === false) {
            const inApprovedDirectory = APPROVED_CHILD_PROCESS_DIRECTORIES.some((directory) =>
              packageRelativeFile.startsWith(directory + sep),
            );
            if (!inApprovedDirectory) {
              errors.push(
                `${location}: unsandboxed process spawning is prohibited outside approved sandbox and git modules`,
              );
            }
          }
          if (pkg.name === "@solaris/core") {
            if (specifier.startsWith("@solaris/")) {
              errors.push(`${location}: core must not import workspace package ${specifier}`);
            }
            if (specifier.startsWith("node:")) {
              errors.push(`${location}: core must not import Node module ${specifier}`);
            }
            if (
              (isCoreReferenceModule(packageRelativeFile) ||
                isCoreResearchModule(packageRelativeFile)) &&
              NETWORK_IO_MODULES.has(normalized)
            ) {
              errors.push(
                `${location}: reference and research core modules must not import network modules (${specifier}); fetching belongs to the adapter-owned research transports, never core domain models`,
              );
            }
          }
          if (pkg.name === "@solaris/core" && isCoreWorkspaceModule(packageRelativeFile)) {
            if (specifier.startsWith("../ports/")) {
              errors.push(
                `${location}: workspace revision modules must not depend on provider ports; revision identity is provider-neutral`,
              );
            }
            if (specifier.startsWith("../tasks/task-runtime")) {
              errors.push(
                `${location}: workspace revision modules must not import the task runtime mutation surface; revisions are identity, not authority`,
              );
            }
            if (specifier.startsWith("../security/sandbox-")) {
              errors.push(
                `${location}: workspace revision modules must not depend on sandbox implementations`,
              );
            }
            if (
              specifier.startsWith("../checkpoints/") ||
              specifier.startsWith("../godot/development/") ||
              specifier.startsWith("../tools/")
            ) {
              errors.push(
                `${location}: workspace revision modules must not import mutation/checkpoint machinery; summary and structural views can never mutate files`,
              );
            }
            if (specifier.startsWith("../godot/") && specifier !== "../godot/digest.js") {
              errors.push(
                `${location}: workspace revision modules must not depend on Godot modules (the generic digest utility is allowed)`,
              );
            }
          }
          if (pkg.name === "@solaris/core" && isCoreProjectionModule(packageRelativeFile)) {
            if (NETWORK_IO_MODULES.has(normalized)) {
              errors.push(
                `${location}: projection modules must not import network modules (${specifier}); ContextProjector performs no network calls`,
              );
            }
            if (specifier.startsWith("../doctor/") || specifier.startsWith("../self/")) {
              errors.push(
                `${location}: projection modules must not import the doctor or self-reference surface (${specifier}); ContextProjector never runs doctor automatically and ToolProjector stays the authority for model-visible tool state`,
              );
            }
            if (specifier.startsWith("../workspace/workspace-revision")) {
              errors.push(
                `${location}: projection modules must not own workspace revisions; they consume revision handles as data only`,
              );
            }
            if (specifier.startsWith("../ports/")) {
              errors.push(
                `${location}: projection modules must not depend on provider ports; projectors build provider-neutral inputs only`,
              );
            }
            if (specifier.startsWith("../tasks/task-runtime")) {
              errors.push(
                `${location}: projection modules must not import the task runtime mutation surface; projectors receive task snapshots through injected getters and can never mutate TaskState`,
              );
            }
            if (specifier.startsWith("../security/sandbox-")) {
              errors.push(
                `${location}: projection modules must not depend on sandbox implementations; projectors classify tool visibility from the capability policy and profile identifiers only`,
              );
            }
            if (specifier.startsWith("../godot/") && specifier !== "../godot/digest.js") {
              errors.push(
                `${location}: projection modules must not depend on Godot modules (the generic digest utility is allowed)`,
              );
            }
          }
          if (
            pkg.name === "@solaris/core" &&
            (isCoreDoctorModule(packageRelativeFile) || isCoreSelfModule(packageRelativeFile))
          ) {
            if (NETWORK_IO_MODULES.has(normalized)) {
              errors.push(
                `${location}: doctor and self-reference modules must not import network modules (${specifier}); default doctor operation is offline`,
              );
            }
            if (
              specifier.startsWith("../checkpoints/") ||
              specifier.startsWith("../undo/") ||
              (specifier.startsWith("../tools/") &&
                specifier !== "../tools/tool.js" &&
                specifier !== "../tools/tool-registry.js") ||
              specifier.startsWith("../workspace/mutation")
            ) {
              errors.push(
                `${location}: doctor and self-reference modules must not import mutation, undo, or checkpoint machinery (${specifier}); the doctor is read-only and never creates checkpoints`,
              );
            }
            if (specifier.startsWith("../security/default-policy")) {
              errors.push(
                `${location}: doctor and self-reference modules must not import default policy construction (${specifier}); capability rules arrive through injected sources, never re-derived`,
              );
            }
            if (specifier.startsWith("../security/permission-evaluator")) {
              errors.push(
                `${location}: doctor and self-reference modules must not import capability evaluation (${specifier}); ToolProjector and the security layer stay authoritative for capability resolution`,
              );
            }
            if (specifier.startsWith("../projection/")) {
              errors.push(
                `${location}: doctor and self-reference modules must not import projection internals (${specifier}); ToolProjector remains the authority for model-visible tool state and is queried through the doctor sources port`,
              );
            }
            if (
              isCoreDoctorModule(packageRelativeFile) &&
              specifier.startsWith("../self/") &&
              specifier !== "../self/self-reference.js"
            ) {
              errors.push(
                `${location}: doctor modules must not import the self-reference surface (${specifier}); the doctor and the self-reference stay separable surfaces (the shared SolarisRuntimeIdentity type is allowed)`,
              );
            }
            if (isCoreSelfModule(packageRelativeFile) && specifier.startsWith("../doctor/")) {
              errors.push(
                `${location}: self-reference modules must not import the doctor surface (${specifier}); the self-reference documents, it never diagnoses`,
              );
            }
            if (
              isCoreDoctorModule(packageRelativeFile) &&
              !isTestSupportFile(file) &&
              specifier.endsWith("safe-report.js")
            ) {
              errors.push(
                `${location}: diagnostic collection (capability-doctor/capability-state) must not import the safe-report renderer (${specifier}); safe report rendering stays separate from diagnostic collection`,
              );
            }
          }
          if (pkg.name === "@solaris/core" && isCoreTaskModule(packageRelativeFile)) {
            if (specifier.startsWith("../ports/")) {
              errors.push(
                `${location}: task runtime modules must not depend on provider ports; the runtime observes typed host facts only`,
              );
            }
            if (specifier.startsWith("../security/sandbox-")) {
              errors.push(
                `${location}: task runtime modules must not depend on sandbox implementations; the runtime snapshots profile identifiers only`,
              );
            }
            if (
              specifier.startsWith("../godot/") &&
              specifier !== "../godot/digest.js" &&
              !isDevelopmentTaskBridge(packageRelativeFile)
            ) {
              errors.push(
                `${location}: task runtime modules must not depend on Godot modules; only the development bridge (task-development.ts) maps workflow events (the generic digest utility is allowed)`,
              );
            }
          }
          if (pkg.name === "@solaris/core" && isCoreInstructionModule(packageRelativeFile)) {
            if (specifier.startsWith("../ports/")) {
              errors.push(
                `${location}: instruction modules must not depend on provider ports; instruction resolution is provider-neutral`,
              );
            }
            if (specifier.startsWith("../security/sandbox-")) {
              errors.push(
                `${location}: instruction modules must not depend on sandbox implementations; security policy stays outside the instruction resolver`,
              );
            }
            if (
              specifier.startsWith("../checkpoints/") ||
              specifier.startsWith("../godot/development/") ||
              specifier.startsWith("../tools/")
            ) {
              errors.push(
                `${location}: instruction modules must not import mutation/checkpoint machinery; resolving guidance can never mutate files`,
              );
            }
            if (specifier.startsWith("../godot/") && specifier !== "../godot/digest.js") {
              errors.push(
                `${location}: instruction modules must not depend on Godot modules (the generic digest utility is allowed)`,
              );
            }
          }
          if (pkg.name === "@solaris/core" && isCoreKnowledgeModule(packageRelativeFile)) {
            if (specifier.startsWith("../ports/")) {
              errors.push(
                `${location}: knowledge modules must not depend on provider ports; knowledge is provider-neutral factual context`,
              );
            }
            if (specifier.startsWith("../security/sandbox-")) {
              errors.push(
                `${location}: knowledge modules must not depend on sandbox implementations; knowledge can never grant capability`,
              );
            }
            if (
              specifier.startsWith("../checkpoints/") ||
              specifier.startsWith("../godot/development/") ||
              specifier.startsWith("../tools/") ||
              specifier.startsWith("../workspace/")
            ) {
              errors.push(
                `${location}: knowledge modules must not import mutation/checkpoint machinery; the KnowledgeCoordinator can never invoke workspace mutation directly`,
              );
            }
            if (specifier.startsWith("../projection/")) {
              errors.push(
                `${location}: knowledge modules must not depend on the projection layer; ContextProjector consumes knowledge, never the reverse`,
              );
            }
            if (specifier.startsWith("../godot/") && specifier !== "../godot/digest.js") {
              errors.push(
                `${location}: knowledge modules must not depend on Godot modules (the generic digest utility is allowed); project knowledge does not depend on engine or future semantic services`,
              );
            }
          }
          if (pkg.name === "@solaris/core" && isCoreProjectionModule(packageRelativeFile)) {
            if (
              packageRelativeFile !== join(PROJECTION_DIRECTORY, "projection-service.ts") &&
              packageRelativeFile !== join(PROJECTION_DIRECTORY, "projection-service.test.ts") &&
              (specifier.startsWith("../instructions/") || specifier.startsWith("../knowledge/"))
            ) {
              errors.push(
                `${location}: only projection-service consumes instruction/knowledge models; ToolProjector and EvidenceProjector never treat knowledge as policy or authority`,
              );
            }
            if (
              specifier === "../knowledge/knowledge-coordinator.js" ||
              specifier === "../knowledge/knowledge-coordinator.test.js"
            ) {
              errors.push(
                `${location}: projection modules consume knowledge through injected projections (structured models), never the concrete KnowledgeCoordinator persistence`,
              );
            }
          }
          if (pkg.name === "@solaris/adapters" && isTestSupportFile(file) === false) {
            if (
              specifier.startsWith("../instructions/") &&
              packageRelativeFile.startsWith(INSTRUCTIONS_DIRECTORY + sep) === false
            ) {
              errors.push(
                `${location}: provider adapters must not discover or resolve project instructions themselves; the instruction service under src/instructions is the only adapter-owned discovery surface`,
              );
            }
          }

          if (pkg.name === "@solaris/adapters" && specifier === "node:net") {
            const socketApproved =
              packageRelativeFile.startsWith(join("src", "godot", "lsp") + sep) ||
              packageRelativeFile.startsWith(join("src", "sandbox") + sep) ||
              isTestSupportFile(file);
            if (!socketApproved) {
              errors.push(
                `${location}: raw TCP socket usage is allowed only inside the approved Godot LSP adapter (src/godot/lsp) and the sandbox adapter (src/sandbox)`,
              );
            }
          }
          if (pkg.name === "@solaris/cli" && specifier === "node:net") {
            errors.push(
              `${location}: the CLI must not open sockets; only the LSP adapter owns TCP`,
            );
          }
          if (pkg.name === "@solaris/adapters" && specifier.startsWith("@solaris/cli")) {
            errors.push(`${location}: adapters must not import CLI code`);
          }
          if (pkg.name === "@solaris/adapters" && specifier.startsWith(".")) {
            const inProviders = packageRelativeFile.startsWith(join("src", "providers"));
            const inSandbox = packageRelativeFile.startsWith(join("src", "sandbox"));
            const target = resolve(dirname(file), specifier);
            if (inProviders) {
              const toolsRoot = join(pkg.path, "src", "tools");
              if (isUnder(target, toolsRoot)) {
                errors.push(`${location}: providers must not import concrete workspace tools`);
              }
              if (
                isUnder(target, join(pkg.path, "src", "sandbox")) ||
                isUnder(target, join(pkg.path, "src", "environment")) ||
                isUnder(target, join(pkg.path, "src", "checkpoints")) ||
                isUnder(target, join(pkg.path, "src", "git")) ||
                isUnder(target, join(pkg.path, "src", "process"))
              ) {
                errors.push(
                  `${location}: providers must not import sandbox, environment, checkpoint, git, or process adapters`,
                );
              }
              if (isUnder(target, join(pkg.path, "src", "research"))) {
                errors.push(
                  `${location}: providers must not import research adapters; providers never fetch research directly`,
                );
              }
            }
            if (
              packageRelativeFile.startsWith(
                join("src", "tools", "workspace", "mutations") + sep,
              ) &&
              isUnder(target, join(pkg.path, "src", "reference"))
            ) {
              errors.push(
                `${location}: workspace mutation modules must not import reference adapters; reference roots are read-only and never mutation targets`,
              );
            }
            if (inSandbox && isUnder(target, join(pkg.path, "src", "providers"))) {
              errors.push(`${location}: sandbox adapters must not import provider adapters`);
            }
            if (isQualityAdapterModule(packageRelativeFile, file)) {
              for (const forbidden of QUALITY_FORBIDDEN_IMPORT_ROOTS) {
                if (isUnder(target, join(pkg.path, forbidden.root))) {
                  errors.push(
                    `${location}: the quality/reviewer adapter must not import ${forbidden.label}; the reviewer is strictly read-only and cannot mutate, execute, approve, checkpoint, or alter sandbox rules or provider credentials`,
                  );
                }
              }
              // Deterministic gates stay separate from model-based review:
              // the stage runner and validation plumbing consume only core
              // contracts, never a reviewer implementation.
              const targetFile = relative(pkg.path, target);
              const reviewerImplementationFiles = [
                join("src", "godot", "quality", "provider-change-reviewer.ts"),
                join("src", "godot", "quality", "fake-change-reviewer.ts"),
              ];
              const targetsAReviewerImplementation = reviewerImplementationFiles.some(
                (file) => targetFile === file || targetFile === file.replace(/\.ts$/, ".js"),
              );
              const isReviewerImplementationItself = reviewerImplementationFiles.some(
                (file) => packageRelativeFile === file,
              );
              if (targetsAReviewerImplementation && !isReviewerImplementationItself) {
                errors.push(
                  `${location}: deterministic quality gates must not import reviewer implementations; the reviewer is an isolated reasoning signal`,
                );
              }
            }

            if (!isTestSupportFile(file)) {
              const mirrorRoot = join(pkg.path, "src", "godot", "mirror");
              const recoveryRunner = join(
                pkg.path,
                "src",
                "godot",
                "process",
                "godot-recovery-runner.ts",
              );
              const isRecoveryUser = GODOT_RECOVERY_APPROVED_USERS.some((directory) =>
                packageRelativeFile.startsWith(directory + sep),
              );
              const isPackageBarrel = packageRelativeFile === join("src", "index.ts");
              if (!isPackageBarrel) {
                const checkOnlyUser = packageRelativeFile.startsWith(
                  join("src", "godot", "diagnostics") + sep,
                );
                const knowledgeUser = packageRelativeFile.startsWith(
                  join("src", "godot", "knowledge") + sep,
                );
                const checkOnlyRunner = join(
                  pkg.path,
                  "src",
                  "godot",
                  "process",
                  "godot-check-only-runner.ts",
                );
                const knowledgeRunner = join(
                  pkg.path,
                  "src",
                  "godot",
                  "process",
                  "godot-knowledge-runner.ts",
                );
                if (
                  !checkOnlyUser &&
                  (target === checkOnlyRunner || target === checkOnlyRunner.replace(/\.ts$/, ".js"))
                ) {
                  errors.push(
                    `${location}: the Godot check-only runner may only be used by the approved diagnostics adapter`,
                  );
                }
                if (
                  !knowledgeUser &&
                  (target === knowledgeRunner || target === knowledgeRunner.replace(/\.ts$/, ".js"))
                ) {
                  errors.push(
                    `${location}: the Godot API documentation runner may only be used by the approved knowledge adapter`,
                  );
                }
                const lspRunner = join(pkg.path, "src", "godot", "process", "godot-lsp-runner.ts");
                const lspUser = packageRelativeFile.startsWith(join("src", "godot", "lsp") + sep);
                if (
                  !lspUser &&
                  (target === lspRunner || target === lspRunner.replace(/\.ts$/, ".js"))
                ) {
                  errors.push(
                    `${location}: the Godot LSP runner may only be used by the approved LSP adapter`,
                  );
                }
              }
              if (!isRecoveryUser && !isPackageBarrel) {
                if (isUnder(target, mirrorRoot)) {
                  errors.push(
                    `${location}: the disposable project mirror may only be used by the approved Godot probe adapter`,
                  );
                }
                if (
                  target === recoveryRunner ||
                  target === recoveryRunner.replace(/\.ts$/, ".js")
                ) {
                  errors.push(
                    `${location}: the Godot recovery runner may only be used by the approved Godot probe adapter`,
                  );
                }
              }
            }
          }
          if (pkg.name === "@solaris/cli" && specifier.startsWith("@solaris/adapters")) {
            if (!packageRelativeFile.startsWith(join("src", "bootstrap"))) {
              errors.push(`${location}: only the composition root may import concrete adapters`);
            }
          }
        }
      }
    }

    const declaredDependencies = Object.keys(pkg.packageJson.dependencies ?? {});
    if (pkg.name === "@solaris/core") {
      for (const dependency of declaredDependencies) {
        if (dependency.startsWith("@solaris/")) {
          errors.push(`package.json: core must not depend on workspace package ${dependency}`);
        }
      }
    }
    if (pkg.name === "@solaris/adapters" && declaredDependencies.includes("@solaris/cli")) {
      errors.push("package.json: adapters must not depend on @solaris/cli");
    }
  }

  const dependencyGraph = new Map(
    packages.map((pkg) => [
      pkg.name,
      Object.keys(pkg.packageJson.dependencies ?? {}).filter((name) => packagesByName.has(name)),
    ]),
  );
  for (const pkg of packages) {
    const cycle = findCycle(pkg.name, dependencyGraph);
    if (cycle !== null) {
      errors.push(`workspace dependency cycle detected: ${cycle.join(" -> ")}`);
      break;
    }
  }

  return errors;
}

function findCycle(start, graph) {
  const visited = new Set();
  const inStack = new Set();
  const stack = [];
  const visit = (name) => {
    if (inStack.has(name)) {
      const startIndex = stack.indexOf(name);
      return stack.slice(startIndex).concat(name);
    }
    if (visited.has(name)) {
      return null;
    }
    visited.add(name);
    inStack.add(name);
    stack.push(name);
    for (const dependency of graph.get(name) ?? []) {
      const result = visit(dependency);
      if (result !== null) {
        return result;
      }
    }
    stack.pop();
    inStack.delete(name);
    return null;
  };
  return visit(start);
}

/**
 * Limitations of this checker (documented, not claims of an OS boundary):
 * - Specifiers built at runtime (template literals, variables) are not
 *   resolved; canonical spellings of the dangerous modules are still caught.
 * - `require(...)` calls are not analyzed structurally (textual fallbacks
 *   cover the raw-process patterns).
 * - String contents are not semantically analyzed: a repository could
 *   construct Git mutation commands at runtime from parts. Runtime
 *   enforcement (the Git adapter allowlist and the sandbox) is the
 *   security boundary; this checker is a developer guardrail.
 */
function main() {
  const errors = runChecks(join(import.meta.dirname, ".."));
  if (errors.length > 0) {
    console.error("Architecture violations:");
    for (const error of errors) {
      console.error(`  - ${error}`);
    }
    process.exit(1);
  }
  console.log("Architecture check passed.");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
