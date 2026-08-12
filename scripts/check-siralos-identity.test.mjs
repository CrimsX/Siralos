import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  collectProjectFiles,
  findOldIdentityViolations,
  runCheck,
} from "./check-siralos-identity.mjs";

const tempDirectories = [];

function writeFixture(files) {
  const root = mkdtempSync(join(tmpdir(), "siralos-identity-"));
  tempDirectories.push(root);
  for (const [path, content] of Object.entries(files)) {
    const fullPath = join(root, path);
    mkdirSync(dirname(fullPath), { recursive: true });
    writeFileSync(fullPath, content);
  }
  return root;
}

afterEach(() => {
  for (const directory of tempDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe("collectProjectFiles", () => {
  it("returns relative paths and skips ignored directories", () => {
    const root = writeFixture({
      "src/main.ts": "export const ok = true;\n",
      "node_modules/dependency/index.js": "old solaris identity\n",
      "packages/core/dist/index.js": "old solaris identity\n",
      "target/debug/build.rs": "old solaris identity\n",
      "docs/note.md": "fine\n",
      "dist/bundle.js": "old solaris identity\n",
    });
    expect(collectProjectFiles(root)).toEqual(["docs/note.md", "src/main.ts"]);
  });

  it("skips tsbuildinfo artifacts", () => {
    const root = writeFixture({
      "tests/tsconfig.tsbuildinfo": "old solaris identity\n",
      "src/main.ts": "fine\n",
    });
    expect(collectProjectFiles(root)).toEqual(["src/main.ts"]);
  });
});

describe("findOldIdentityViolations", () => {
  it("flags every casing and derived form of the old identity", () => {
    const root = writeFixture({
      "src/names.ts": [
        'const product = "Solaris";',
        'const upper = "SOLARIS";',
        'const lower = "solaris";',
        'import { x } from "@solaris/core";',
        'const dir = ".solaris";',
        'const env = "SOLARIS_CONFIG";',
        'const binary = "solaris-cli";',
        'const home = "~/.solaris";',
        'const clean = "Siralos";',
        'const canonical = "siralos";',
        "",
      ].join("\n"),
    });
    const violations = findOldIdentityViolations(root, collectProjectFiles(root));
    expect(violations.map((v) => v.line)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it("respects the documented exclusion for the verification mechanism itself", () => {
    const root = writeFixture({
      "scripts/check-siralos-identity.mjs": "const OLD_IDENTITY_PATTERN = /solaris/i;\n",
      "scripts/check-siralos-identity.test.mjs": 'expect("Solaris").toMatch(/solaris/i);\n',
    });
    const violations = findOldIdentityViolations(root, collectProjectFiles(root));
    expect(violations).toEqual([]);
  });

  it("reports file, line, and text for each violation", () => {
    const root = writeFixture({
      "docs/guide.md": "line one\nSolaris is the old name\nline three\n",
    });
    const violations = findOldIdentityViolations(root, collectProjectFiles(root));
    expect(violations).toHaveLength(1);
    expect(violations[0]).toMatchObject({
      file: "docs/guide.md",
      line: 2,
      text: "Solaris is the old name",
    });
  });
});

describe("runCheck", () => {
  it("passes when no project-owned file uses the old identity", () => {
    const root = writeFixture({
      "README.md": "# Siralos\n",
      "src/main.ts": 'const canonical = "Siralos";\n',
    });
    const result = runCheck(root);
    expect(result.ok).toBe(true);
    expect(result.violations).toEqual([]);
  });

  it("fails when a project-owned file uses the old identity", () => {
    const root = writeFixture({
      "src/main.ts": 'const old = "Solaris";\n',
    });
    const result = runCheck(root);
    expect(result.ok).toBe(false);
    expect(result.violations).toHaveLength(1);
  });
});
