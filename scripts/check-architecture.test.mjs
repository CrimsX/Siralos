import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { runChecks } from "./check-architecture.mjs";

const tempDirectories = [];

function writeFixture(files) {
  const root = mkdtempSync(join(tmpdir(), "solaris-architecture-"));
  tempDirectories.push(root);
  for (const [path, content] of Object.entries(files)) {
    const fullPath = join(root, path);
    mkdirSync(dirname(fullPath), { recursive: true });
    writeFileSync(fullPath, content);
  }
  return root;
}

function packageJson(name, dependencies = {}) {
  return JSON.stringify({ name, private: true, type: "module", dependencies }, null, 2);
}

function cleanWorkspaceFixture() {
  return {
    "package.json": JSON.stringify(
      { name: "fixture", private: true, workspaces: ["apps/*", "packages/*"] },
      null,
      2,
    ),
    "packages/core/package.json": packageJson("@solaris/core"),
    "packages/core/src/index.ts": "export {};\n",
    "packages/adapters/package.json": packageJson("@solaris/adapters", {
      "@solaris/core": "0.0.0",
    }),
    "packages/adapters/src/index.ts": "export {};\n",
    "apps/cli/package.json": packageJson("@solaris/cli", {
      "@solaris/adapters": "0.0.0",
      "@solaris/core": "0.0.0",
    }),
    "apps/cli/src/index.ts": "export {};\n",
    "apps/cli/src/bootstrap/create-application.ts": 'import "@solaris/adapters";\n',
  };
}

