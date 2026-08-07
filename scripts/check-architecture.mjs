#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

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

export function extractImportSpecifiers(source) {
  const specifiers = new Set();
  const patterns = [
    /\bfrom\s*["']([^"']+)["']/g,
    /\bimport\s*["']([^"']+)["']/g,
    /\bimport\s*\(\s*["']([^"']+)["']/g,
  ];
  for (const pattern of patterns) {
    for (const match of source.matchAll(pattern)) {
      specifiers.add(match[1]);
    }
  }
  return [...specifiers];
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

const WRITE_API_TOKENS = ["writeFile", "unlink(", "rename(", "appendFile", "createWriteStream"];

const FORBIDDEN_GIT_WRITE_TOKENS = [
  "git reset",
  "git restore",
  "git checkout",
  "git clean",
  "git stash",
];

const APPROVED_CHILD_PROCESS_DIRECTORIES = [join("src", "sandbox"), join("src", "git", "cli")];

const APPROVED_MUTATION_DIRECTORIES = [
  join("src", "tools", "workspace", "mutations"),
  join("src", "sandbox", "conformance"),
  join("src", "checkpoints", "filesystem"),
];

function isApprovedWriteApiLocation(packageRelativeFile, file) {
  if (isTestSupportFile(file)) {
    return true;
  }
  return APPROVED_MUTATION_DIRECTORIES.some((directory) =>
    packageRelativeFile.startsWith(directory + sep),
  );
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
        if (containsProcessEnvAccess(source, packageRelativeFile, file)) {
          errors.push(
            `${location}: process.env inspection is prohibited in package source; build child environments from an explicit allowlist`,
          );
        }
        if (
          WRITE_API_TOKENS.some((token) => source.includes(token)) &&
          !isApprovedWriteApiLocation(packageRelativeFile, file)
        ) {
          errors.push(
            `${location}: direct file write APIs are prohibited outside approved workspace mutation modules and tests`,
          );
        }
        if (
          !isTestSupportFile(file) &&
          FORBIDDEN_GIT_WRITE_TOKENS.some((token) => source.includes(token))
        ) {
          errors.push(
            `${location}: Git mutation commands (reset, restore, checkout, clean, stash) are prohibited in runtime code`,
          );
        }
        for (const specifier of extractImportSpecifiers(source)) {
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
          if (specifier === "node:child_process") {
            const inApprovedDirectory = APPROVED_CHILD_PROCESS_DIRECTORIES.some((directory) =>
              packageRelativeFile.startsWith(directory + sep),
            );
            const isTestFile = file.endsWith(".test.ts");
            if (!inApprovedDirectory && !isTestFile) {
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
                isUnder(target, join(pkg.path, "src", "git"))
              ) {
                errors.push(
                  `${location}: providers must not import sandbox, environment, checkpoint, or git adapters`,
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
