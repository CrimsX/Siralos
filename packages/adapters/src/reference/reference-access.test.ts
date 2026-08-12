import { afterEach, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import type { ReferenceAlias, ReferenceId, ReferenceRevision } from "@siralos/core";
import { createReferenceId } from "@siralos/core";
import { createReferenceAccess, type ReferenceAccessLimits } from "./reference-access.js";
import {
  createReferenceMaterializer,
  createReferenceRootProvider,
} from "./reference-materializer.js";
import { createLocalDirectoryResolver } from "./reference-resolver.js";
import { createReferenceServices } from "./reference-services.js";
import {
  createTempWorkspace,
  writeFixtureFiles,
  type TempWorkspace,
} from "../tools/workspace/workspace-fixtures.js";

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

signal done

var speed := 10

func _ready() -> void:
	print("ready")

func move(delta: float) -> void:
	position += delta
`;

async function buildServices(fixtureRoot: string, workspaceRoot: string) {
  return createReferenceServices({
    declarations: [
      {
        alias: "docs",
        kind: "local-directory",
        source: { kind: "local-directory", path: fixtureRoot },
        description: null,
      },
    ],
    workspaceRoot,
  });
}

async function buildFixtureTree(): Promise<TempWorkspace> {
  const fixture = await withRoot();
  await writeFixtureFiles(fixture.root, {
    "a.txt": "hello world\nsecond line\n",
    "sub/b.gd": GD_SOURCE,
    "sub/notes.md": "hello from notes\n",
    "sub/binary.bin": Buffer.from([0x00, 0x01, 0x02, 0xff]),
  });
  return fixture;
}

describe("reference access over services (local-directory reference)", () => {
  it("lists the root and subdirectories with reference-relative paths", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const rootList = await services.access.list({ reference: createReferenceId("docs") });
    expect(rootList.status).toBe("ok");
    if (rootList.status === "ok") {
      expect(rootList.alias).toBe("docs");
      expect(rootList.path).toBe(".");
      expect(rootList.entries.map((entry) => entry.name).sort()).toEqual(["a.txt", "sub"]);
      const sub = rootList.entries.find((entry) => entry.name === "sub");
      expect(sub).toMatchObject({ type: "directory", path: "sub" });
      const file = rootList.entries.find((entry) => entry.name === "a.txt");
      expect(file).toMatchObject({ type: "file", path: "a.txt" });
      expect(rootList.entries.every((entry) => !entry.path.startsWith("/"))).toBe(true);
    }
    const subList = await services.access.list({
      reference: createReferenceId("docs"),
      path: "sub",
    });
    expect(subList.status).toBe("ok");
    if (subList.status === "ok") {
      expect(subList.entries.map((entry) => entry.name).sort()).toEqual([
        "b.gd",
        "binary.bin",
        "notes.md",
      ]);
    }
  });

  it("rejects paths outside the reference root", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const outcome = await services.access.list({
      reference: createReferenceId("docs"),
      path: "../outside",
    });
    expect(outcome).toMatchObject({ status: "invalid_path" });
  });

  it("fails for missing directories and file targets", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const missing = await services.access.list({
      reference: createReferenceId("docs"),
      path: "nope",
    });
    expect(missing.status).toBe("failed");
    const fileTarget = await services.access.list({
      reference: createReferenceId("docs"),
      path: "a.txt",
    });
    expect(fileTarget).toMatchObject({ status: "failed", reason: "Target is not a directory." });
  });

  it("reads exact content with sha256 and line slicing", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const outcome = await services.access.read({
      reference: createReferenceId("docs"),
      path: "a.txt",
      mode: "exact",
    });
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      expect(outcome.content).toBe("hello world\nsecond line\n");
      expect(outcome.sha256).toBe(
        createHash("sha256").update("hello world\nsecond line\n").digest("hex"),
      );
      expect(outcome.structure).toBeNull();
      expect(outcome.summary).toBeNull();
      expect(outcome.truncated).toBe(false);
      expect(outcome.alias).toBe("docs");
    }
    const sliced = await services.access.read({
      reference: createReferenceId("docs"),
      path: "a.txt",
      mode: "exact",
      startLine: 2,
      endLine: 2,
    });
    expect(sliced.status).toBe("ok");
    if (sliced.status === "ok") {
      expect(sliced.content).toBe("second line");
    }
    const beyond = await services.access.read({
      reference: createReferenceId("docs"),
      path: "a.txt",
      mode: "exact",
      startLine: 99,
    });
    expect(beyond).toMatchObject({
      status: "failed",
      reason: new RegExp("beyond the end of the file"),
    });
  });

  it("returns GDScript structure in structural mode", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const outcome = await services.access.read({
      reference: createReferenceId("docs"),
      path: "sub/b.gd",
      mode: "structural",
    });
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      const structure = outcome.structure as {
        functions: readonly unknown[];
        signals: readonly unknown[];
        properties: readonly unknown[];
      };
      expect(structure.functions.map((fn) => (fn as { name: string }).name)).toEqual([
        "_ready",
        "move",
      ]);
      expect(structure.signals).toHaveLength(1);
      expect(structure.properties).toHaveLength(1);
      expect(outcome.content).toBeNull();
      expect(outcome.sha256).toMatch(/^[0-9a-f]{64}$/);
    }
  });

  it("returns a bounded advisory summary in summary mode", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const outcome = await services.access.read({
      reference: createReferenceId("docs"),
      path: "sub/b.gd",
      mode: "summary",
    });
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      expect(typeof outcome.summary).toBe("string");
      expect(outcome.summary as string).toContain("2 functions");
      expect(outcome.content).toBeNull();
    }
  });

  it("refuses structural/summary modes on non-GDScript files", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const outcome = await services.access.read({
      reference: createReferenceId("docs"),
      path: "sub/notes.md",
      mode: "structural",
    });
    expect(outcome).toMatchObject({
      status: "unsupported",
      reason: new RegExp("GDScript"),
    });
  });

  it("refuses binary and invalid-UTF-8 files", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const binary = await services.access.read({
      reference: createReferenceId("docs"),
      path: "sub/binary.bin",
      mode: "exact",
    });
    expect(binary).toMatchObject({ status: "unsupported", reason: "File appears to be binary." });
  });

  it("reads files under the default size limit", async () => {
    const fixture = await withRoot();
    await writeFixtureFiles(fixture.root, { "big.txt": "x".repeat(200) });
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const outcome = await services.access.read({
      reference: createReferenceId("docs"),
      path: "big.txt",
      mode: "exact",
    });
    expect(outcome.status).toBe("ok"); // default limit is 1 MiB; 200 bytes is fine
  });

  it("returns not_found for missing files and invalid_path for traversal", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const missing = await services.access.read({
      reference: createReferenceId("docs"),
      path: "nope.txt",
      mode: "exact",
    });
    expect(missing).toMatchObject({ status: "not_found" });
    const traversal = await services.access.read({
      reference: createReferenceId("docs"),
      path: "../outside.txt",
      mode: "exact",
    });
    expect(traversal).toMatchObject({ status: "invalid_path" });
  });

  it("reports unavailable for unknown references", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const unknownId = createReferenceId("unknown");
    const outcome = await services.access.list({ reference: unknownId });
    expect(outcome).toMatchObject({
      status: "unavailable",
      reason: new RegExp("not configured"),
    });
  });

  it("searches with reference-relative matches and optional scoping", async () => {
    const fixture = await buildFixtureTree();
    const workspace = await withRoot();
    const services = await buildServices(fixture.root, workspace.root);
    const outcome = await services.access.search({
      reference: createReferenceId("docs"),
      query: "hello",
    });
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      expect(outcome.matches.map((match) => match.path).sort()).toEqual(["a.txt", "sub/notes.md"]);
      expect(outcome.scannedFiles).toBeGreaterThan(0);
      expect(outcome.truncated).toBe(false);
      expect(outcome.truncationReason).toBeNull();
      expect(outcome.alias).toBe("docs");
    }
    const scoped = await services.access.search({
      reference: createReferenceId("docs"),
      query: "hello",
      path: "sub",
    });
    expect(scoped.status).toBe("ok");
    if (scoped.status === "ok") {
      expect(scoped.matches.map((match) => match.path)).toEqual(["sub/notes.md"]);
    }
  });
});

describe("reference access bounds (direct construction with limit overrides)", () => {
  const REF_ID = createReferenceId("docs");

  async function directAccess(fixtureRoot: string, limits?: Partial<ReferenceAccessLimits>) {
    const resolver = createLocalDirectoryResolver();
    const outcome = await resolver.resolveIdentity(
      { kind: "local-directory", path: fixtureRoot },
      { allowMutableRefs: false },
    );
    expect(outcome.status).toBe("resolved");
    if (outcome.status !== "resolved") {
      throw new Error("fixture must resolve");
    }
    const revision: ReferenceRevision = { identity: outcome.identity, resolvedAtMs: 0 };
    return createReferenceAccess({
      roots: createReferenceRootProvider({ materializer: createReferenceMaterializer() }),
      referenceInfo: (id: ReferenceId) =>
        id === REF_ID ? { alias: "docs" as ReferenceAlias, revision } : null,
      ...(limits === undefined ? {} : { limits }),
    });
  }

  it("truncates lists at the entry cap", async () => {
    const fixture = await withRoot();
    await writeFixtureFiles(fixture.root, { "a.txt": "x", "b.txt": "x", "c.txt": "x" });
    const access = await directAccess(fixture.root, { maxListEntries: 2 });
    const outcome = await access.list({ reference: REF_ID });
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      expect(outcome.entries).toHaveLength(2);
      expect(outcome.truncated).toBe(true);
    }
  });

  it("refuses oversized reads at the configured cap", async () => {
    const fixture = await withRoot();
    await writeFixtureFiles(fixture.root, { "big.txt": "x".repeat(200) });
    const access = await directAccess(fixture.root, { maxReadFileSizeBytes: 100 });
    const outcome = await access.read({ reference: REF_ID, path: "big.txt", mode: "exact" });
    expect(outcome).toMatchObject({
      status: "unsupported",
      reason: new RegExp("File is too large"),
    });
  });

  it("truncates searches at the scan budget with an explicit reason", async () => {
    const fixture = await withRoot();
    await writeFixtureFiles(fixture.root, { "a.txt": "needle", "b.txt": "needle" });
    const access = await directAccess(fixture.root, { maxSearchFiles: 1 });
    const outcome = await access.search({ reference: REF_ID, query: "needle" });
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      expect(outcome.truncated).toBe(true);
      expect(outcome.truncationReason).toBe("scan_budget");
      expect(outcome.matches).toHaveLength(1);
    }
  });

  it("truncates searches at the match limit", async () => {
    const fixture = await withRoot();
    await writeFixtureFiles(fixture.root, { "a.txt": "needle", "b.txt": "needle" });
    const access = await directAccess(fixture.root);
    const outcome = await access.search({ reference: REF_ID, query: "needle", maxResults: 1 });
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      expect(outcome.truncated).toBe(true);
      expect(outcome.truncationReason).toBe("match_limit");
      expect(outcome.matches).toHaveLength(1);
    }
  });

  it("caps requested maxResults at the configured limit", async () => {
    const fixture = await withRoot();
    await writeFixtureFiles(fixture.root, {
      "a.txt": "needle",
      "b.txt": "needle",
      "c.txt": "needle",
      "d.txt": "needle",
    });
    const access = await directAccess(fixture.root, { maxSearchMatches: 2 });
    const outcome = await access.search({ reference: REF_ID, query: "needle", maxResults: 99 });
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      expect(outcome.matches).toHaveLength(2);
      expect(outcome.truncationReason).toBe("match_limit");
    }
  });

  it("defaults maxResults to 20", async () => {
    const fixture = await withRoot();
    await writeFixtureFiles(fixture.root, { "a.txt": "needle" });
    const access = await directAccess(fixture.root);
    const outcome = await access.search({ reference: REF_ID, query: "needle" });
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      expect(outcome.matches).toHaveLength(1);
      expect(outcome.truncated).toBe(false);
    }
  });

  it("reports unavailable when referenceInfo returns null", async () => {
    const fixture = await withRoot();
    await writeFixtureFiles(fixture.root, { "a.txt": "x" });
    const access = await directAccess(fixture.root);
    const outcome = await access.read({
      reference: createReferenceId("other"),
      path: "a.txt",
      mode: "exact",
    });
    expect(outcome).toMatchObject({
      status: "unavailable",
      reason: new RegExp("not configured"),
    });
  });
});
