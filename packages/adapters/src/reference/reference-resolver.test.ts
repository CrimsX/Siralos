import { afterEach, describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { chmod, realpath, writeFile } from "node:fs/promises";
import path from "node:path";
import type { ReferenceSource, RepositoryRef } from "@siralos/core";
import {
  createFakeRepositoryBackend,
  createLocalDirectoryResolver,
  createReferenceResolver,
  createRepositoryResolver,
  createUnavailableRepositoryBackend,
  MUTABLE_REF_REFUSAL,
  type FakeRepositoryFixture,
} from "./reference-resolver.js";
import {
  createSymlink,
  createTempWorkspace,
  SYMLINKS_SUPPORTED,
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

const LOCAL_SOURCE: ReferenceSource = { kind: "local-directory", path: "/unused" };

describe("createLocalDirectoryResolver", () => {
  it("canonicalizes the path and produces a hex fingerprint", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "hello" });
    const resolver = createLocalDirectoryResolver();
    const outcome = await resolver.resolveIdentity(
      { kind: "local-directory", path: root.root },
      { allowMutableRefs: false },
    );
    expect(outcome.status).toBe("resolved");
    if (outcome.status === "resolved") {
      expect(outcome.identity).toMatchObject({
        kind: "local-directory",
        canonicalPath: await realpath(root.root),
        fingerprint: /^[0-9a-f]{64}$/,
      });
    }
  });

  it("is deterministic: same content, same fingerprint", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "hello", "sub/b.txt": "world" });
    const first = await createLocalDirectoryResolver().resolveIdentity(
      { kind: "local-directory", path: root.root },
      { allowMutableRefs: false },
    );
    const second = await createLocalDirectoryResolver().resolveIdentity(
      { kind: "local-directory", path: root.root },
      { allowMutableRefs: false },
    );
    expect(first.status).toBe("resolved");
    expect(second.status).toBe("resolved");
    if (
      first.status === "resolved" &&
      second.status === "resolved" &&
      first.identity.kind === "local-directory" &&
      second.identity.kind === "local-directory"
    ) {
      expect(first.identity.fingerprint).toBe(second.identity.fingerprint);
    }
  });

  it("changes the fingerprint when content changes", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "hello" });
    const before = await createLocalDirectoryResolver().resolveIdentity(
      { kind: "local-directory", path: root.root },
      { allowMutableRefs: false },
    );
    await writeFile(path.join(root.root, "a.txt"), "changed");
    const after = await createLocalDirectoryResolver().resolveIdentity(
      { kind: "local-directory", path: root.root },
      { allowMutableRefs: false },
    );
    expect(before.status).toBe("resolved");
    expect(after.status).toBe("resolved");
    if (
      before.status === "resolved" &&
      after.status === "resolved" &&
      before.identity.kind === "local-directory" &&
      after.identity.kind === "local-directory"
    ) {
      expect(after.identity.fingerprint).not.toBe(before.identity.fingerprint);
    }
  });

  it(
    "ignores symlinks in the tree (never traversed, never hashed)",
    { skip: !SYMLINKS_SUPPORTED },
    async () => {
      const root = await withRoot();
      await writeFixtureFiles(root.root, { "a.txt": "hello" });
      const before = await createLocalDirectoryResolver().resolveIdentity(
        { kind: "local-directory", path: root.root },
        { allowMutableRefs: false },
      );
      await createSymlink(path.join(root.root, "a.txt"), path.join(root.root, "link.txt"));
      const after = await createLocalDirectoryResolver().resolveIdentity(
        { kind: "local-directory", path: root.root },
        { allowMutableRefs: false },
      );
      expect(before.status).toBe("resolved");
      expect(after.status).toBe("resolved");
      if (
        before.status === "resolved" &&
        after.status === "resolved" &&
        before.identity.kind === "local-directory" &&
        after.identity.kind === "local-directory"
      ) {
        expect(after.identity.fingerprint).toBe(before.identity.fingerprint);
      }
    },
  );

  it("fails when the manifest entry cap is exceeded", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, {
      "a.txt": "x",
      "b.txt": "x",
      "c.txt": "x",
      "d.txt": "x",
    });
    const resolver = createLocalDirectoryResolver({ limits: { maxManifestEntries: 3 } });
    const outcome = await resolver.resolveIdentity(
      { kind: "local-directory", path: root.root },
      { allowMutableRefs: false },
    );
    expect(outcome).toMatchObject({
      status: "failed",
      reason: new RegExp("manifest is too large"),
    });
  });

  it("fails when the manifest byte cap is exceeded", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "x".repeat(64) });
    const resolver = createLocalDirectoryResolver({ limits: { maxManifestBytes: 32 } });
    const outcome = await resolver.resolveIdentity(
      { kind: "local-directory", path: root.root },
      { allowMutableRefs: false },
    );
    expect(outcome).toMatchObject({
      status: "failed",
      reason: new RegExp("manifest is too large"),
    });
  });

  it("fails when a single file exceeds the per-file hash cap", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "x".repeat(64) });
    const resolver = createLocalDirectoryResolver({ limits: { maxFileSha256Bytes: 32 } });
    const outcome = await resolver.resolveIdentity(
      { kind: "local-directory", path: root.root },
      { allowMutableRefs: false },
    );
    expect(outcome).toMatchObject({
      status: "failed",
      reason: new RegExp("not fingerprintable"),
    });
  });

  it(
    "fails on special files (non-fingerprintable manifest)",
    { skip: process.platform === "win32" },
    async () => {
      const root = await withRoot();
      await writeFixtureFiles(root.root, { "a.txt": "x" });
      execFileSync("mkfifo", [path.join(root.root, "pipe")]);
      const outcome = await createLocalDirectoryResolver().resolveIdentity(
        { kind: "local-directory", path: root.root },
        { allowMutableRefs: false },
      );
      expect(outcome).toMatchObject({
        status: "failed",
        reason: new RegExp("special file"),
      });
    },
  );

  it("fails when a file cannot be read", { skip: process.platform === "win32" }, async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "x" });
    await chmod(path.join(root.root, "a.txt"), 0o000);
    try {
      const outcome = await createLocalDirectoryResolver().resolveIdentity(
        { kind: "local-directory", path: root.root },
        { allowMutableRefs: false },
      );
      expect(outcome.status).toBe("failed");
    } finally {
      await chmod(path.join(root.root, "a.txt"), 0o644);
    }
  });

  it("reports unavailable for a missing path", async () => {
    const root = await withRoot();
    const outcome = await createLocalDirectoryResolver().resolveIdentity(
      { kind: "local-directory", path: path.join(root.root, "missing") },
      { allowMutableRefs: false },
    );
    expect(outcome).toMatchObject({
      status: "unavailable",
      reason: new RegExp("cannot be resolved"),
    });
  });

  it("fails for a non-directory path", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "x" });
    const outcome = await createLocalDirectoryResolver().resolveIdentity(
      { kind: "local-directory", path: path.join(root.root, "a.txt") },
      { allowMutableRefs: false },
    );
    expect(outcome).toMatchObject({
      status: "failed",
      reason: "Reference path is not a directory.",
    });
  });

  it("reports unavailable for non-local sources", async () => {
    const outcome = await createLocalDirectoryResolver().resolveIdentity(
      { kind: "repository", repository: "https://github.com/o/r", ref: { kind: "tag", tag: "v1" } },
      { allowMutableRefs: false },
    );
    expect(outcome).toMatchObject({ status: "unavailable" });
  });
});

