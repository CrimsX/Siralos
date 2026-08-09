import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { canonicalizeJson, createWorkspaceRevisionRegistry, sha256Hex } from "@solaris/core";
import { SYMLINKS_SUPPORTED } from "../tools/workspace/workspace-fixtures.js";
import {
  createProjectInstructionService,
  discoverProjectInstructions,
} from "./instruction-discovery.js";

const tempRoots: string[] = [];

async function createFixture(files: Record<string, string>): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "solaris-instructions-"));
  tempRoots.push(root);
  for (const [relative, content] of Object.entries(files)) {
    const absolute = join(root, relative);
    await mkdir(join(absolute, ".."), { recursive: true });
    await writeFile(absolute, content, "utf8");
  }
  return root;
}

afterEach(async () => {
  for (const root of tempRoots.splice(0)) {
    await rm(root, { recursive: true, force: true });
  }
});

const FILES = {
  "AGENTS.md": "Root guidance.",
  "src/AGENTS.md": "src guidance.",
  "src/player/AGENTS.md": "player guidance.",
  "src/enemy/AGENTS.md": "enemy guidance.",
  "src/player/player.gd": "extends CharacterBody2D\n",
};

describe("instruction discovery", () => {
  it("discovers hierarchical AGENTS.md files with scoped sources", async () => {
    const root = await createFixture(FILES);
    const outcome = await discoverProjectInstructions({ workspaceRoot: root });
    expect(outcome.truncated).toBe(false);
    const byPath = new Map(
      outcome.instructions.map((instruction) => [instruction.source.path ?? ".", instruction]),
    );
    expect(byPath.get(".")?.source.kind).toBe("project_root");
    expect(byPath.get("src")?.source.kind).toBe("project_directory");
    expect(byPath.get("src/player")?.source.kind).toBe("project_directory");
    expect(byPath.get("src/enemy")?.source.kind).toBe("project_directory");
  });

  it("binds instructions to exact file revisions", async () => {
    const root = await createFixture(FILES);
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const service = createProjectInstructionService({ workspaceRoot: root, revisions });
    await service.load();
    const before = await service.resolveForPath("src/player/controller.gd");
    const beforeRoot = before.instructions.find(
      (instruction) => instruction.source.kind === "project_root",
    );
    expect(beforeRoot?.sourceRevision).toMatch(/^rev_/);
    const beforeInventory = service.revision();

    await writeFile(join(root, "AGENTS.md"), "Changed root guidance.", "utf8");
    await service.refresh();
    const after = await service.resolveForPath("src/player/controller.gd");
    const afterRoot = after.instructions.find(
      (instruction) => instruction.source.kind === "project_root",
    );
    expect(afterRoot?.sourceRevision).not.toBe(beforeRoot?.sourceRevision);
    expect(after.revision).not.toBe(before.revision);
    expect(service.revision()).not.toBe(beforeInventory);
  });

  it("does not apply sibling-directory guidance", async () => {
    const root = await createFixture(FILES);
    const service = createProjectInstructionService({ workspaceRoot: root });
    await service.load();
    const set = await service.resolveForPath("src/player/controller.gd");
    const scopes = set.instructions.map((instruction) => instruction.source.path ?? ".");
    expect(scopes).toEqual([".", "src", "src/player"]);
  });

  it("applies the union of scopes for multi-file tasks", async () => {
    const root = await createFixture(FILES);
    const service = createProjectInstructionService({ workspaceRoot: root });
    await service.load();
    const set = await service.resolveForPaths(["src/player/controller.gd", "src/enemy/enemy.gd"]);
    const scopes = set.instructions.map((instruction) => instruction.source.path ?? ".");
    expect(scopes).toEqual([".", "src", "src/enemy", "src/player"]);
  });

  it("never traverses symbolic links (containment)", async () => {
    if (!SYMLINKS_SUPPORTED) {
      return;
    }
    const outside = await createFixture({ "AGENTS.md": "outside guidance." });
    const root = await createFixture({
      "AGENTS.md": "root guidance.",
      "linked/AGENTS.md": "should not be discovered",
    });
    await rm(join(root, "linked"), { recursive: true, force: true });
    await symlink(outside, join(root, "linked"), "dir");
    const outcome = await discoverProjectInstructions({ workspaceRoot: root });
    const paths = outcome.instructions.map((instruction) => instruction.source.path ?? ".");
    expect(paths).toEqual(["."]);
    expect(outcome.instructions[0]?.content).toBe("root guidance.");
  });

  it("rejects out-of-containment focus paths", async () => {
    const root = await createFixture(FILES);
    const service = createProjectInstructionService({ workspaceRoot: root });
    await service.load();
    const escaped = await service.resolveForPath("../outside/AGENTS.md");
    expect(escaped.instructions).toEqual([]);
  });

  it("skips excluded directories including .solaris", async () => {
    const root = await createFixture({
      "AGENTS.md": "root.",
      "node_modules/AGENTS.md": "excluded",
      ".git/AGENTS.md": "excluded",
      ".solaris/AGENTS.md": "future user-level guidance, not project instructions",
      "dist/AGENTS.md": "excluded",
    });
    const outcome = await discoverProjectInstructions({ workspaceRoot: root });
    expect(outcome.instructions.map((instruction) => instruction.source.path ?? ".")).toEqual([
      ".",
    ]);
  });

  it("is bounded: oversized instruction files are skipped, never loaded", async () => {
    const root = await createFixture({ "AGENTS.md": "root." });
    await mkdir(join(root, "src"), { recursive: true });
    await writeFile(join(root, "src", "AGENTS.md"), "x".repeat(70_000), "utf8");
    const outcome = await discoverProjectInstructions({ workspaceRoot: root });
    expect(outcome.instructions.map((instruction) => instruction.source.path ?? ".")).toEqual([
      ".",
    ]);
  });
});
