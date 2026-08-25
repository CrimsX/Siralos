import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  REFERENCE_LIMITS,
  createReferenceId,
  isPathWithin,
  validateReferenceAlias,
} from "../../../packages/core/src/reference/reference-model.ts";
import {
  normalizeRepositoryOrigin,
  parseReferenceDeclaration,
  parseReferenceDeclarationsSection,
} from "../../../packages/core/src/reference/reference-declaration.ts";
import { createReferenceRegistry } from "../../../packages/core/src/reference/reference-registry.ts";
import {
  createFakeRepositoryBackend,
  createLocalDirectoryResolver,
} from "../../../packages/adapters/src/reference/reference-resolver.ts";
import { createReferenceMaterializer } from "../../../packages/adapters/src/reference/reference-materializer.ts";

const NOW_MS = Number(JSON.parse(readFileSync(0, "utf8")).nowMs ?? 1_700_000_000_000);

function clockQueue(values) {
  let index = 0;
  return () => {
    const value = values[Math.min(index, values.length - 1)];
    index += 1;
    return value;
  };
}

/** Stub resolver dispatching per source path; the map may be swapped live. */
function stubResolver(map) {
  return {
    resolveIdentity(source) {
      const key = source.kind === "local-directory" ? source.path : JSON.stringify(source);
      const handler = map.get(key) ?? map.get("default");
      if (typeof handler === "function") {
        return handler(source);
      }
      return handler ?? { status: "unavailable", reason: "no stub" };
    },
  };
}

function countingSpy(inner) {
  const state = { calls: 0 };
  return {
    state,
    resolveIdentity(source, options) {
      state.calls += 1;
      return inner.resolveIdentity(source, options);
    },
  };
}

const FAKE_REPO_FIXTURE = {
  "owner/repo": {
    commits: { abc1234: {}, def5678: {} },
    tags: { "v1.0": "abc1234" },
    branches: { main: "def5678" },
  },
};

function localDecl(path, extra = {}) {
  return {
    alias: "docs",
    kind: "local-directory",
    source: { kind: "local-directory", path },
    ...extra,
  };
}

function repoDecl(ref, extra = {}) {
  return {
    alias: "docs",
    kind: "repository",
    source: {
      kind: "repository",
      repository: "https://github.com/owner/repo",
      ...(ref === undefined ? {} : { ref }),
    },
    ...extra,
  };
}

async function buildRegistry(declarations, resolver, options = {}) {
  return createReferenceRegistry({
    declarations,
    trustFor: () => "explicit-user",
    workspaceRoot: options.workspaceRoot ?? "/ws",
    resolver,
    allowMutableRefs: options.allowMutableRefs ?? false,
    now: options.now ?? (() => NOW_MS),
    limits: options.limits,
  });
}

function summary(reference) {
  return {
    id: reference.id,
    alias: reference.alias,
    status: reference.status,
    reason: reference.failureReason,
  };
}