describe("createRepositoryResolver + fake backend", () => {
  const ORIGIN = "https://github.com/owner/repo";
  const COMMIT = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";
  const fixture: FakeRepositoryFixture = {
    [ORIGIN]: {
      commits: { [COMMIT]: { "a.txt": "hello" } },
      tags: { v1: COMMIT },
      branches: { main: COMMIT },
    },
  };

  function resolve(ref: RepositoryRef, allowMutableRefs = false) {
    const resolver = createRepositoryResolver(createFakeRepositoryBackend(fixture));
    return resolver.resolveIdentity(
      { kind: "repository", repository: ORIGIN, ref },
      { allowMutableRefs },
    );
  }

  it("resolves a pinned commit", async () => {
    const outcome = await resolve({ kind: "commit", commit: COMMIT });
    expect(outcome).toEqual({
      status: "resolved",
      identity: {
        kind: "repository",
        origin: ORIGIN,
        commit: COMMIT,
        requestedRef: { kind: "commit", commit: COMMIT },
      },
    });
  });

  it("resolves a tag to its commit", async () => {
    const outcome = await resolve({ kind: "tag", tag: "v1" });
    expect(outcome.status).toBe("resolved");
    if (outcome.status === "resolved") {
      expect(outcome.identity).toMatchObject({
        kind: "repository",
        origin: ORIGIN,
        commit: COMMIT,
      });
      expect(outcome.identity).toMatchObject({
        kind: "repository",
        requestedRef: { kind: "tag", tag: "v1" },
      });
    }
  });

  it("resolves a branch only when mutable refs are allowed", async () => {
    const refused = await resolve({ kind: "branch", branch: "main" }, false);
    expect(refused).toEqual({ status: "refused", reason: MUTABLE_REF_REFUSAL });
    const resolved = await resolve({ kind: "branch", branch: "main" }, true);
    expect(resolved.status).toBe("resolved");
    if (resolved.status === "resolved") {
      expect(resolved.identity).toMatchObject({ kind: "repository", commit: COMMIT });
    }
  });

  it("fails on unknown commit, tag, branch, and origin", async () => {
    expect(
      await resolve({ kind: "commit", commit: "ffffffffffffffffffffffffffffffffffffffff" }),
    ).toMatchObject({
      status: "failed",
      reason: new RegExp("Unknown commit"),
    });
    expect(await resolve({ kind: "tag", tag: "nope" })).toMatchObject({
      status: "failed",
      reason: new RegExp("Unknown tag"),
    });
    expect(await resolve({ kind: "branch", branch: "nope" }, true)).toMatchObject({
      status: "failed",
      reason: new RegExp("Unknown branch"),
    });
    const originResolver = createRepositoryResolver(createFakeRepositoryBackend(fixture));
    const unknownOrigin = await originResolver.resolveIdentity(
      {
        kind: "repository",
        repository: "https://github.com/other/repo",
        ref: { kind: "tag", tag: "v1" },
      },
      { allowMutableRefs: false },
    );
    expect(unknownOrigin).toMatchObject({
      status: "failed",
      reason: new RegExp("Unknown repository origin"),
    });
  });

  it("fails on malformed commit shas", async () => {
    expect(await resolve({ kind: "commit", commit: "zzz" })).toMatchObject({
      status: "failed",
      reason: new RegExp("Malformed commit"),
    });
  });

  it("rejects non-repository sources", async () => {
    const resolver = createRepositoryResolver(createFakeRepositoryBackend(fixture));
    const outcome = await resolver.resolveIdentity(LOCAL_SOURCE, { allowMutableRefs: false });
    expect(outcome).toMatchObject({ status: "unavailable" });
  });
});

