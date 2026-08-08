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
    file.endsWith("git-test-support.ts")
  );
}

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
  // the engine-profile cache performs verified atomic metadata writes
  // beneath ~/.solaris/godot/engine-profiles
  join("src", "godot", "cache"),
  // the probe executable-copy staging writes only the verified private
  // executable copy inside the Solaris-created run directory
  join("src", "godot", "process", "executable-copy.ts"),
  // the bounded no-follow removal and bounded file reads are Solaris-owned
  // filesystem primitives operating only on verified Solaris-created roots
  join("src", "fs"),
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

  const addCall = (module, api, calleeText) => {
    calls.push({ module, api, calleeText });
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
          addCall(binding.module, binding.originalName, node.expression.text);
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
          addCall(namespace, node.expression.name.text, calleeText);
          if (namespace === CHILD_PROCESS_MODULE) {
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
  return { imports, calls, spawnCalls, destructiveFsImports, importedNames };
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
          for (const imported of analysis.destructiveFsImports) {
            if (!isApprovedWriteApiLocation(packageRelativeFile, file)) {
              errors.push(
                `${location}: direct file write APIs are prohibited: ${imported.api} imported from ${imported.module} outside approved workspace mutation modules and tests`,
              );
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
            }
            if (inSandbox && isUnder(target, join(pkg.path, "src", "providers"))) {
              errors.push(`${location}: sandbox adapters must not import provider adapters`);
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