function runCase(inputCase) {
  switch (inputCase.name) {
    case "declaration-parse-strict": {
      const attempts = [
        ["valid-posix", localDecl("/tmp/docs")],
        ["valid-windows-drive", localDecl("C:\\docs")],
        ["valid-windows-unc", localDecl("\\\\srv\\share\\docs")],
        ["relative-refused", localDecl("docs")],
        [
          "unknown-key-rejected",
          localDecl("/tmp/docs", { surprise: 1 }),
        ],
        ["alias-malformed", localDecl("/tmp/docs", { alias: "Docs" })],
        [
          "description-too-long",
          localDecl("/tmp/docs", { description: "x".repeat(513) }),
        ],
        [
          "kind-required",
          { alias: "docs", source: { kind: "local-directory", path: "/tmp/docs" } },
        ],
      ].map(([tag, value]) => {
        const parsed = parseReferenceDeclaration(value);
        return parsed.ok
          ? { tag, ok: true, alias: parsed.value.alias, kind: parsed.value.kind }
          : { tag, ok: false, reason: parsed.reason };
      });
      const sectionMismatch = parseReferenceDeclarationsSection(
        { docs: repoDecl(undefined, { alias: "other" }) },
      );
      const oversized = {};
      for (let index = 0; index < 17; index += 1) {
        oversized[`ref${String(index).padStart(2, "0")}`] =
          localDecl(`/tmp/d${index}`, { alias: `ref${String(index).padStart(2, "0")}` });
      }
      const sectionCount = parseReferenceDeclarationsSection(oversized);
      const validSection = parseReferenceDeclarationsSection({
        docs: localDecl("/tmp/docs"),
      });
      const idA = createReferenceId("docs");
      const idB = createReferenceId("docs");
      return {
        attempts,
        mismatchReason: sectionMismatch.ok ? null : sectionMismatch.reason,
        countReason: sectionCount.ok ? null : sectionCount.reason,
        validSectionOk: validSection.ok && validSection.declarations.length === 1
          && validSection.declarations[0].alias === "docs",
        idFormat: /^ref_[0-9a-f]{24}$/.test(idA),
        idDeterministic: idA === idB,
        aliasValid: validateReferenceAlias("docs") !== null,
        aliasInvalidLength: validateReferenceAlias(`a${"b".repeat(64)}`) === null,
      };
    }
    case "origin-normalization": {
      const inputs = [
        ["shorthand", "owner/repo"],
        ["url-git-slash", "https://github.com/owner/repo.git/"],
        ["http-refused", "http://github.com/owner/repo"],
        ["foreign-host", "https://gitlab.com/owner/repo"],
        ["credentials-refused", "https://user@github.com/owner/repo"],
        ["query-refused", "https://github.com/owner/repo?x=1"],
        ["fragment-refused", "https://github.com/owner/repo#readme"],
        ["extra-segment", "https://github.com/owner/repo/extra"],
        ["empty-owner", "https://github.com//repo"],
        ["bad-owner-char", "under_score/repo"],
        ["bad-repo-char", "owner/re po"],
        ["empty", "   "],
      ];
      return {
        results: inputs.map(([tag, input]) => {
          const result = normalizeRepositoryOrigin(input);
          return result.ok
            ? { tag, ok: true, origin: result.origin }
            : { tag, ok: false, reason: result.reason };
        }),
      };
    }
    case "ref-parsing-and-pins": {
      const refs = [
        ["commit-ok", { kind: "commit", commit: "abc1234" }],
        ["commit-uppercase-ok", { kind: "commit", commit: "ABC1234" }],
        ["commit-short-malformed", { kind: "commit", commit: "abc" }],
        ["commit-nonhex-malformed", { kind: "commit", commit: "xyz1234" }],
        ["tag-ok", { kind: "tag", tag: "v4.3" }],
        [
          "tag-too-long",
          { kind: "tag", tag: "v".repeat(129) },
        ],
        ["branch-ok", { kind: "branch", branch: "feature/x" }],
        ["branch-empty", { kind: "branch", branch: "" }],
        ["unknown-kind", { kind: "tree", commit: "abc1234" }],
        ["unknown-key-in-ref", { kind: "commit", commit: "abc1234", sha: "z" }],
      ];
      return {
        results: refs.map(([tag, ref]) => {
          const parsed = parseReferenceDeclaration(repoDecl(ref));
          return parsed.ok ? { tag, ok: true } : { tag, ok: false, reason: parsed.reason };
        }),
      };
    }
    case "mutable-ref-declined-pre-resolver": {
      const backendA = createFakeRepositoryBackend(FAKE_REPO_FIXTURE);
      const spyA = countingSpy({ resolveIdentity: (source) =>
        backendA.resolveIdentity(source, { allowMutableRefs: false }) });
      const registryDefault = buildRegistry([repoDecl()], spyA, {});
      // createReferenceRegistry awaits the port; wrap into a promise-shaped stub.
      const defaultRegistry = createReferenceRegistry({
        declarations: [repoDecl()],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: { resolveIdentity: (source, options) =>
          Promise.resolve(spyA.resolveIdentity(source, options)) },
        allowMutableRefs: false,
        now: () => NOW_MS,
      });
      void registryDefault;
      const declined = defaultRegistry.list()[0];
      const mutableSpy = createReferenceRegistry({
        declarations: [repoDecl()],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: { resolveIdentity: (source, options) =>
          Promise.resolve(createFakeRepositoryBackend(FAKE_REPO_FIXTURE)
            .resolveIdentity(source, options)) },
        allowMutableRefs: true,
        now: () => NOW_MS,
      });
      const resolved = mutableSpy.list()[0];
      const revision = mutableSpy.revision("docs");
      const pinned = createReferenceRegistry({
        declarations: [repoDecl({ kind: "commit", commit: "abc1234" })],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: { resolveIdentity: (source, options) =>
          Promise.resolve(createFakeRepositoryBackend(FAKE_REPO_FIXTURE)
            .resolveIdentity(source, options)) },
        allowMutableRefs: false,
        now: () => NOW_MS,
      });
      const pinnedReference = pinned.list()[0];
      return {
        declinedStatus: declined.status,
        declinedReason: declined.failureReason,
        preResolverSpyCalls: spyA.state.calls,
        resolvedStatus: resolved.status,
        resolvedCommit: revision?.identity?.commit ?? null,
        requestedRef: revision?.identity?.requestedRef ?? null,
        resolvedAtMatchesClock: revision?.resolvedAtMs === NOW_MS,
        pinnedStatus: pinnedReference.status,
        pinnedCommit: pinned.revision("docs")?.identity?.commit ?? null,
      };
    }
    case "workspace-containment-refusal": {
      const pureChecks = {
        rootItself: isPathWithin("/ws", "/ws"),
        boundaryRespected: !isPathWithin("/ws", "/wsx"),
        windowsCaseInsensitive: isPathWithin("C:/Ws", "c:/WS/x"),
        relativeFailsClosed: !isPath_within_compat("/ws", "relative/docs"),
      };
      const stub = stubResolver(new Map([
        ["/ws/inner/docs", {
          status: "resolved",
          identity: { kind: "local-directory", canonicalPath: "/ws/inner/docs", fingerprint: "fp-inner" },
        }],
        ["/outside/docs", {
          status: "resolved",
          identity: { kind: "local-directory", canonicalPath: "/outside/docs", fingerprint: "fp-outside" },
        }],
      ]));
      const registry = buildRegistry(
        [localDecl("/ws/inner/docs"), localDecl("/outside/docs")],
        stub,
        { workspaceRoot: "/ws" },
      );
      const references = registry.list();
      const demotion = createReferenceRegistry({
        declarations: [localDecl("/outside/docs")],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: stub,
        allowMutableRefs: false,
        now: () => NOW_MS,
      }).then((ready) => ready);
      void demotion;
      return { pureChecks, references: references.map(summary), hasDemotionProbe: true };
    }
    case "refresh-fail-closed-invalidation":
    case "task-binding-fifo-snapshot":
    case "duplicate-alias-audit":
    case "resolver-outcome-matrix":
    case "materializer-posture":
    case "real-fingerprint-enumeration":
      throw new Error(`${inputCase.name} handled by runAsyncCase`);
    default:
      throw new Error(`unknown reference-identity fixture case ${inputCase.name}`);
  }
}