describe("createUnavailableRepositoryBackend", () => {
  it("always reports unavailable with the configured reason and touches nothing", async () => {
    const backend = createUnavailableRepositoryBackend("custom reason");
    const outcome = await backend.resolveCommit(
      "https://github.com/o/r",
      { kind: "tag", tag: "v1" },
      { allowMutableRefs: true },
    );
    expect(outcome).toEqual({ status: "unavailable", reason: "custom reason" });
    const defaulted = await createUnavailableRepositoryBackend().resolveCommit(
      "https://github.com/o/r",
      { kind: "commit", commit: "a1b2c3d" },
      { allowMutableRefs: false },
    );
    expect(defaulted).toMatchObject({
      status: "unavailable",
      reason: new RegExp("sandboxed git"),
    });
  });
});

describe("createReferenceResolver dispatch", () => {
  it("fails closed when the side for a source kind is missing", async () => {
    const localOnly = createReferenceResolver({ local: createLocalDirectoryResolver() });
    const repositoryOnly = createReferenceResolver({
      repository: createRepositoryResolver(createUnavailableRepositoryBackend()),
    });
    const repositoryOutcome = await localOnly.resolveIdentity(
      {
        kind: "repository",
        repository: "https://github.com/o/r",
        ref: { kind: "tag", tag: "v1" },
      },
      { allowMutableRefs: false },
    );
    expect(repositoryOutcome).toMatchObject({
      status: "unavailable",
      reason: new RegExp("Repository resolution is not configured"),
    });
    const localOutcome = await repositoryOnly.resolveIdentity(LOCAL_SOURCE, {
      allowMutableRefs: false,
    });
    expect(localOutcome).toMatchObject({
      status: "unavailable",
      reason: new RegExp("Local-directory resolution is not configured"),
    });
  });

  it("dispatches by source kind", async () => {
    const root = await withRoot();
    await writeFixtureFiles(root.root, { "a.txt": "x" });
    const resolver = createReferenceResolver({
      local: createLocalDirectoryResolver(),
      repository: createRepositoryResolver(createFakeRepositoryBackend({})),
    });
    const local = await resolver.resolveIdentity(
      { kind: "local-directory", path: root.root },
      { allowMutableRefs: false },
    );
    expect(local.status).toBe("resolved");
    const repository = await resolver.resolveIdentity(
      { kind: "repository", repository: "https://github.com/o/r", ref: { kind: "tag", tag: "v1" } },
      { allowMutableRefs: false },
    );
    expect(repository).toMatchObject({
      status: "failed",
      reason: new RegExp("Unknown repository origin"),
    });
  });
});