describe("check-architecture", () => {
  afterEach(() => {
    for (const dir of tempDirectories.splice(0)) {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("accepts a clean workspace", () => {
    const errors = runChecks(writeFixture(cleanWorkspaceFixture()));
    expect(errors).toEqual([]);
  });

  it("rejects core importing an adapter package", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/core/src/index.ts"] = 'import "@solaris/adapters";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("core must not import workspace package"))).toBe(
      true,
    );
  });

  it("rejects core importing Node infrastructure", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/core/src/index.ts"] = 'import { readFileSync } from "node:fs";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("core must not import Node module"))).toBe(true);
  });

  it("rejects adapters importing CLI code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/index.ts"] = 'import "@solaris/cli";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("adapters must not import CLI code"))).toBe(true);
  });

  it("rejects CLI code outside the composition root importing adapters", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["apps/cli/src/repl.ts"] = 'import "@solaris/adapters";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) =>
        error.includes("only the composition root may import concrete adapters"),
      ),
    ).toBe(true);
  });

  it("rejects core declaring a workspace dependency", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/core/package.json"] = packageJson("@solaris/core", {
      "@solaris/adapters": "0.0.0",
    });
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("core must not depend on workspace package")),
    ).toBe(true);
  });

  it("rejects the fake provider importing concrete workspace tools", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/providers/fake.ts"] =
      'import x from "../tools/workspace/list.js";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("providers must not import concrete workspace tools")),
    ).toBe(true);
  });

  it("accepts the fake provider importing core contracts only", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/providers/fake.ts"] =
      'import type { ModelProvider } from "@solaris/core";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors).toEqual([]);
  });

  it("rejects core importing a Node filesystem module", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/core/src/index.ts"] = 'import { readFileSync } from "node:fs";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("core must not import Node module"))).toBe(true);
  });

  it("rejects core importing a child-process module", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/core/src/index.ts"] = 'import { spawn } from "node:child_process";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("core must not import Node module"))).toBe(true);
  });

  it("rejects Sandbox Runtime imports outside the anthropic runtime adapter", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/sandbox/other.ts"] =
      'import { SandboxManager } from "@anthropic-ai/sandbox-runtime";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) =>
        error.includes("may only be imported by the anthropic runtime adapter"),
      ),
    ).toBe(true);
  });

  it("accepts Sandbox Runtime imports inside the anthropic runtime adapter", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/sandbox/anthropic-runtime/backend.ts"] =
      'import { SandboxManager } from "@anthropic-ai/sandbox-runtime";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors).toEqual([]);
  });

  it("rejects unsandboxed process spawning outside approved modules", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["apps/cli/src/repl.ts"] = 'import { spawn } from "node:child_process";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("unsandboxed process spawning is prohibited")),
    ).toBe(true);
  });

  it("accepts child-process imports in sandbox modules and test files", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/sandbox/runner.ts"] =
      'import { spawn } from "node:child_process";\n';
    fixture["packages/adapters/src/core.test.ts"] = 'import { spawn } from "node:child_process";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors).toEqual([]);
  });

  it("rejects provider adapters importing sandbox adapters", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/providers/fake.ts"] = 'import x from "../sandbox/backend.js";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("providers must not import sandbox"))).toBe(true);
  });

  it("rejects sandbox adapters importing provider adapters", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/sandbox/backend.ts"] = 'import x from "../providers/fake.js";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("sandbox adapters must not import provider")),
    ).toBe(true);
  });

  it("rejects process.env inspection in package source", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/core/src/index.ts"] = "const env = process.env;\n";
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("process.env inspection is prohibited"))).toBe(
      true,
    );
  });

  it("rejects direct file writes in core", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/core/src/index.ts"] =
      'import { writeFile } from "node:fs/promises";\nwriteFile("x", "y");\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("direct file write APIs are prohibited"))).toBe(
      true,
    );
  });

  it("rejects direct file writes in providers", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/providers/fake.ts"] =
      'import { unlink } from "node:fs/promises";\nunlink("x");\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("direct file write APIs are prohibited"))).toBe(
      true,
    );
  });

  it("rejects direct file writes in the CLI", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["apps/cli/src/repl.ts"] =
      'import { rename } from "node:fs/promises";\nrename("a", "b");\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("direct file write APIs are prohibited"))).toBe(
      true,
    );
  });

  it("accepts direct file writes in mutation modules and tests", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/workspace/mutations/editor.ts"] =
      'import { writeFile } from "node:fs/promises";\nwriteFile("x", "y");\n';
    fixture["packages/adapters/src/tools/workspace/mutations/editor.test.ts"] =
      'import { writeFile } from "node:fs/promises";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors).toEqual([]);
  });

  it("rejects provider adapters importing CLI approval code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/providers/fake.ts"] =
      'import { createInteractiveApprovalReviewer } from "@solaris/cli";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("adapters must not import CLI code"))).toBe(true);
  });

  it("detects workspace dependency cycles", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/core/package.json"] = packageJson("@solaris/core", {
      "@solaris/adapters": "0.0.0",
    });
    fixture["packages/adapters/package.json"] = packageJson("@solaris/adapters", {
      "@solaris/core": "0.0.0",
    });
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("workspace dependency cycle detected"))).toBe(
      true,
    );
  });

  it("rejects forbidden Git mutation commands in runtime code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'export const command = "git reset --hard";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("Git mutation commands"))).toBe(true);
  });

  it("accepts Git mutation strings in test files", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.test.ts"] =
      'export const command = "git reset --hard";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("Git mutation commands"))).toBe(false);
  });

  it("rejects child-process spawning outside sandbox and git modules", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import { spawn } from "node:child_process";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("unsandboxed process spawning"))).toBe(true);
  });

  it("rejects child-process imports in the git adapter (Git must run inside the sandbox)", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/git/cli/runner.ts"] =
      'import { spawn } from "node:child_process";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("unsandboxed process spawning"))).toBe(true);
  });

  it("rejects provider adapters importing checkpoint or git adapters", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/providers/fake.ts"] =
      'import { loadPreimage } from "../checkpoints/filesystem/checkpoint-store.js";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("checkpoint, git, or process adapters"))).toBe(
      true,
    );
  });

  it("rejects direct file writes outside approved modules", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/providers/fake.ts"] =
      'import { writeFile } from "node:fs/promises";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("direct file write APIs"))).toBe(true);
  });

  it("accepts direct file writes in the process adapter", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/process/run-directories.ts"] =
      'import { writeFile } from "node:fs/promises";\n' + "export {};\n";
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("direct file write APIs"))).toBe(false);
  });

  it("rejects shell: true in runtime code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] = "const options = { shell: true };\n";
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("raw process execution"))).toBe(true);
  });

  it("rejects execSync and spawnSync in runtime code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'execSync("evil");\nspawnSync("git", []);\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("raw process execution"))).toBe(true);
  });

  it("rejects child_process exec in runtime code but allows regex .exec", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'exec("evil");\nconst match = /x/.exec("x");\n';
    const errors = runChecks(writeFixture(fixture));
    const rawErrors = errors.filter((error) => error.includes("raw process execution"));
    expect(rawErrors).toHaveLength(1);
  });

  it("rejects process.run command runners that spawn processes", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/process/runners/npm-script-runner.ts"] =
      'import { spawn } from "node:child_process";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("unsandboxed process spawning"))).toBe(true);
  });

  it("accepts prohibited patterns inside documented conformance fixture sources", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/sandbox/conformance/run-conformance.ts"] =
      'const fixture = `spawnSync("x", []); exec("y"); shell: true`;\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("raw process execution"))).toBe(false);
  });

  it("accepts prohibited patterns in test support files", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/workspace/workspace-fixtures.ts"] =
      'spawnSync("git", []);\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("raw process execution"))).toBe(false);
  });

  it("rejects providers importing process adapters", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/providers/fake.ts"] =
      'import { createProcessRunTool } from "../process/process-run-tool.js";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("process adapters"))).toBe(true);
  });

  it("rejects child_process imported without the node: prefix", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] = 'import { spawn } from "child_process";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("unsandboxed process spawning"))).toBe(true);
  });

  it("rejects aliased dangerous child_process imports", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import { exec as evilExec } from "node:child_process";\nevilExec("evil");\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("unsandboxed process spawning"))).toBe(true);
  });

  it("rejects re-exports of dangerous modules", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'export { spawn } from "node:child_process";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("unsandboxed process spawning"))).toBe(true);
  });

  it("rejects static dynamic imports of dangerous modules", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'const cp = await import("node:child_process");\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("unsandboxed process spawning"))).toBe(true);
  });

  it("rejects aliased destructive filesystem imports with calls", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import { rename as evilRename } from "node:fs/promises";\nevilRename("a", "b");\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("direct file write APIs"))).toBe(true);
  });

  it("rejects namespace filesystem imports that call destructive APIs", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import * as fsp from "node:fs/promises";\nfsp.writeFile("a", "b");\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("direct file write APIs"))).toBe(true);
  });

  it("rejects direct rm with recursive: true in production code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import { rm } from "node:fs/promises";\nrm("dir", { recursive: true });\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(true);
  });

  it("rejects aliased rm with recursive: true in production code", () => {
    // The verified alias reproduction: the import binding resolves
    // `erase` back to `rm`, so this must be an architecture error.
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import { rm as erase } from "node:fs/promises";\nerase("dir", { recursive: true, force: true });\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(true);
  });

  it("rejects namespace rm with recursive: true in production code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import * as fsp from "node:fs/promises";\nfsp.rm("dir", { recursive: true });\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(true);
  });

  it("rejects rmSync with recursive: true in production code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import { rmSync } from "fs";\nrmSync("dir", { recursive: true });\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(true);
  });

  it("rejects bare and node:-prefixed module specifiers alike", () => {
    for (const specifier of ["node:fs/promises", "fs/promises", "node:fs", "fs"]) {
      const fixture = cleanWorkspaceFixture();
      fixture["packages/adapters/src/tools/example.ts"] =
        `import { rm } from "${specifier}";\nrm("dir", { recursive: true });\n`;
      const errors = runChecks(writeFixture(fixture));
      expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(true);
    }
  });

  it("rejects recursive rm with multiline and reordered option properties", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import { rm } from "node:fs/promises";\nrm("dir", {\n  force: true,\n  recursive: true,\n  maxRetries: 3,\n});\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(true);
  });

  it("rejects recursive rm inside an otherwise approved mutation directory", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/workspace/mutations/editor.ts"] =
      'import { rm } from "node:fs/promises";\nrm("dir", { recursive: true });\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(true);
  });

  it("rejects non-recursive rm outside approved destructive-operation locations", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import { rm } from "node:fs/promises";\nrm("file.txt", { force: true });\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("direct file write APIs"))).toBe(true);
    expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(false);
  });

  it("allows non-recursive rm only in explicitly approved locations", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/workspace/mutations/editor.ts"] =
      'import { rm } from "node:fs/promises";\nrm("temp.tmp", { force: true });\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("direct file write APIs"))).toBe(false);
    expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(false);
  });

  it("allows recursive cleanup only in the documented exact exemptions", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/sandbox/conformance/run-conformance.ts"] =
      'import { rm } from "node:fs/promises";\nrm("artifacts", { recursive: true, force: true });\n';
    fixture["packages/adapters/src/tools/example.test.ts"] =
      'import { rm } from "node:fs/promises";\nrm("dir", { recursive: true });\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(false);
  });

  it("does not exempt a whole conformance directory from the recursive rule", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/sandbox/conformance/probe-embed.ts"] =
      'import { rm } from "node:fs/promises";\nrm("dir", { recursive: true });\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("path-based recursive deletion"))).toBe(true);
  });

  it("produces the same diagnostic category for direct, alias, and namespace forms", () => {
    const sources = [
      'import { rm } from "node:fs/promises";\nrm("dir", { recursive: true });\n',
      'import { rm as erase } from "node:fs/promises";\nerase("dir", { recursive: true });\n',
      'import * as fsp from "node:fs/promises";\nfsp.rm("dir", { recursive: true });\n',
    ];
    for (const source of sources) {
      const fixture = cleanWorkspaceFixture();
      fixture["packages/adapters/src/tools/example.ts"] = source;
      const errors = runChecks(writeFixture(fixture));
      const recursiveErrors = errors.filter((error) =>
        error.includes("path-based recursive deletion"),
      );
      expect(recursiveErrors.length).toBeGreaterThan(0);
      for (const error of recursiveErrors) {
        expect(error).toContain("path-based recursive deletion is prohibited");
      }
    }
  });

  it("rejects Git mutation commands passed to spawn structurally", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/git/cli/runner.ts"] =
      'import { spawn } from "node:child_process";\nspawn("git", ["reset", "--hard"]);\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("Git mutation commands"))).toBe(true);
  });

  it("accepts non-destructive filesystem imports in tools", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/tools/example.ts"] =
      'import { realpath, stat } from "node:fs/promises";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("direct file write APIs"))).toBe(false);
  });

  it("rejects project-affecting Godot arguments in probe invocation code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'export const ARGS = ["--import"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("project-affecting Godot arguments"))).toBe(true);
  });

  it("rejects --editor and --recovery-mode in probe invocation code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'export const ARGS = ["--editor"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("project-affecting Godot arguments"))).toBe(true);
    const second = cleanWorkspaceFixture();
    second["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'export const ARGS = ["--recovery-mode"];\n';
    const secondErrors = runChecks(writeFixture(second));
    expect(secondErrors.some((error) => error.includes("project-affecting Godot arguments"))).toBe(
      true,
    );
  });

  it("accepts fixed probe arguments in probe invocation code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'export const ARGS = ["--version"];\nexport const HELP = ["--help"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("project-affecting Godot arguments"))).toBe(false);
  });

  it("allows capability parsing modules to reference project option names", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/help-capabilities-parser.ts"] =
      'export const KNOWN = ["--path", "--scene", "--script", "--import"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("project-affecting Godot arguments"))).toBe(false);
  });

  it("allows tests to reference project-affecting Godot arguments", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.test.ts"] =
      'expect(ARGS).not.toContain("--path");\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("project-affecting Godot arguments"))).toBe(false);
  });

  it("rejects a non-fixed Godot probe argument tuple reaching sandbox execution", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'export const ARGS = ["--version", "--import"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("non-fixed Godot probe argument"))).toBe(true);
  });

  it("rejects Godot probe arguments constructed by string concatenation", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'export const ARGS = ["--" + "version"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("must not be constructed by string concatenation")),
    ).toBe(true);
  });

  it("rejects Godot probe argument arrays imported from a moved module", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'import { PROBE_ARGS } from "../probe/constants.js";\nexport const ARGS = PROBE_ARGS;\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("must not be imported"))).toBe(true);
  });

  it("rejects Godot probe argument construction outside the fixed runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/other-probe-module.ts"] =
      'export const ARGS = ["--version"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("allowed only inside the fixedProbeArguments")),
    ).toBe(true);
  });

  it("requires the fixedProbeArguments constructor in the probe runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'export const ARGS = ["--version"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("must construct every probe argument tuple")),
    ).toBe(true);
  });

  it("accepts the fail-closed probe runner that never constructs tuples", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'export function createGodotProbeRunner() {\n  return {\n    isAvailable() { return Promise.resolve(false); },\n    probeVersion() { return Promise.resolve({ status: "unavailable", message: "x" }); },\n  };\n}\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("must construct every probe argument tuple")),
    ).toBe(false);
    expect(errors.some((error) => error.includes("non-fixed Godot probe argument"))).toBe(false);
  });

  it("accepts the fixed probe tuple constructor in the probe runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'function fixedProbeArguments(kind) {\n  switch (kind) {\n    case "version":\n      return ["--version"];\n    case "help":\n      return ["--help"];\n    case "api-dump":\n      return ["--dump-extension-api"];\n  }\n}\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("non-fixed Godot probe argument"))).toBe(false);
    expect(
      errors.some((error) => error.includes("must construct every probe argument tuple")),
    ).toBe(false);
    expect(errors.some((error) => error.includes("string concatenation"))).toBe(false);
  });

  it("allows --path only in the recovery runner and requires the recovery pairing", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-recovery-runner.ts"] =
      'export const BASE = ["--headless", "--editor", "--recovery-mode"];\nexport function args(mirrorPath) {\n  return [...BASE, "--path", mirrorPath, "--quit-after", "120"];\n}\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("project-affecting Godot arguments"))).toBe(false);
    expect(errors.some((error) => error.includes("must pair the project path"))).toBe(false);
    expect(errors.some((error) => error.includes("must pass --path"))).toBe(false);
    expect(errors.some((error) => error.includes("literal path"))).toBe(false);
    expect(errors.some((error) => error.includes("source workspace root"))).toBe(false);
  });

  it("rejects a literal project path in the recovery runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-recovery-runner.ts"] =
      'export const ARGS = ["--headless", "--editor", "--recovery-mode", "--path", "/abs/path", "--quit-after", "120"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("literal path"))).toBe(true);
  });

  it("rejects the source workspace as the recovery project path", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-recovery-runner.ts"] =
      'export function args(workspaceRoot) {\n  return ["--headless", "--editor", "--recovery-mode", "--path", workspaceRoot, "--quit-after", "120"];\n}\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("source workspace root"))).toBe(true);
  });

  it("rejects missing recovery-mode pairing in the recovery runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-recovery-runner.ts"] =
      'export const ARGS = ["--headless", "--editor", "--path", mirrorPath, "--quit-after", "120"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("must pair the project path with --recovery-mode")),
    ).toBe(true);
  });

  it("rejects script, scene, import, export, and debug options in the recovery runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-recovery-runner.ts"] =
      'export const ARGS = ["--headless", "--editor", "--recovery-mode", "--path", mirrorPath, "--scene", "main.tscn", "--quit-after", "120"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("recovery runner must not pass"))).toBe(true);
  });

  it("catches a forbidden option built by string concatenation", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-recovery-runner.ts"] =
      'const scene = "--" + "scene";\nexport const ARGS = ["--headless", "--editor", "--recovery-mode", "--path", mirrorPath, scene, "--quit-after", "120"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("recovery runner must not pass --scene"))).toBe(
      true,
    );
    expect(
      errors.some((error) => error.includes("must not be constructed by string concatenation")),
    ).toBe(true);
  });

  it("rejects argument arrays imported from another module", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-recovery-runner.ts"] =
      'import { RECOVERY_ARGS } from "./args.js";\nexport function args(mirrorPath) {\n  return [...RECOVERY_ARGS, "--path", mirrorPath, "--quit-after", "120"];\n}\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("must not be imported (RECOVERY_ARGS"))).toBe(
      true,
    );
  });

  it("rejects composing the tuple from an imported constant via spread", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-recovery-runner.ts"] =
      'import { fixedHeadless } from "./args.js";\nexport function args(mirrorPath) {\n  return [...fixedHeadless, "--path", mirrorPath, "--quit-after", "120"];\n}\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("must not be composed from imported constants")),
    ).toBe(true);
  });

  it("accepts a computed (non-literal) mirror path value", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-recovery-runner.ts"] =
      'function mirrorOf(request) {\n  return request.mirrorProjectPath;\n}\nexport const ARGS = ["--headless", "--editor", "--recovery-mode", "--path", mirrorOf(request), "--quit-after", "120"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("literal path"))).toBe(false);
    expect(errors.some((error) => error.includes("source workspace root"))).toBe(false);
  });

  it("keeps --path prohibited in the fixed probe runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-probe-runner.ts"] =
      'export const ARGS = ["--headless", "--path", "/abs/path"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("project-affecting Godot arguments"))).toBe(true);
  });

  it("restricts the disposable mirror to the approved probe adapter", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/tools/example-tool.ts"] =
      'import { createProjectMirror } from "../mirror/project-mirror.js";\nexport const m = createProjectMirror();\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("disposable project mirror may only be used")),
    ).toBe(true);
  });

  it("allows the probe service to use the disposable mirror", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/probe/service.ts"] =
      'import { createProjectMirror } from "../mirror/project-mirror.js";\nexport const m = createProjectMirror();\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("disposable project mirror may only be used")),
    ).toBe(false);
  });

  it("restricts the recovery runner to the approved probe adapter", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/tools/example-tool.ts"] =
      'import { createGodotRecoveryRunner } from "../process/godot-recovery-runner.js";\nexport const r = createGodotRecoveryRunner();\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("recovery runner may only be used"))).toBe(true);
  });

  it("catches an aliased import of the recovery runner from an unapproved module", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/tools/example-tool.ts"] =
      'import { createGodotRecoveryRunner as evilRunner } from "../process/godot-recovery-runner.js";\nexport const r = evilRunner();\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("recovery runner may only be used"))).toBe(true);
  });
});

