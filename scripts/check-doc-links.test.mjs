import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { collectMarkdownFiles, extractLocalLinks, runCheck } from "./check-doc-links.mjs";

const tempDirectories = [];

function writeFixture(files) {
  const root = mkdtempSync(join(tmpdir(), "siralos-doc-links-"));
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

describe("collectMarkdownFiles", () => {
  it("collects project Markdown and skips generated trees", () => {
    const root = writeFixture({
      "README.md": "# Root\n",
      "docs/guide.md": "# Guide\n",
      "node_modules/pkg/README.md": "# Dependency\n",
      "target/doc/book.md": "# Generated\n",
      "src/code.ts": "export {};\n",
    });

    expect(collectMarkdownFiles(root)).toEqual(["README.md", "docs/guide.md"]);
  });
});

describe("extractLocalLinks", () => {
  it("extracts inline and reference targets but ignores external links and fenced examples", () => {
    const markdown = [
      "[guide](docs/guide.md#start)",
      "[reference][architecture]",
      "[web](https://example.com)",
      "[same](#section)",
      "```md",
      "[example](missing.md)",
      "```",
      "[architecture]: <ARCHITECTURE.md>",
      "",
    ].join("\n");

    expect(extractLocalLinks(markdown)).toEqual([
      { target: "docs/guide.md#start", line: 1 },
      { target: "ARCHITECTURE.md", line: 8 },
    ]);
  });
});

describe("runCheck", () => {
  it("accepts existing relative targets from root and nested documents", () => {
    const root = writeFixture({
      "README.md": "[guide](docs/guide.md)\n",
      "ARCHITECTURE.md": "# Architecture\n",
      "docs/guide.md": "[architecture](../ARCHITECTURE.md#ownership)\n",
    });

    expect(runCheck(root)).toEqual({ ok: true, violations: [] });
  });

  it("reports missing, escaped, absolute, and invalid targets", () => {
    const root = writeFixture({
      "README.md": [
        "[missing](docs/missing.md)",
        "[escape](../outside.md)",
        "[absolute](/etc/passwd)",
        "[encoding](docs/%ZZ.md)",
        "",
      ].join("\n"),
    });

    expect(runCheck(root).violations).toEqual([
      { file: "README.md", line: 1, target: "docs/missing.md", reason: "target not found" },
      { file: "README.md", line: 2, target: "../outside.md", reason: "outside repository" },
      { file: "README.md", line: 3, target: "/etc/passwd", reason: "absolute local path" },
      { file: "README.md", line: 4, target: "docs/%ZZ.md", reason: "invalid URI encoding" },
    ]);
  });

  it("rejects a target whose case only matches through filesystem folding", () => {
    const root = writeFixture({
      "README.md": "[architecture](architecture.md)\n",
      "ARCHITECTURE.md": "# Architecture\n",
    });

    expect(runCheck(root).violations).toEqual([
      { file: "README.md", line: 1, target: "architecture.md", reason: "target not found" },
    ]);
  });
});
