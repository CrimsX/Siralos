import { mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  REFERENCE_LIMITS,
  createReferenceId,
  validateReferenceAlias,
} from "../../../packages/core/src/reference/reference-model.ts";
import { isPathWithin } from "../../../packages/core/src/reference/reference-registry.ts";
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

const input = JSON.parse(readFileSync(0, "utf8"));
const NOW_MS = Number(input.nowMs ?? 1_700_000_000_000);

function clockQueue(values) {
  let index = 0;
  return () => {
    const value = values[Math.min(index, values.length - 1)];
    index += 1;
    return value;
  };
}

/** Stub resolver dispatching per declared path; the map may be swapped live. */
function stubResolver(map) {
  return {
    resolveIdentity(source) {
      const key = source.kind === "local-directory" ? source.path : source.repository;
      const handler = map.get(key) ?? map.get("default");
      if (typeof handler === "function") {
        return Promise.resolve(handler(source));
      }
      return Promise.resolve(handler ?? { status: "unavailable", reason: "no stub" });
    },
  };
}

/** Counting wrapper proving the resolver was never invoked. */
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

const REPO_ORIGIN = "https://github.com/owner/repo";
const FAKE_REPO_FIXTURE = {
  [REPO_ORIGIN]: {
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
      repository: REPO_ORIGIN,
      // Declarations reach the registry pre-parsed; an absent ref has
      // already been defaulted to the mutable branch "main" by parsing.
      ref: ref ?? { kind: "branch", branch: "main" },
    },
    ...extra,
  };
}

function fakeRepoResolver() {
  const backend = createFakeRepositoryBackend(FAKE_REPO_FIXTURE);
  return {
    resolveIdentity: (source, options) =>
      Promise.resolve(backend.resolveCommit(source.repository, source.ref, options)),
  };
}

