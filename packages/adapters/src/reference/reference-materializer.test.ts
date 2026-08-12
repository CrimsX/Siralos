import { afterEach, describe, expect, it } from "vitest";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ReferenceId, ResolvedReferenceIdentity } from "@siralos/core";
import { createReferenceId } from "@siralos/core";
import { createFakeRepositoryMaterializer } from "./reference-test-support.js";
import {
  createReferenceMaterializer,
  createReferenceRootProvider,
  REPOSITORY_MATERIALIZATION_UNAVAILABLE_MESSAGE,
} from "./reference-materializer.js";
import type { FakeRepositoryFixture } from "./reference-resolver.js";
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

const REF_ID = createReferenceId("docs");
const REPO_IDENTITY: ResolvedReferenceIdentity = {
  kind: "repository",
  origin: "https://github.com/owner/repo",
  commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0",
  requestedRef: { kind: "tag", tag: "v1" },
};

describe("createReferenceMaterializer (real)", () => {
  it("materializes local-directory identities with no copy", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "x" });
    const materializer = createReferenceMaterializer();
    const identity: ResolvedReferenceIdentity = {
      kind: "local-directory",
      canonicalPath: root.root,
      fingerprint: "fp",
    };
    expect(materializer.status(REF_ID)).toBe("not-materialized");
    const outcome = await materializer.materialize(REF_ID, identity);
    expect(outcome).toEqual({ status: "materialized", root: root.root });
    expect(materializer.status(REF_ID)).toBe("not-required");
  });

  it("fails closed for repository identities with zero filesystem operations", async () => {
    const materializer = createReferenceMaterializer();
    const outcome = await materializer.materialize(REF_ID, REPO_IDENTITY);
    expect(outcome).toEqual({
      status: "unavailable",
      reason: REPOSITORY_MATERIALIZATION_UNAVAILABLE_MESSAGE,
    });
    expect(materializer.status(REF_ID)).toBe("unavailable");
  });
});

describe("createFakeRepositoryMaterializer", () => {
  const ORIGIN = "https://github.com/owner/repo";
  const COMMIT = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";
  const fixture: FakeRepositoryFixture = {
    [ORIGIN]: {
      commits: { [COMMIT]: { "a.txt": "hello", "sub/b.gd": "func f():\n\tpass\n" } },
      tags: { v1: COMMIT },
      branches: { main: COMMIT },
    },
  };

  it("writes fixture files deterministically under baseDir", async () => {
    const baseDir = join(tmpdir(), `siralos-fake-materializer-${Date.now()}`);
    roots.push({ root: baseDir, cleanup: async () => {} });
    const materializer = createFakeRepositoryMaterializer(fixture, { baseDir });
    expect(materializer.status(REF_ID)).toBe("not-materialized");
    const outcome = await materializer.materialize(REF_ID, {
      kind: "repository",
      origin: ORIGIN,
      commit: COMMIT,
      requestedRef: { kind: "tag", tag: "v1" },
    });
    expect(outcome.status).toBe("materialized");
    if (outcome.status === "materialized") {
      expect(outcome.root).toBe(join(baseDir, "https_github.com_owner_repo", COMMIT));
      expect(await readFile(join(outcome.root, "a.txt"), "utf8")).toBe("hello");
      expect(await readFile(join(outcome.root, "sub", "b.gd"), "utf8")).toBe("func f():\n\tpass\n");
    }
    expect(materializer.status(REF_ID)).toBe("materialized");
  });

  it("fails for unknown origins and content-less commits", async () => {
    const baseDir = join(tmpdir(), `siralos-fake-materializer-${Date.now()}`);
    roots.push({ root: baseDir, cleanup: async () => {} });
    const materializer = createFakeRepositoryMaterializer(fixture, { baseDir });
    const unknown = await materializer.materialize(REF_ID, {
      kind: "repository",
      origin: "https://github.com/other/repo",
      commit: COMMIT,
      requestedRef: { kind: "tag", tag: "v1" },
    });
    expect(unknown).toMatchObject({
      status: "failed",
      reason: new RegExp("Unknown repository origin"),
    });
    const noContent = await materializer.materialize(REF_ID, {
      kind: "repository",
      origin: ORIGIN,
      commit: "ffffffffffffffffffffffffffffffffffffffff",
      requestedRef: { kind: "commit", commit: "ffffffffffffffffffffffffffffffffffffffff" },
    });
    expect(noContent).toMatchObject({
      status: "failed",
      reason: new RegExp("has no file content"),
    });
  });

  it("fails closed on fixture paths that could escape the root", async () => {
    const baseDir = join(tmpdir(), `siralos-fake-materializer-${Date.now()}`);
    roots.push({ root: baseDir, cleanup: async () => {} });
    const hostile: FakeRepositoryFixture = {
      [ORIGIN]: { commits: { [COMMIT]: { "../escape.txt": "x" } }, tags: {}, branches: {} },
    };
    const materializer = createFakeRepositoryMaterializer(hostile, { baseDir });
    const outcome = await materializer.materialize(REF_ID, {
      kind: "repository",
      origin: ORIGIN,
      commit: COMMIT,
      requestedRef: { kind: "commit", commit: COMMIT },
    });
    expect(outcome).toMatchObject({
      status: "failed",
      reason: new RegExp("safe relative path"),
    });
  });
});

describe("createReferenceRootProvider", () => {
  it("maps local-directory identities to their canonical path", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "x" });
    const provider = createReferenceRootProvider({ materializer: createReferenceMaterializer() });
    const identity: ResolvedReferenceIdentity = {
      kind: "local-directory",
      canonicalPath: root.root,
      fingerprint: "fp",
    };
    const result = await provider.rootFor(REF_ID, identity);
    expect(result).toEqual({ path: root.root, kind: "local-directory" });
  });

  it("returns null when repository materialization is unavailable", async () => {
    const provider = createReferenceRootProvider({ materializer: createReferenceMaterializer() });
    expect(await provider.rootFor(REF_ID, REPO_IDENTITY)).toBeNull();
  });

  it("returns the materialized root for repository identities via the fake", async () => {
    const ORIGIN = "https://github.com/owner/repo";
    const COMMIT = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";
    const fixture: FakeRepositoryFixture = {
      [ORIGIN]: { commits: { [COMMIT]: { "a.txt": "x" } }, tags: {}, branches: {} },
    };
    const baseDir = join(tmpdir(), `siralos-fake-rootprovider-${Date.now()}`);
    roots.push({ root: baseDir, cleanup: async () => {} });
    const provider = createReferenceRootProvider({
      materializer: createFakeRepositoryMaterializer(fixture, { baseDir }),
    });
    const result = await provider.rootFor(REF_ID, {
      kind: "repository",
      origin: ORIGIN,
      commit: COMMIT,
      requestedRef: { kind: "commit", commit: COMMIT },
    });
    expect(result).toEqual({
      path: join(baseDir, "https_github.com_owner_repo", COMMIT),
      kind: "repository",
    });
  });

  it("resolves references by id through the provider (id is a plain string key)", async () => {
    const otherId: ReferenceId = createReferenceId("other");
    const root = await withRoot();
    const provider = createReferenceRootProvider({ materializer: createReferenceMaterializer() });
    const result = await provider.rootFor(otherId, {
      kind: "local-directory",
      canonicalPath: root.root,
      fingerprint: "fp",
    });
    expect(result).toEqual({ path: root.root, kind: "local-directory" });
  });
});