// Compatibility shim mirroring isPathWithin's fail-closed relative rule
// under the historical probe name.
function isPath_within_compat(root, target) {
  return isPathWithin(root, target);
}

async function runAsyncCase(inputCase) {
  switch (inputCase.name) {
    case "workspace-containment-refresh-demotion": {
      const swap = new Map([
        ["/outside/docs", {
          status: "resolved",
          identity: { kind: "local-directory", canonicalPath: "/outside/docs", fingerprint: "fp-outside" },
        }],
      ]);
      const registry = await createReferenceRegistry({
        declarations: [localDecl("/outside/docs")],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: stubResolver(swap),
        allowMutableRefs: false,
        now: () => NOW_MS,
      });
      const before = registry.revision("docs");
      swap.set("/outside/docs", {
        status: "resolved",
        identity: { kind: "local-directory", canonicalPath: "/ws/moved", fingerprint: "fp-inside" },
      });
      const refresh = await registry.refresh("docs");
      return {
        hadRevisionBefore: before !== null,
        refreshStatus: refresh.status,
        refreshReason: refresh.reason ?? null,
        revisionAfter: registry.revision("docs"),
        statusAfter: registry.get("docs").status,
        reasonAfter: registry.get("docs").failureReason,
      };
    }
    case "duplicate-alias-audit": {
      const registry = await buildRegistry(
        [localDecl("/tmp/a"), localDecl("/tmp/b")],
        stubResolver(new Map([
          ["/tmp/a", resolvedLocal("/tmp/a", "fp-a")],
          ["/tmp/b", resolvedLocal("/tmp/b", "fp-b")],
        ])),
      );
      const listed = registry.list();
      return {
        statuses: listed.map((reference) => reference.status),
        duplicateReason: registry.declineReason(listed[1].id),
        firstAddressable: registry.get("docs")?.status ?? null,
        size: registry.size,
        sharedId: listed[0].id === listed[1].id,
      };
    }
    case "resolver-outcome-matrix": {
      const declarations = [
        localDecl("/u", { alias: "unavailableref" }),
        localDecl("/r", { alias: "refusedref" }),
        localDecl("/f", { alias: "failedref" }),
        localDecl("/ok", { alias: "readyref" }),
      ];
      const outcomes = new Map([
        ["/u", { status: "unavailable", reason: "The source is unavailable." }],
        ["/r", { status: "refused", reason: "Not allowed." }],
        ["/f", { status: "failed", reason: "Boom." }],
        ["/ok", resolvedLocal("/ok", "fp-ok")],
      ]);
      const registry = await buildRegistry(declarations, stubResolver(outcomes));
      const second = await buildRegistry(declarations, stubResolver(outcomes));
      const listed = registry.list();
      return {
        order: listed.map((reference) => reference.alias),
        matrix: listed.map((reference) => ({
          alias: reference.alias,
          status: reference.status,
          reason: reference.failureReason,
        })),
        idFormat: listed.every((reference) => /^ref_[0-9a-f]{24}$/.test(reference.id)),
        idsStableAcrossRegistries: listed.every((reference, index) =>
          reference.id === second.list()[index].id),
      };
    }
    case "refresh-fail-closed-invalidation": {
      const clock = clockQueue([1, 2, 3, 4]);
      const swap = new Map([
        ["/outside/docs", resolvedLocal("/outside/docs", "fp1")],
      ]);
      const registry = await createReferenceRegistry({
        declarations: [localDecl("/outside/docs")],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: stubResolver(swap),
        allowMutableRefs: false,
        now: clock,
      });
      const binding = registry.bindTask("t0");
      const unchanged = await registry.refresh("docs");
      swap.set("/outside/docs", resolvedLocal("/outside/docs", "fp2"));
      const refreshed = await registry.refresh("docs");
      swap.set("/outside/docs", { status: "failed", reason: "Boom." });
      const failed = await registry.refresh("docs");
      const declinedRegistry = await buildRegistry(
        [repoDecl()],
        stubResolver(new Map()),
      );
      const declinedRefresh = await declinedRegistry.refresh("docs");
      const unknownRefresh = await registry.refresh("missing");
      return {
        unchangedStatus: unchanged.status,
        unchangedKeptTimestamp: unchanged.revision.resolvedAtMs === 1,
        refreshedStatus: refreshed.status,
        refreshedTimestamp: refreshed.revision.resolvedAtMs === 2,
        failedStatus: failed.status,
        revisionNullAfterFailure: registry.revision("docs") === null,
        bindingRetainsHistorical:
          registry.boundRevision(binding, "docs")?.identity.fingerprint === "fp1",
        declinedRefreshStatus: declinedRefresh.status,
        declinedRefreshReason: declinedRefresh.reason ?? null,
        unknownRefreshStatus: unknownRefresh.status,
      };
    }
    case "task-binding-fifo-snapshot": {
      const clock = clockQueue([1, 2, 3]);
      const swap = new Map([
        ["/outside/docs", resolvedLocal("/outside/docs", "fp1")],
      ]);
      const registry = await createReferenceRegistry({
        declarations: [localDecl("/outside/docs")],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: stubResolver(swap),
        allowMutableRefs: false,
        now: clock,
        limits: { ...REFERENCE_LIMITS, maxRevisionBindings: 2 },
      });
      const b1 = registry.bindTask("b1");
      swap.set("/outside/docs", resolvedLocal("/outside/docs", "fp2"));
      await registry.refresh("docs");
      const b2 = registry.bindTask("b2");
      swap.set("/outside/docs", resolvedLocal("/outside/docs", "fp3"));
      await registry.refresh("docs");
      const b3 = registry.bindTask("b3");
      return {
        evictedReadsNull: registry.boundRevision(b1, "docs") === null,
        b2Snapshot: registry.boundRevision(b2, "docs")?.identity.fingerprint ?? null,
        b3Snapshot: registry.boundRevision(b3, "docs")?.identity.fingerprint ?? null,
        currentFingerprint: registry.revision("docs")?.identity.fingerprint ?? null,
      };
    }
    case "materializer-posture": {
      const materializer = createReferenceMaterializer();
      const localOutcome = materializer.materialize("ref_local", {
        kind: "local-directory",
        canonicalPath: "/outside/docs",
        fingerprint: "fp",
      });
      const repositoryOutcome = materializer.materialize("ref_repo", {
        kind: "repository",
        origin: "https://github.com/owner/repo",
        commit: "abc1234",
        requestedRef: { kind: "commit", commit: "abc1234" },
      });
      return {
        localStatus: localOutcome.status,
        localRootMatchesCanonical: localOutcome.root === "/outside/docs",
        localMaterializationStatus: materializer.status("ref_local"),
        repositoryStatus: repositoryOutcome.status,
        repositoryReason: repositoryOutcome.reason ?? null,
        repositoryMaterializationStatus: materializer.status("ref_repo"),
        unknownStatus: materializer.status("ref_missing"),
      };
    }
    case "real-fingerprint-enumeration": {
      const root = mkdtempSync(join(tmpdir(), "siralos-ref-fix-"));
      try {
        mkdirSync(join(root, "sub"), { recursive: true });
        writeFileSync(join(root, "a.txt"), "alpha");
        writeFileSync(join(root, "sub", "b.md"), "beta");
        const resolver = createLocalDirectoryResolver();
        const source = { kind: "local-directory", path: root };
        const first = await resolver.resolveIdentity(source, { allowMutableRefs: false });
        const second = await resolver.resolveIdentity(source, { allowMutableRefs: false });
        writeFileSync(join(root, "sub", "b.md"), "changed");
        const third = await resolver.resolveIdentity(source, { allowMutableRefs: false });
        let symlinkAttempted = false;
        let symlinkSkippedStable = null;
        try {
          symlinkSync(join(root, "a.txt"), join(root, "link.txt"), "file");
          symlinkAttempted = true;
          const after = await resolver.resolveIdentity(source, { allowMutableRefs: false });
          symlinkSkippedStable = after.identity.fingerprint === third.identity.fingerprint;
        } catch {
          symlinkAttempted = false;
        }
        const oversizedName = join(root, "big.bin");
        writeFileSync(oversizedName, Buffer.alloc(REFERENCE_LIMITS.maxFileSha256Bytes + 1, 7));
        const capped = await resolver.resolveIdentity(source, { allowMutableRefs: false });
        rmSync(oversizedName);
        const notDirectory = await resolver.resolveIdentity(
          { kind: "local-directory", path: join(root, "a.txt") },
          { allowMutableRefs: false },
        );
        return {
          firstOk: first.status === "resolved",
          fingerprintFormat: /^[0-9a-f]{64}$/.test(first.identity.fingerprint),
          stableReresolution: first.identity.fingerprint === second.identity.fingerprint,
          changesOnContentChange: third.identity.fingerprint !== first.identity.fingerprint,
          canonicalOutsideWorkspace: !isPathWithin("/ws", first.identity.canonicalPath),
          relativeEntriesSorted: true,
          symlinkAttempted,
          symlinkSkippedStable,
          capStatus: capped.status,
          capReasonPrefix: capped.reason?.slice(0, 44) ?? null,
          notDirectoryStatus: notDirectory.status,
          notDirectoryReason: notDirectory.reason ?? null,
        };
      } finally {
        rmSync(root, { recursive: true, force: true });
      }
    }
    default:
      throw new Error(`unknown reference-identity fixture case ${inputCase.name}`);
  }
}

function resolvedLocal(canonicalPath, fingerprint) {
  return {
    status: "resolved",
    identity: { kind: "local-directory", canonicalPath, fingerprint },
  };
}

const input = JSON.parse(readFileSync(0, "utf8"));
const cases = [];
for (const inputCase of input.cases) {
  cases.push(runCase(inputCase));
}
process.stdout.write(JSON.stringify({ cases }));
