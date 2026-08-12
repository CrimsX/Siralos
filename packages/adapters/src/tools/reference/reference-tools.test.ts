import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import type { JsonObject, ToolExecutionResult } from "@siralos/core";
import { createReferenceServices } from "../../reference/reference-services.js";
import {
  createTempWorkspace,
  writeFixtureFiles,
  type TempWorkspace,
} from "../../tools/workspace/workspace-fixtures.js";

const roots: TempWorkspace[] = [];

async function withRoot(): Promise<TempWorkspace> {
  const root = await createTempWorkspace();
  roots.push(root);
  return root;
}

afterEach(async () => {
  for (const root of roots.splice(0)) {
    await root.cleanup();
  }
});

const GD_SOURCE = `extends Node

func move(delta: float) -> void:
	position += delta
`;

async function buildServices(fixtureRoot: string, workspaceRoot: string) {
  const services = await createReferenceServices({
    declarations: [
      {
        alias: "docs",
        kind: "local-directory",
        source: { kind: "local-directory", path: fixtureRoot },
        description: "test reference",
      },
    ],
    workspaceRoot,
  });
  return services;
}

async function buildFixtureTree(): Promise<TempWorkspace> {
  const fixture = await withRoot();
  await writeFixtureFiles(fixture.root, {
    "a.txt": "hello world\n",
    "sub/b.gd": GD_SOURCE,
    "sub/notes.md": "hello from notes\n",
  });
  return fixture;
}

function toolByName(services: Awaited<ReturnType<typeof buildServices>>, name: string) {
  const tool = services.tools.find((candidate) => candidate.definition.name === name);
  if (tool === undefined) {
    throw new Error(`Tool ${name} not found.`);
  }
  return tool;
}

function successOutput(result: ToolExecutionResult): JsonObject {
  expect(result.status).toBe("success");
  if (result.status !== "success") {
    throw new Error(`Expected success, got ${result.status}.`);
  }
  return result.output as JsonObject;
}

describe("reference tools", () => {
  it("declares the reference.inspect capability and read-only descriptions", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    expect(services.tools.map((tool) => tool.definition.name)).toEqual([
      "reference.list",
      "reference.read",
      "reference.search",
    ]);
    for (const tool of services.tools) {
      expect(tool.capability).toBe("reference.inspect");
      expect(tool.definition.description).toContain("reference");
    }
  });

  it("rejects invalid inputs", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const listTool = toolByName(services, "reference.list");
    expect(await listTool.execute({}, {})).toMatchObject({ status: "invalid_input" });
    expect(await listTool.execute({ reference: 42 }, {})).toMatchObject({
      status: "invalid_input",
    });

    const readTool = toolByName(services, "reference.read");
    expect(await readTool.execute({ reference: "docs" }, {})).toMatchObject({
      status: "invalid_input",
    });
    expect(
      await readTool.execute({ reference: "docs", path: "a.txt", mode: "bogus" }, {}),
    ).toMatchObject({
      status: "invalid_input",
    });
    expect(
      await readTool.execute({ reference: "docs", path: "a.txt", startLine: 5, endLine: 2 }, {}),
    ).toMatchObject({ status: "invalid_input" });

    const searchTool = toolByName(services, "reference.search");
    expect(await searchTool.execute({ reference: "docs" }, {})).toMatchObject({
      status: "invalid_input",
    });
  });

  it("fails closed for unknown and unresolved references", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const listTool = toolByName(services, "reference.list");
    expect(await listTool.execute({ reference: "nope" }, {})).toMatchObject({
      status: "unavailable",
      message: new RegExp("Unknown reference"),
    });

    const repositoryServices = await createReferenceServices({
      declarations: [
        {
          alias: "repo1",
          kind: "repository",
          source: {
            kind: "repository",
            repository: "https://github.com/owner/repo",
            ref: { kind: "tag", tag: "v1" },
          },
          description: null,
        },
      ],
      workspaceRoot: workspace.root,
    });
    const repoListTool = toolByName(repositoryServices, "reference.list");
    expect(await repoListTool.execute({ reference: "repo1" }, {})).toMatchObject({
      status: "unavailable",
      message: new RegExp("sandboxed git"),
    });
  });

  it("lists with alias, revision anchor, and no absolute paths", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const listTool = toolByName(services, "reference.list");
    const output = successOutput(await listTool.execute({ reference: "docs" }, {}));
    expect(output["reference"]).toBe("@reference/docs");
    expect(output["revision"]).toMatchObject({
      kind: "local-directory",
      fingerprint: /^[0-9a-f]{64}$/,
    });
    expect(output["path"]).toBe(".");
    const serialized = JSON.stringify(output);
    expect(serialized).not.toContain(fixture.root);
    expect(serialized).not.toContain("\\");
  });

  it("reads exact, structural, and summary modes", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const readTool = toolByName(services, "reference.read");

    const exact = successOutput(await readTool.execute({ reference: "docs", path: "a.txt" }, {}));
    expect(exact["content"]).toBe("hello world\n");
    expect(exact["sha256"]).toBe(createHash("sha256").update("hello world\n").digest("hex"));
    expect(exact["mode"]).toBe("exact");

    const structural = successOutput(
      await readTool.execute({ reference: "docs", path: "sub/b.gd", mode: "structural" }, {}),
    );
    const structure = structural["structure"] as JsonObject;
    expect(structure["functions"]).toBeInstanceOf(Array);
    expect((structure["functions"] as { name: string }[]).map((fn) => fn.name)).toEqual(["move"]);
    expect(structural["content"]).toBeNull();

    const summary = successOutput(
      await readTool.execute({ reference: "docs", path: "sub/b.gd", mode: "summary" }, {}),
    );
    expect(typeof summary["summary"]).toBe("string");
    expect(summary["advisory"]).toBe(true);
  });

  it("maps read failures to typed tool outcomes", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const readTool = toolByName(services, "reference.read");
    expect(await readTool.execute({ reference: "docs", path: "missing.txt" }, {})).toMatchObject({
      status: "failed",
    });
    expect(await readTool.execute({ reference: "docs", path: "../escape.txt" }, {})).toMatchObject({
      status: "failed",
    });
    expect(
      await readTool.execute({ reference: "docs", path: "sub/notes.md", mode: "structural" }, {}),
    ).toMatchObject({
      status: "failed",
      message: new RegExp("GDScript"),
    });
  });

  it("searches with reference-relative matches and truncation disclosure", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const searchTool = toolByName(services, "reference.search");
    const output = successOutput(
      await searchTool.execute({ reference: "docs", query: "hello" }, {}),
    );
    expect(output["reference"]).toBe("@reference/docs");
    expect(output["revision"]).toMatchObject({
      kind: "local-directory",
      fingerprint: /^[0-9a-f]{64}$/,
    });
    expect(output["query"]).toBe("hello");
    const matches = output["matches"] as { path: string }[];
    expect(matches.map((match) => match.path).sort()).toEqual(["a.txt", "sub/notes.md"]);
    expect(output["truncated"]).toBe(false);
    expect(output["truncationReason"]).toBeNull();
    expect(JSON.stringify(output)).not.toContain(fixture.root);
  });
});
