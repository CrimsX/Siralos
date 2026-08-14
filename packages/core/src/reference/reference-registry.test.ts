import { describe, expect, it } from "vitest";
import type { ReferenceDeclaration } from "./reference-declaration.js";
import type { ReferenceResolutionOutcome, ReferenceResolverPort } from "./reference-ports.js";
import { createReferenceRegistry, isPathWithin } from "./reference-registry.js";
import type { ReferenceAlias, ReferenceSource } from "./reference-model.js";
import { createReferenceId, referenceIdOf } from "./reference-model.js";

/**
 * Deterministic fake resolver: a scripted list of outcomes per source.
 * Records every invocation so tests can assert the resolver is (or is not)
 * called.
 */
function fakeResolver(
  script: ReadonlyArray<{ readonly source: string; readonly outcome: ReferenceResolutionOutcome }>,
  calls: ReferenceSource[] = [],
): ReferenceResolverPort {
  return {
    resolveIdentity(source, _options): Promise<ReferenceResolutionOutcome> {
      calls.push(source);
      const entry = script.find((item) => item.source === sourceLabel(source));
      if (entry !== undefined) {
        return Promise.resolve(entry.outcome);
      }
      // Default: resolve local directories outside the workspace and
      // repositories to a deterministic commit.
      if (source.kind === "local-directory") {
        return Promise.resolve({
          status: "resolved",
          identity: {
            kind: "local-directory",
            canonicalPath: source.path,
            fingerprint: "fp-local-1",
          },
        });
      }
      return Promise.resolve({
        status: "resolved",
        identity: {
          kind: "repository",
          origin: source.repository,
          commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0",
          requestedRef: source.ref,
        },
      });
    },
  };
}

function sourceLabel(source: ReferenceSource): string {
  return source.kind === "local-directory" ? source.path : source.repository;
}

function localDeclaration(path: string, alias = `dir-${path.length}`): ReferenceDeclaration {
  return {
    alias,
    kind: "local-directory",
    source: { kind: "local-directory", path },
    description: null,
  };
}

function repositoryDeclaration(
  alias: string,
  ref: { kind: "commit" | "tag" | "branch"; commit?: string; tag?: string; branch?: string },
): ReferenceDeclaration {
  return {
    alias,
    kind: "repository",
    source: {
      kind: "repository",
      repository: "https://github.com/godotengine/godot",
      ref:
        ref.kind === "commit"
          ? { kind: "commit", commit: ref.commit ?? "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0" }
          : ref.kind === "tag"
            ? { kind: "tag", tag: ref.tag ?? "4.3-stable" }
            : { kind: "branch", branch: ref.branch ?? "main" },
    },
    description: null,
  };
}

const WORKSPACE_ROOT = "/home/user/project";

describe("isPathWithin (pure containment)", () => {
  it("detects containment with separator boundaries", () => {
    expect(isPathWithin("/home/user/project", "/home/user/project/sub")).toBe(true);
    expect(isPathWithin("/home/user/project", "/home/user/project")).toBe(true);
    expect(isPathWithin("/home/user/project", "/home/user/project2")).toBe(false);
    expect(isPathWithin("/home/user/project", "/home/user/other")).toBe(false);
    expect(isPathWithin("/home/user/project", "/home/user/project/../escape")).toBe(false);
  });

  it("handles Windows-style paths case-insensitively", () => {
    expect(isPathWithin("C:\\Users\\TestUser\\project", "C:\\Users\\TestUser\\project\\sub")).toBe(
      true,
    );
    expect(isPathWithin("c:\\users\\testuser\\project", "C:\\Users\\TestUser\\PROJECT\\sub")).toBe(
      true,
    );
    expect(isPathWithin("C:\\Users\\TestUser\\project", "C:\\Users\\TestUser\\project2")).toBe(
      false,
    );
    expect(isPathWithin("C:\\Users\\TestUser\\project", "D:\\Users\\TestUser\\project\\sub")).toBe(
      false,
    );
  });

  it("normalizes trailing slashes and mixed separators", () => {
    expect(isPathWithin("/home/user/project/", "/home/user/project/sub")).toBe(true);
    expect(isPathWithin("/home/user/project", "/home/user/project/sub/")).toBe(true);
  });
});

