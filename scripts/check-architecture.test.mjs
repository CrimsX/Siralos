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
});
