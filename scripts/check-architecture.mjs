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
  const patterns = [/\bfrom\s*["']([^"']+)["']/g, /\bimport\s*["']([^"']+)["']/g];
  for (const pattern of patterns) {
    for (const match of source.matchAll(pattern)) {
      specifiers.add(match[1]);
    }
  }
  return [...specifiers];
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
        for (const specifier of extractImportSpecifiers(source)) {
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
            const packageRelativeFile = relative(pkg.path, file);
            const inProviders = packageRelativeFile.startsWith(join("src", "providers"));
            if (inProviders) {
              const target = resolve(dirname(file), specifier);
              const toolsRoot = join(pkg.path, "src", "tools");
              if (target === toolsRoot || target.startsWith(toolsRoot + sep)) {
                errors.push(`${location}: providers must not import concrete workspace tools`);
              }
            }
          }
          if (pkg.name === "@solaris/cli" && specifier.startsWith("@solaris/adapters")) {
            const packageRelative = relative(pkg.path, file);
            if (!packageRelative.startsWith(join("src", "bootstrap"))) {
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