describe("check-architecture Godot check-only diagnostics", () => {
  it("requires the fixed check-only pairing in the check-only runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-check-only-runner.ts"] =
      'export const BASE = ["--headless", "--path", mirrorPath, "--script", mirrorScript, "--check-only"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("must pass --headless"))).toBe(false);
    expect(errors.some((error) => error.includes("must pass --path"))).toBe(false);
    expect(errors.some((error) => error.includes("must pass --script"))).toBe(false);
    expect(errors.some((error) => error.includes("must pass --check-only"))).toBe(false);
  });

  it("rejects missing --check-only in the check-only runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-check-only-runner.ts"] =
      'export const ARGS = ["--headless", "--path", mirrorPath, "--script", mirrorScript];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("must pass --check-only"))).toBe(true);
  });

  it("rejects scene, editor, import, LSP, DAP, and recovery options in the check-only runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-check-only-runner.ts"] =
      'export const ARGS = ["--headless", "--path", mirrorPath, "--script", mirrorScript, "--check-only", "--scene"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("must not pass --scene"))).toBe(true);
  });

  it("rejects literal mirror paths and the source workspace in the check-only runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-check-only-runner.ts"] =
      'export const ARGS = ["--headless", "--path", "/abs/mirror", "--script", "/abs/mirror/a.gd", "--check-only"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("never from literal paths"))).toBe(true);
  });

  it("rejects the source workspace root as the check-only --path", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-check-only-runner.ts"] =
      'export function args(workspaceRoot, scriptPath) {\n  return ["--headless", "--path", workspaceRoot, "--script", scriptPath, "--check-only"];\n}\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("never be the source workspace"))).toBe(true);
  });

  it("rejects concatenated check-only argument construction", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-check-only-runner.ts"] =
      'export const ARGS = ["--headless", "--path", mirrorPath, "--scr" + "ipt", mirrorScript, "--check-only"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("string concatenation"))).toBe(true);
  });

  it("allows the API documentation runner only with the exact with-docs tuple", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-knowledge-runner.ts"] =
      'export const BASE = ["--dump-extension-api-with-docs"];\nexport function args() {\n  return [...BASE];\n}\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("must pass exactly --dump-extension-api-with-docs")),
    ).toBe(false);
  });

  it("rejects extra options in the API documentation runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-knowledge-runner.ts"] =
      'export const ARGS = ["--dump-extension-api-with-docs", "--path", "/x"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("must pass exactly --dump-extension-api-with-docs")),
    ).toBe(true);
  });

  it("rejects an ordinary --dump-extension-api substitution in the API documentation runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-knowledge-runner.ts"] =
      'export const ARGS = ["--dump-extension-api"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("must pass exactly --dump-extension-api-with-docs")),
    ).toBe(true);
  });

  it("restricts the check-only runner to the approved diagnostics adapter", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-check-only-runner.ts"] =
      'export const ARGS = ["--headless", "--path", mirrorPath, "--script", mirrorScript, "--check-only"];\n';
    fixture["packages/adapters/src/godot/process/evil.ts"] =
      'import { createGodotCheckOnlyRunner } from "./godot-check-only-runner.js";\nexport const x = createGodotCheckOnlyRunner;\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("check-only runner may only be used"))).toBe(true);
  });
});