describe("createReferenceRegistry", () => {
  it("resolves local-directory and repository declarations and lists them in order", async () => {
    const declarations = [
      localDeclaration("/srv/shared-assets", "assets"),
      repositoryDeclaration("docs", { kind: "commit" }),
    ];
    const registry = await createReferenceRegistry({
      declarations,
      trustFor: (declaration) =>
        declaration.kind === "local-directory" ? "explicit-user" : "untrusted-project",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
    });
    expect(registry.size).toBe(2);
    const listed = registry.list();
    expect(listed.map((reference) => reference.alias)).toEqual(["assets", "docs"]);
    expect(listed[0]?.status).toBe("ready");
    expect(listed[0]?.trust).toBe("explicit-user");
    expect(listed[1]?.trust).toBe("untrusted-project");
    expect(registry.revision("assets" as ReferenceAlias)).not.toBeNull();
  });

  it("declines duplicate aliases with a precise reason (first occurrence wins)", async () => {
    const declarations = [localDeclaration("/srv/a", "same"), localDeclaration("/srv/b", "same")];
    const registry = await createReferenceRegistry({
      declarations,
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
    });
    const first = registry.get("same" as ReferenceAlias);
    expect(first?.status).toBe("ready");
    expect(registry.declineReason("same" as ReferenceAlias)).toBeNull();
    // Both aliases resolve to the same id; the second record is declined.
    const listed = registry.list();
    const declined = listed.filter((reference) => reference.status === "declined");
    expect(declined).toHaveLength(1);
    expect(declined[0]?.failureReason).toBe("duplicate alias");
  });

  it("refuses a mutable branch ref without allowMutableRefs and never calls the resolver", async () => {
    const calls: ReferenceSource[] = [];
    const registry = await createReferenceRegistry({
      declarations: [repositoryDeclaration("docs", { kind: "branch" })],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([], calls),
    });
    const reference = registry.get("docs" as ReferenceAlias);
    expect(reference?.status).toBe("declined");
    expect(reference?.failureReason).toBe(
      "mutable repository ref requires an explicit pinned commit/tag",
    );
    expect(registry.revision("docs" as ReferenceAlias)).toBeNull();
    expect(calls).toHaveLength(0); // registry-level refusal, resolver untouched
    expect(registry.declineReason("docs" as ReferenceAlias)).toBe(reference?.failureReason);
  });

  it("resolves mutable refs when allowMutableRefs is true", async () => {
    const registry = await createReferenceRegistry({
      declarations: [repositoryDeclaration("docs", { kind: "branch", branch: "main" })],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
      allowMutableRefs: true,
    });
    expect(registry.get("docs" as ReferenceAlias)?.status).toBe("ready");
    const revision = registry.revision("docs" as ReferenceAlias);
    expect(revision?.identity.kind).toBe("repository");
  });

  it("refuses local-directory references inside the workspace namespace", async () => {
    const inside = await createReferenceRegistry({
      declarations: [localDeclaration("/home/user/project/vendor", "vendor")],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
    });
    expect(inside.get("vendor" as ReferenceAlias)?.status).toBe("declined");
    expect(inside.get("vendor" as ReferenceAlias)?.failureReason).toBe(
      "reference root must be outside the workspace namespace",
    );
  });

  it("keeps resolution-failed references listed with status and reason", async () => {
    const registry = await createReferenceRegistry({
      declarations: [
        localDeclaration("/srv/missing", "missing"),
        repositoryDeclaration("gone", { kind: "commit", commit: "deadbeef" }),
      ],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([
        {
          source: "/srv/missing",
          outcome: { status: "failed", reason: "directory does not exist" },
        },
        {
          source: "https://github.com/godotengine/godot",
          outcome: { status: "unavailable", reason: "network unavailable" },
        },
      ]),
    });
    expect(registry.get("missing" as ReferenceAlias)?.status).toBe("resolution-failed");
    expect(registry.get("missing" as ReferenceAlias)?.failureReason).toBe(
      "directory does not exist",
    );
    expect(registry.get("gone" as ReferenceAlias)?.status).toBe("unavailable");
    expect(registry.get("gone" as ReferenceAlias)?.failureReason).toBe("network unavailable");
    expect(registry.revision("missing" as ReferenceAlias)).toBeNull();
  });

  it("stores resolver refusals as declined with the reason", async () => {
    const registry = await createReferenceRegistry({
      declarations: [repositoryDeclaration("denied", { kind: "commit", commit: "deadbeef" })],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([
        {
          source: "https://github.com/godotengine/godot",
          outcome: { status: "refused", reason: "repository policy refuses access" },
        },
      ]),
    });
    expect(registry.get("denied" as ReferenceAlias)?.status).toBe("declined");
    expect(registry.get("denied" as ReferenceAlias)?.failureReason).toBe(
      "repository policy refuses access",
    );
  });

  it("keeps revisions immutable and unchanged until refresh (no silent branch advance)", async () => {
    let commit = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";
    const calls: ReferenceSource[] = [];
    const resolver: ReferenceResolverPort = {
      resolveIdentity(source, _options): Promise<ReferenceResolutionOutcome> {
        calls.push(source);
        return Promise.resolve({
          status: "resolved",
          identity: {
            kind: "repository",
            origin: "https://github.com/godotengine/godot",
            commit,
            requestedRef: { kind: "commit", commit },
          },
        });
      },
    };
    const registry = await createReferenceRegistry({
      declarations: [repositoryDeclaration("docs", { kind: "commit" })],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver,
      allowMutableRefs: true,
    });
    const before = registry.revision("docs" as ReferenceAlias);
    // The "remote" advances; the registry still serves the old revision
    // until refresh() is called explicitly.
    commit = "f0e1d2c3b4a5968778695a4b3c2d1e0f";
    expect(registry.revision("docs" as ReferenceAlias)?.identity).toEqual(before?.identity);
    const refreshed = await registry.refresh("docs" as ReferenceAlias);
    expect(refreshed.status).toBe("refreshed");
    if (refreshed.status === "refreshed") {
      expect(refreshed.revision.identity).toMatchObject({ kind: "repository", commit });
    }
    expect(registry.revision("docs" as ReferenceAlias)?.identity).toMatchObject({
      kind: "repository",
      commit,
    });
    // The old revision object is untouched (immutable value).
    expect(before?.identity).toMatchObject({
      kind: "repository",
      commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0",
    });
  });

  it("refresh reports unchanged when the identity did not move", async () => {
    const registry = await createReferenceRegistry({
      declarations: [localDeclaration("/srv/stable", "stable")],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
    });
    const result = await registry.refresh("stable" as ReferenceAlias);
    expect(result.status).toBe("unchanged");
    if (result.status === "unchanged") {
      expect(result.revision.identity).toEqual(
        registry.revision("stable" as ReferenceAlias)?.identity,
      );
    }
  });

  it("a failed refresh invalidates the current revision and fails closed", async () => {
    let fail = false;
    const resolver: ReferenceResolverPort = {
      resolveIdentity(_source, _options): Promise<ReferenceResolutionOutcome> {
        if (fail) {
          return Promise.resolve({ status: "failed", reason: "remote went away" });
        }
        return Promise.resolve({
          status: "resolved",
          identity: { kind: "local-directory", canonicalPath: "/srv/stable", fingerprint: "fp-1" },
        });
      },
    };
    const registry = await createReferenceRegistry({
      declarations: [localDeclaration("/srv/stable", "stable")],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver,
    });
    expect(registry.revision("stable" as ReferenceAlias)).not.toBeNull();
    fail = true;
    const result = await registry.refresh("stable" as ReferenceAlias);
    expect(result.status).toBe("failed");
    expect(registry.revision("stable" as ReferenceAlias)).toBeNull();
    expect(registry.get("stable" as ReferenceAlias)?.status).toBe("resolution-failed");
  });

  it("bindTask captures an immutable snapshot that outlives refreshes", async () => {
    let commit = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";
    const resolver: ReferenceResolverPort = {
      resolveIdentity(_source, _options): Promise<ReferenceResolutionOutcome> {
        return Promise.resolve({
          status: "resolved",
          identity: {
            kind: "repository",
            origin: "https://github.com/godotengine/godot",
            commit,
            requestedRef: { kind: "commit", commit },
          },
        });
      },
    };
    const registry = await createReferenceRegistry({
      declarations: [
        repositoryDeclaration("docs", { kind: "commit" }),
        localDeclaration("/srv/assets", "assets"),
      ],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver,
      allowMutableRefs: true,
    });
    const binding = registry.bindTask("task-1");
    expect(binding.revisions.size).toBe(2);
    const boundCommit = binding.revisions.get(referenceIdOf("docs" as ReferenceAlias));
    expect(boundCommit?.identity).toMatchObject({ kind: "repository", commit });
    commit = "f0e1d2c3b4a5968778695a4b3c2d1e0f";
    await registry.refresh("docs" as ReferenceAlias);
    expect(registry.revision("docs" as ReferenceAlias)?.identity).toMatchObject({
      kind: "repository",
      commit,
    });
    // The binding still holds the task-start revision.
    expect(binding.revisions.get(referenceIdOf("docs" as ReferenceAlias))?.identity).toMatchObject({
      kind: "repository",
      commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0",
    });
    expect(registry.boundRevision(binding, "docs" as ReferenceAlias)?.identity).toMatchObject({
      kind: "repository",
      commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0",
    });
    expect(registry.boundRevision(binding, createReferenceId("docs"))?.identity).toMatchObject({
      kind: "repository",
      commit: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0",
    });
    expect(registry.boundRevision(binding, "unknown" as ReferenceAlias)).toBeNull();
  });

  it("bindings are FIFO-bounded by maxRevisionBindings", async () => {
    const registry = await createReferenceRegistry({
      declarations: [localDeclaration("/srv/a", "aa")],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
      limits: { maxRevisionBindings: 3 },
    });
    const first = registry.bindTask("task-1");
    registry.bindTask("task-2");
    registry.bindTask("task-3");
    registry.bindTask("task-4");
    // The oldest binding was evicted.
    expect(registry.boundRevision(first, "a" as ReferenceAlias)).toBeNull();
  });

  it("bindTask only snapshots ready references", async () => {
    const registry = await createReferenceRegistry({
      declarations: [
        localDeclaration("/srv/ok", "ok"),
        repositoryDeclaration("bad", { kind: "branch" }),
      ],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
    });
    const binding = registry.bindTask("task-1");
    expect(binding.revisions.size).toBe(1);
    expect(binding.revisions.has(referenceIdOf("ok" as ReferenceAlias))).toBe(true);
    expect(binding.revisions.has(referenceIdOf("bad" as ReferenceAlias))).toBe(false);
  });

  it("refresh on a declined reference refuses without re-resolving", async () => {
    const calls: ReferenceSource[] = [];
    const registry = await createReferenceRegistry({
      declarations: [repositoryDeclaration("bad", { kind: "branch" })],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([], calls),
    });
    const result = await registry.refresh("bad" as ReferenceAlias);
    expect(result.status).toBe("refused");
    expect(calls).toHaveLength(0);
  });

  it("resolves declarations in parallel but records outcomes in declaration order", async () => {
    const declarations = Array.from({ length: 6 }, (_, index) =>
      localDeclaration(`/srv/dir-${index}`, `ref${index}`),
    );
    const registry = await createReferenceRegistry({
      declarations,
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
    });
    expect(registry.list().map((reference) => reference.alias)).toEqual([
      "ref0",
      "ref1",
      "ref2",
      "ref3",
      "ref4",
      "ref5",
    ]);
  });

  it("get() accepts both aliases and ids", async () => {
    const registry = await createReferenceRegistry({
      declarations: [localDeclaration("/srv/a", "assets")],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
    });
    const id = referenceIdOf("assets" as ReferenceAlias);
    expect(registry.get(id)?.alias).toBe("assets");
    expect(registry.get("assets" as ReferenceAlias)?.id).toBe(id);
    expect(registry.get("nope" as ReferenceAlias)).toBeUndefined();
    expect(registry.revision("nope" as ReferenceAlias)).toBeNull();
  });

  it("resolution revisions carry resolvedAtMs from the injected clock", async () => {
    let tick = 1000;
    const registry = await createReferenceRegistry({
      declarations: [localDeclaration("/srv/a", "aa")],
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
      now: () => {
        tick += 1;
        return tick;
      },
    });
    const revision = registry.revision("aa" as ReferenceAlias);
    expect(revision?.resolvedAtMs).toBe(1001);
  });

  it("declines invalid aliases defensively", async () => {
    const declarations: ReferenceDeclaration[] = [
      {
        alias: "Bad Alias!",
        kind: "local-directory",
        source: { kind: "local-directory", path: "/srv/a" },
        description: null,
      },
    ];
    const registry = await createReferenceRegistry({
      declarations,
      trustFor: () => "explicit-user",
      workspaceRoot: WORKSPACE_ROOT,
      resolver: fakeResolver([]),
    });
    expect(registry.list()[0]?.status).toBe("declined");
    expect(registry.list()[0]?.failureReason).toBe("invalid alias");
  });
});

describe("reference id derivation", () => {
  it("mints deterministic ref_ ids from the alias", () => {
    const id = referenceIdOf("docs" as ReferenceAlias);
    expect(id.startsWith("ref_")).toBe(true);
    expect(id.length).toBe("ref_".length + 24);
    expect(referenceIdOf("docs" as ReferenceAlias)).toBe(id);
    expect(referenceIdOf("other" as ReferenceAlias)).not.toBe(id);
  });
});
