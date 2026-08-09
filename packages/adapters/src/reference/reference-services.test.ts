import { afterEach, describe, expect, it } from "vitest";
import {
  createFakeRepositoryBackend,
  createReferenceResolver,
  createRepositoryResolver,
} from "./reference-resolver.js";
import { createFakeRepositoryMaterializer } from "./reference-test-support.js";
import { createReferenceServices } from "./reference-services.js";
import type { ReferenceAlias } from "@solaris/core";
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

describe("createReferenceServices", () => {
  it("assembles registry, access, and tools for a local-directory declaration", async () => {
    const fixture = await withRoot();
    await writeFixtureFiles(fixture.root, { "a.txt": "x" });
    const workspace = await withRoot();
    const services = await createReferenceServices({
      declarations: [
        {
          alias: "docs",
          kind: "local-directory",
          source: { kind: "local-directory", path: fixture.root },
          description: null,
        },
      ],
      workspaceRoot: workspace.root,
    });
    expect(services.registry.size).toBe(1);
    const reference = services.registry.get("docs" as ReferenceAlias);
    expect(reference).toMatchObject({ status: "ready", trust: "explicit-user" });
    const revision = services.registry.revision("docs" as ReferenceAlias);
    expect(revision).not.toBeNull();
    if (revision !== null) {
      expect(revision.identity.kind).toBe("local-directory");
    }
    expect(services.tools).toHaveLength(3);
    expect(services.cacheStore.status()).toMatchObject({ status: "unavailable" });
    expect(() => services.close()).not.toThrow();
  });

  it("fails closed for repository declarations by default (no git execution)", async () => {
    const workspace = await withRoot();
    const services = await createReferenceServices({
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
    const reference = services.registry.get("repo1" as ReferenceAlias);
    expect(reference).toMatchObject({
      status: "unavailable",
      failureReason: new RegExp("sandboxed git"),
    });
    const id = services.registry.list()[0]?.id;
    expect(id).toBeDefined();
    if (id !== undefined) {
      const materialized = await services.materializer.materialize(id, {
        kind: "repository",
        origin: "https://github.com/owner/repo",
        commit: "a1b2c3d",
        requestedRef: { kind: "tag", tag: "v1" },
      });
      expect(materialized).toMatchObject({ status: "unavailable" });
    }
  });

  it("declines local-directory references inside the workspace namespace", async () => {
    const workspace = await withRoot();
    await writeFixtureFiles(workspace.root, { "a.txt": "x" });
    const services = await createReferenceServices({
      declarations: [
        {
          alias: "inside",
          kind: "local-directory",
          source: { kind: "local-directory", path: workspace.root },
          description: null,
        },
      ],
      workspaceRoot: workspace.root,
    });
    const reference = services.registry.get("inside" as ReferenceAlias);
    expect(reference).toMatchObject({
      status: "declined",
      failureReason: "reference root must be outside the workspace namespace",
    });
  });

  it("supports a fully fake repository pipeline when injected", async () => {
    const ORIGIN = "https://github.com/owner/repo";
    const COMMIT = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";
    const workspace = await withRoot();
    const baseDir = await withRoot();
    const services = await createReferenceServices({
      declarations: [
        {
          alias: "repo1",
          kind: "repository",
          source: {
            kind: "repository",
            repository: ORIGIN,
            ref: { kind: "tag", tag: "v1" },
          },
          description: null,
        },
      ],
      workspaceRoot: workspace.root,
      resolver: createReferenceResolver({
        repository: createRepositoryResolver(
          createFakeRepositoryBackend({
            [ORIGIN]: {
              commits: { [COMMIT]: { "readme.md": "hello from repo\n" } },
              tags: { v1: COMMIT },
              branches: { main: COMMIT },
            },
          }),
        ),
      }),
      materializer: createFakeRepositoryMaterializer(
        {
          [ORIGIN]: {
            commits: { [COMMIT]: { "readme.md": "hello from repo\n" } },
            tags: { v1: COMMIT },
            branches: { main: COMMIT },
          },
        },
        { baseDir: baseDir.root },
      ),
    });
    expect(services.registry.get("repo1" as ReferenceAlias)).toMatchObject({ status: "ready" });
    const revision = services.registry.revision("repo1" as ReferenceAlias);
    expect(revision).not.toBeNull();
    if (revision !== null && revision.identity.kind === "repository") {
      expect(revision.identity.commit).toBe(COMMIT);
    }
    const id = services.registry.list()[0]?.id;
    expect(id).toBeDefined();
    if (id === undefined) {
      throw new Error("expected a registry entry");
    }
    const read = await services.access.read({
      reference: id,
      path: "readme.md",
      mode: "exact",
    });
    expect(read.status).toBe("ok");
    if (read.status === "ok") {
      expect(read.content).toBe("hello from repo\n");
      expect(read.path).toBe("readme.md");
    }
  });
});
