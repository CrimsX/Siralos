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
});