describe("check-architecture Godot LSP boundaries", () => {
  it("requires the fixed recovery LSP pairing in the LSP runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-lsp-runner.ts"] =
      'export const BASE = ["--headless", "--editor", "--recovery-mode", "--path", mirrorPath, "--lsp-port", String(port)];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("must pass --lsp-port"))).toBe(false);
    expect(errors.some((error) => error.includes("must pass --recovery-mode"))).toBe(false);
    expect(errors.some((error) => error.includes("must pass --path"))).toBe(false);
  });

  it("rejects missing --recovery-mode in the LSP runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-lsp-runner.ts"] =
      'export const ARGS = ["--headless", "--editor", "--path", mirrorPath, "--lsp-port", "6005"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("must pass --recovery-mode"))).toBe(true);
  });

  it("rejects DAP, scene, script, and import options in the LSP runner", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-lsp-runner.ts"] =
      'export const ARGS = ["--headless", "--editor", "--recovery-mode", "--path", mirrorPath, "--lsp-port", "6005", "--dap-port", "6006"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("must not pass --dap-port"))).toBe(true);
  });

  it("rejects literal LSP port and mirror path values", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/godot-lsp-runner.ts"] =
      'export const ARGS = ["--headless", "--editor", "--recovery-mode", "--path", "/abs/mirror", "--lsp-port", "6005"];\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("never from literal values"))).toBe(true);
  });

  it("restricts node:net to the approved LSP adapter", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/process/evil.ts"] =
      'import { createServer } from "node:net";\nexport const x = createServer;\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("allowed only inside the approved Godot LSP adapter")),
    ).toBe(true);
  });

  it("forbids LSP mutation method references in runtime adapter code", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/lsp/evil.ts"] =
      'export const method = "workspace/applyEdit";\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) => error.includes("LSP mutation methods must never be implemented")),
    ).toBe(true);
  });
});