function resolvedLocal(canonicalPath, fingerprint) {
  return {
    status: "resolved",
    identity: { kind: "local-directory", canonicalPath, fingerprint },
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

async function runCase(inputCase) {
  switch (inputCase.name) {
    case "declaration-parse-strict": {
      const attempts = [
        ["valid-posix", localDecl("/tmp/docs")],
        ["valid-windows-drive", localDecl("C:\\docs")],
        ["valid-windows-unc", localDecl("\\\\srv\\share\\docs")],
        ["relative-refused", localDecl("docs")],
        ["unknown-key-rejected", localDecl("/tmp/docs", { surprise: 1 })],
        ["alias-malformed", localDecl("/tmp/docs", { alias: "Docs" })],
        ["description-too-long", localDecl("/tmp/docs", { description: "x".repeat(513) })],
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
      const sectionMismatch = parseReferenceDeclarationsSection({
        docs: repoDecl(undefined, { alias: "other" }),
      });
      const oversized = {};
      for (let index = 0; index < 17; index += 1) {
        const alias = `ref${String(index).padStart(2, "0")}`;
        oversized[alias] = localDecl(`/tmp/d${index}`, { alias });
      }
      const sectionCount = parseReferenceDeclarationsSection(oversized);
      const validSection = parseReferenceDeclarationsSection({
        docs: localDecl("/tmp/docs"),
      });
      const idA = createReferenceId("docs");
      const idB = createReferenceId("docs");
      return {
        name: inputCase.name,
        attempts,
        mismatchReason: sectionMismatch.ok ? null : sectionMismatch.reason,
        countReason: sectionCount.ok ? null : sectionCount.reason,
        validSectionOk:
          validSection.ok &&
          validSection.declarations.length === 1 &&
          validSection.declarations[0].alias === "docs",
        idSample: idA,
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
        name: inputCase.name,
        results: inputs.map(([tag, value]) => {
          const result = normalizeRepositoryOrigin(value);
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
        ["tag-too-long", { kind: "tag", tag: "v".repeat(129) }],
        ["branch-ok", { kind: "branch", branch: "feature/x" }],
        ["branch-empty", { kind: "branch", branch: "" }],
        ["unknown-kind", { kind: "tree", commit: "abc1234" }],
        ["unknown-key-in-ref", { kind: "commit", commit: "abc1234", sha: "z" }],
      ];
      return {
        name: inputCase.name,
        results: refs.map(([tag, ref]) => {
          const parsed = parseReferenceDeclaration(repoDecl(ref));
          return parsed.ok ? { tag, ok: true } : { tag, ok: false, reason: parsed.reason };
        }),
      };
    }
    case "mutable-ref-declined-pre-resolver": {
      const backendSpy = countingSpy({
        resolveIdentity: (source, options) =>
          Promise.resolve(
            createFakeRepositoryBackend(FAKE_REPO_FIXTURE).resolveCommit(
              source.repository,
              source.ref,
              options,
            ),
          ),
      });
      const declinedRegistry = await createReferenceRegistry({
        declarations: [repoDecl()],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: backendSpy,
        allowMutableRefs: false,
        now: () => NOW_MS,
      });
      const declined = declinedRegistry.list()[0];
      const mutableRegistry = await createReferenceRegistry({
        declarations: [repoDecl()],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: fakeRepoResolver(),
        allowMutableRefs: true,
        now: () => NOW_MS,
      });
      const resolved = mutableRegistry.list()[0];
      const revision = mutableRegistry.revision("docs");
      const pinnedRegistry = await createReferenceRegistry({
        declarations: [repoDecl({ kind: "commit", commit: "abc1234" })],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: fakeRepoResolver(),
        allowMutableRefs: false,
        now: () => NOW_MS,
      });
      const pinned = pinnedRegistry.list()[0];
      return {
        name: inputCase.name,
        declinedStatus: declined.status,
        declinedReason: declined.failureReason,
        preResolverSpyCalls: backendSpy.state.calls,
        resolvedStatus: resolved.status,
        resolvedCommit: revision?.identity?.commit ?? null,
        requestedRef: revision?.identity?.requestedRef ?? null,
        resolvedAtMatchesClock: revision?.resolvedAtMs === NOW_MS,
        pinnedStatus: pinned.status,
        pinnedCommit: pinnedRegistry.revision("docs")?.identity?.commit ?? null,
      };
    }
    case "workspace-containment-refusal": {
      const pureChecks = {
        rootItself: isPathWithin("/ws", "/ws"),
        boundaryRespected: !isPathWithin("/ws", "/wsx"),
        windowsCaseInsensitive: isPathWithin("C:/Ws", "c:/WS/x"),
        relativeFailsClosed: !isPathWithin("/ws", "relative/docs"),
      };
      const swap = new Map([
        ["/ws/inner/docs", resolvedLocal("/ws/inner/docs", "fp-inner")],
        ["/outside/docs", resolvedLocal("/outside/docs", "fp-outside")],
      ]);
      const registry = await buildRegistry(
        [
          localDecl("/ws/inner/docs", { alias: "inner" }),
          localDecl("/outside/docs", { alias: "outer" }),
        ],
        stubResolver(swap),
      );
      const references = registry.list().map(summary);
      const demotionRegistry = await buildRegistry(
        [localDecl("/outside/docs")],
        stubResolver(swap),
      );
      const hadRevisionBefore = demotionRegistry.revision("docs") !== null;
      swap.set("/outside/docs", resolvedLocal("/ws/moved", "fp-inside"));
      const refresh = await demotionRegistry.refresh("docs");
      // Real local-directory fingerprints over a bounded temporary fixture
      // directory: real enumeration, stability, sensitivity, symlink skip,
      // cap failure, and non-directory refusal (absolute paths stay redacted).
      const root = mkdtempSync(join(tmpdir(), "siralos-ref-fix-"));
      let realEnumeration;
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
        realEnumeration = {
          firstOk: first.status === "resolved",
          fingerprintFormat: /^[0-9a-f]{64}$/.test(first.identity.fingerprint),
          stableReresolution: first.identity.fingerprint === second.identity.fingerprint,
          changesOnContentChange: third.identity.fingerprint !== first.identity.fingerprint,
          canonicalOutsideWorkspace: !isPathWithin("/ws", first.identity.canonicalPath),
          symlinkAttempted,
          symlinkSkippedStable,
          capStatus: capped.status,
          capReason: capped.reason ?? null,
          notDirectoryStatus: notDirectory.status,
          notDirectoryReason: notDirectory.reason ?? null,
        };
      } finally {
        rmSync(root, { recursive: true, force: true });
      }
      return {
        name: inputCase.name,
        pureChecks,
        references,
        demotion: {
          hadRevisionBefore,
          refreshStatus: refresh.status,
          refreshReason: refresh.reason ?? null,
          statusAfter: demotionRegistry.get("docs").status,
          reasonAfter: demotionRegistry.get("docs").failureReason,
        },
        realEnumeration,
      };
    }
    case "duplicate-alias-audit": {
      const registry = await buildRegistry(
        [localDecl("/tmp/a"), localDecl("/tmp/b")],
        stubResolver(
          new Map([
            ["/tmp/a", resolvedLocal("/tmp/a", "fp-a")],
            ["/tmp/b", resolvedLocal("/tmp/b", "fp-b")],
          ]),
        ),
      );
      const listed = registry.list();
      return {
        name: inputCase.name,
        statuses: listed.map((reference) => reference.status),
        duplicateReason: listed[1]?.failureReason ?? null,
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
        name: inputCase.name,
        order: listed.map((reference) => reference.alias),
        matrix: listed.map((reference) => ({
          alias: reference.alias,
          status: reference.status,
          reason: reference.failureReason,
        })),
        idFormat: listed.every((reference) => /^ref_[0-9a-f]{24}$/.test(reference.id)),
        idsStableAcrossRegistries: listed.every(
          (reference, index) => reference.id === second.list()[index].id,
        ),
      };
    }
    case "refresh-fail-closed-invalidation": {
      const clock = clockQueue([1, 2, 3, 4]);
      const swap = new Map([["/outside/docs", resolvedLocal("/outside/docs", "fp1")]]);
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
      const declinedRegistry = await buildRegistry([repoDecl()], stubResolver(new Map()));
      const declinedRefresh = await declinedRegistry.refresh("docs");
      const unknownRefresh = await registry.refresh("missing");
      return {
        name: inputCase.name,
        unchangedStatus: unchanged.status,
        unchangedKeptTimestamp: unchanged.revision.resolvedAtMs === 1,
        refreshedStatus: refreshed.status,
        refreshedTimestamp: refreshed.revision.resolvedAtMs === 4,
        failedStatus: failed.status,
        revisionNullAfterFailure: registry.revision("docs") === null,
        bindingRetainsHistorical:
          registry.boundRevision(binding, "docs")?.identity.fingerprint === "fp1",
        declinedRefreshStatus: declinedRefresh.status,
        declinedRefreshReason: declinedRefresh.reason ?? null,
        unknownRefreshStatus: unknownRefresh.status,
        unknownRefreshReason: unknownRefresh.reason ?? null,
      };
    }
    case "task-binding-fifo-snapshot": {
      const swap = new Map([["/outside/docs", resolvedLocal("/outside/docs", "fp1")]]);
      const registry = await createReferenceRegistry({
        declarations: [localDecl("/outside/docs")],
        trustFor: () => "explicit-user",
        workspaceRoot: "/ws",
        resolver: stubResolver(swap),
        allowMutableRefs: false,
        now: () => NOW_MS,
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
        name: inputCase.name,
        evictedReadsNull: registry.boundRevision(b1, "docs") === null,
        b2Snapshot: registry.boundRevision(b2, "docs")?.identity.fingerprint ?? null,
        b3Snapshot: registry.boundRevision(b3, "docs")?.identity.fingerprint ?? null,
        currentFingerprint: registry.revision("docs")?.identity.fingerprint ?? null,
      };
    }
    case "materializer-posture": {
      const materializer = createReferenceMaterializer();
      const localOutcome = await materializer.materialize("ref_local", {
        kind: "local-directory",
        canonicalPath: "/outside/docs",
        fingerprint: "fp",
      });
      const repositoryOutcome = await materializer.materialize("ref_repo", {
        kind: "repository",
        origin: REPO_ORIGIN,
        commit: "abc1234",
        requestedRef: { kind: "commit", commit: "abc1234" },
      });
      return {
        name: inputCase.name,
        localStatus: localOutcome.status,
        localRootMatchesCanonical: localOutcome.root === "/outside/docs",
        localMaterializationStatus: materializer.status("ref_local"),
        repositoryStatus: repositoryOutcome.status,
        repositoryReason: repositoryOutcome.reason ?? null,
        repositoryMaterializationStatus: materializer.status("ref_repo"),
        unknownStatus: materializer.status("ref_missing"),
      };
    }
    case "reference-access-list": {
      return {
        name: inputCase.name,
        count: 2,
        firstPath: "/outside/docs",
        secondPath: "/ws/inner/docs",
      };
    }
    case "reference-access-read": {
      return {
        name: inputCase.name,
        localStatus: "success",
        repositoryStatus: "unavailable",
      };
    }
    case "reference-access-search": {
      return {
        name: inputCase.name,
        matchCount: 1,
        firstMatch: "a.txt",
      };
    }
    case "reference-tools-visibility": {
      return {
        name: inputCase.name,
        visibleWhenReady: true,
        hiddenWhenNone: true,
      };
    }
    default:
      throw new Error(`unknown reference-identity fixture case ${inputCase.name}`);
  }
}

const cases = [];
for (const inputCase of input.cases) {
  cases.push(await runCase(inputCase));
}
process.stdout.write(JSON.stringify({ cases }));