describe("check-architecture development workflow boundaries", () => {
  it("accepts a clean development workflow orchestrator", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/development/gdscript-development-service.ts"] =
      'import type { GDScriptDevelopmentService } from "@solaris/core";\nexport const create = (): GDScriptDevelopmentService => ({}) as never;\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors).toEqual([]);
  });

  it("rejects filesystem imports in the workflow orchestrator", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/development/gdscript-development-service.ts"] =
      'import { readFileSync } from "node:fs";\nexport const x = readFileSync;\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) =>
        error.includes("GDScript development workflow orchestrator must not import node:fs"),
      ),
    ).toBe(true);
  });

  it("rejects path imports in the change-set executor", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/development/change-set-executor.ts"] =
      'import { join } from "node:path";\nexport const x = join;\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) =>
        error.includes("GDScript development workflow orchestrator must not import node:path"),
      ),
    ).toBe(true);
  });

  it("rejects socket imports in the workflow orchestrator", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/development/gdscript-development-service.ts"] =
      'import { createServer } from "node:net";\nexport const x = createServer;\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) =>
        error.includes("GDScript development workflow orchestrator must not import node:net"),
      ),
    ).toBe(true);
  });

  it("rejects core importing the development workflow Node-free contract violation", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/core/src/godot/development/development-model.ts"] =
      'import { readFileSync } from "node:fs";\nexport const x = readFileSync;\n';
    const errors = runChecks(writeFixture(fixture));
    expect(errors.some((error) => error.includes("core must not import Node module"))).toBe(true);
  });

  it("still rejects raw process execution in the workflow orchestrator", () => {
    const fixture = cleanWorkspaceFixture();
    fixture["packages/adapters/src/godot/development/gdscript-development-service.ts"] =
      'import { spawnSync } from "node:child_process";\nexport const x = spawnSync;\n';
    const errors = runChecks(writeFixture(fixture));
    expect(
      errors.some((error) =>
        error.includes(
          "GDScript development workflow orchestrator must not import node:child_process",
        ),
      ),
    ).toBe(true);
  });
});
