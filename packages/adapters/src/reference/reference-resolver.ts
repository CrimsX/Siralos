import { createHash } from "node:crypto";
import { lstat, opendir, open, realpath, stat } from "node:fs/promises";
import path from "node:path";
import {
  REFERENCE_LIMITS,
  canonicalizeJson,
  sha256Hex,
  type ReferenceLimits,
  type ReferenceResolutionOutcome,
  type ReferenceResolverPort,
  type ReferenceSource,
  type RepositoryRef,
} from "@solaris/core";
import { describeFsError } from "../tools/workspace/workspace-path.js";

/**
 * Reference resolvers (Stage 3 milestone 5).
 *
 * `createLocalDirectoryResolver` maps a declared local-directory source to
 * a canonical path plus a bounded manifest fingerprint: every regular file
 * beneath the root is enumerated (symlinks are never traversed; special
 * files make the manifest non-fingerprintable — fail closed with a precise
 * reason) and hashed with SHA-256 (per-file cap `maxFileSha256Bytes`; a
 * file above the cap fails resolution), then the fingerprint is the SHA-256
 * of the canonical JSON manifest of sorted relative paths + hashes.
 * Exceeding the manifest caps (entries/bytes) fails resolution — a
 * non-fingerprintable reference is never silently marked "unhashed".
 *
 * `createRepositoryResolver` shapes the outcome identity from a
 * `RepositoryRevisionBackend`. The REAL production backend is
 * `createUnavailableRepositoryBackend`: repository resolution requires
 * sandboxed git execution, which is unavailable at this stage — nothing is
 * ever spawned or fetched, and no git implementation lives here.
 * `createFakeRepositoryBackend` is the deterministic, network-free
 * stand-in for tests and the behavior harness.
 */

type ManifestFile = { readonly relativePath: string; readonly sha256: string };

type ManifestOutcome =
  | { readonly ok: true; readonly files: readonly ManifestFile[] }
  | { readonly ok: false; readonly reason: string };

/** Work budget for enumeration: at most `maxManifestEntries` non-file entries. */
const MAX_DIRECTORY_ENTRIES_FACTOR = 2;
const MAX_MANIFEST_DEPTH = 64;

/**
 * Bounded, deterministic manifest enumeration. Every enumerated entry is
 * lstat'd (symlinks skipped, never traversed; special files fail the
 * manifest); regular files are size-checked and SHA-256-hashed through a
 * bounded read loop (a short read is never treated as EOF, and a file
 * grown past the cap after its lstat fails the manifest).
 */
async function buildManifest(root: string, limits: ReferenceLimits): Promise<ManifestOutcome> {
  const files: ManifestFile[] = [];
  let totalBytes = 0;
  let directoriesVisited = 0;
  let entriesExamined = 0;
  const pending: Array<{ readonly absolute: string; readonly relative: string }> = [
    { absolute: root, relative: "" },
  ];
  while (pending.length > 0) {
    const directory = pending.pop();
    if (directory === undefined) {
      break;
    }
    directoriesVisited += 1;
    if (directoriesVisited > limits.maxManifestEntries) {
      return {
        ok: false,
        reason: `Reference manifest is too large: more than ${limits.maxManifestEntries} directories.`,
      };
    }
    if (directory.relative.split("/").length > MAX_MANIFEST_DEPTH) {
      return {
        ok: false,
        reason: `Reference manifest is too large: directory depth exceeds ${MAX_MANIFEST_DEPTH}.`,
      };
    }
    const names: string[] = [];
    let handle;
    try {
      handle = await opendir(directory.absolute);
    } catch (error: unknown) {
      return {
        ok: false,
        reason: `Cannot enumerate reference directory: ${describeFsError(error)}`,
      };
    }
    try {
      for await (const entry of handle) {
        entriesExamined += 1;
        if (entriesExamined > limits.maxManifestEntries * MAX_DIRECTORY_ENTRIES_FACTOR) {
          return { ok: false, reason: "Reference manifest is too large: entry budget exceeded." };
        }
        names.push(entry.name);
      }
    } finally {
      await handle.close().catch(() => undefined);
    }
    names.sort();
    for (const name of names) {
      const absolute = path.join(directory.absolute, name);
      const relativePath = directory.relative === "" ? name : `${directory.relative}/${name}`;
      let stats;
      try {
        stats = await lstat(absolute);
      } catch (error: unknown) {
        return {
          ok: false,
          reason: `Cannot inspect reference entry ${relativePath}: ${describeFsError(error)}`,
        };
      }
      if (stats.isSymbolicLink()) {
        // Symlinks are never traversed and never enter the manifest.
        continue;
      }
      if (stats.isDirectory()) {
        pending.push({ absolute, relative: relativePath });
        continue;
      }
      if (!stats.isFile()) {
        return {
          ok: false,
          reason: `Reference manifest is not fingerprintable: special file at ${relativePath}.`,
        };
      }
      if (stats.size > limits.maxFileSha256Bytes) {
        return {
          ok: false,
          reason: `Reference manifest is not fingerprintable: file at ${relativePath} is ${stats.size} bytes (limit ${limits.maxFileSha256Bytes}).`,
        };
      }
      const hash = await hashFileBounded(absolute, limits.maxFileSha256Bytes);
      if (!hash.ok) {
        return { ok: false, reason: hash.reason };
      }
      files.push({ relativePath, sha256: hash.sha256 });
      totalBytes += stats.size;
      if (files.length > limits.maxManifestEntries) {
        return {
          ok: false,
          reason: `Reference manifest is too large: more than ${limits.maxManifestEntries} files.`,
        };
      }
      if (totalBytes > limits.maxManifestBytes) {
        return {
          ok: false,
          reason: `Reference manifest is too large: more than ${limits.maxManifestBytes} bytes.`,
        };
      }
    }
  }
  files.sort((a, b) =>
    a.relativePath < b.relativePath ? -1 : a.relativePath > b.relativePath ? 1 : 0,
  );
  return { ok: true, files };
}

/** Bounded SHA-256 of one regular file (explicit-offset read loop). */
async function hashFileBounded(
  absolute: string,
  maxBytes: number,
): Promise<
  { readonly ok: true; readonly sha256: string } | { readonly ok: false; readonly reason: string }
> {
  let handle;
  try {
    handle = await open(absolute, "r");
  } catch (error: unknown) {
    return { ok: false, reason: `Cannot read reference file: ${describeFsError(error)}` };
  }
  try {
    const hash = createHash("sha256");
    const buffer = Buffer.allocUnsafe(64 * 1024);
    let total = 0;
    for (;;) {
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, total);
      if (bytesRead === 0) {
        break;
      }
      total += bytesRead;
      if (total > maxBytes) {
        return {
          ok: false,
          reason: `Reference manifest is not fingerprintable: file exceeds ${maxBytes} bytes.`,
        };
      }
      hash.update(buffer.subarray(0, bytesRead));
    }
    return { ok: true, sha256: hash.digest("hex") };
  } catch (error: unknown) {
    return { ok: false, reason: `Cannot hash reference file: ${describeFsError(error)}` };
  } finally {
    await handle.close().catch(() => undefined);
  }
}

export interface CreateLocalDirectoryResolverOptions {
  /** Reserved for API symmetry with the resolver family; the registry stamps times. */
  readonly now?: () => number;
  readonly limits?: Partial<ReferenceLimits>;
}

export function createLocalDirectoryResolver(
  options: CreateLocalDirectoryResolverOptions = {},
): ReferenceResolverPort {
  const limits: ReferenceLimits = { ...REFERENCE_LIMITS, ...options.limits };
  return {
    async resolveIdentity(
      source: ReferenceSource,
      _options: { readonly allowMutableRefs: boolean },
    ): Promise<ReferenceResolutionOutcome> {
      if (source.kind !== "local-directory") {
        return {
          status: "unavailable",
          reason: "This resolver only handles local-directory sources.",
        };
      }
      let canonical: string;
      try {
        canonical = await realpath(source.path);
      } catch (error: unknown) {
        return {
          status: "unavailable",
          reason: `Reference path cannot be resolved: ${describeFsError(error)}`,
        };
      }
      let stats;
      try {
        stats = await stat(canonical);
      } catch (error: unknown) {
        return {
          status: "unavailable",
          reason: `Reference path is not accessible: ${describeFsError(error)}`,
        };
      }
      if (!stats.isDirectory()) {
        return { status: "failed", reason: "Reference path is not a directory." };
      }
      const manifest = await buildManifest(canonical, limits);
      if (!manifest.ok) {
        return { status: "failed", reason: manifest.reason };
      }
      const fingerprint = sha256Hex(canonicalizeJson({ files: manifest.files }));
      return {
        status: "resolved",
        identity: { kind: "local-directory", canonicalPath: canonical, fingerprint },
      };
    },
  };
}

export interface RepositoryRevisionBackend {
  resolveCommit(
    origin: string,
    ref: RepositoryRef,
    options: { readonly allowMutableRefs: boolean },
  ): ReferenceResolutionOutcome | Promise<ReferenceResolutionOutcome>;
}

export function createRepositoryResolver(
  backend: RepositoryRevisionBackend,
): ReferenceResolverPort {
  return {
    async resolveIdentity(
      source: ReferenceSource,
      options: { readonly allowMutableRefs: boolean },
    ): Promise<ReferenceResolutionOutcome> {
      if (source.kind !== "repository") {
        return {
          status: "unavailable",
          reason: "This resolver only handles repository sources.",
        };
      }
      const outcome = await backend.resolveCommit(source.repository, source.ref, options);
      if (outcome.status !== "resolved") {
        return outcome;
      }
      if (outcome.identity.kind !== "repository") {
        return {
          status: "failed",
          reason: "Repository backend returned a non-repository identity.",
        };
      }
      return {
        status: "resolved",
        identity: {
          kind: "repository",
          origin: source.repository,
          commit: outcome.identity.commit,
          requestedRef: source.ref,
        },
      };
    },
  };
}

export const REPOSITORY_RESOLUTION_UNAVAILABLE_MESSAGE =
  "repository resolution requires sandboxed git execution, which is unavailable at this stage";

/**
 * The REAL production repository backend: always reports the source
 * unavailable. No sandboxed git execution exists at this stage — nothing
 * is spawned and nothing is fetched. This is the fail-closed posture;
 * a real git implementation must never live here.
 */
export function createUnavailableRepositoryBackend(
  reason: string = REPOSITORY_RESOLUTION_UNAVAILABLE_MESSAGE,
): RepositoryRevisionBackend {
  return {
    resolveCommit(): ReferenceResolutionOutcome {
      return { status: "unavailable", reason };
    },
  };
}

/**
 * Deterministic, network-free repository backend for tests and the
 * behavior harness: `origin -> { commits, tags, branches }`. Commits are
 * matched by full/abbreviated SHA against the fixture; tags and branches
 * map to commits. Mutable refs (branches) are refused unless
 * `allowMutableRefs` is set, mirroring the registry policy as defense in
 * depth.
 */
export type FakeRepositoryFixture = Readonly<
  Record<
    string,
    {
      readonly commits: Readonly<Record<string, unknown>>;
      readonly tags: Readonly<Record<string, string>>;
      readonly branches: Readonly<Record<string, string>>;
    }
  >
>;

export const MUTABLE_REF_REFUSAL = "mutable repository ref requires an explicit pinned commit/tag";
const COMMIT_SHA_PATTERN = /^[0-9a-f]{7,64}$/i;

export function createFakeRepositoryBackend(
  fixture: FakeRepositoryFixture,
): RepositoryRevisionBackend {
  return {
    resolveCommit(
      origin: string,
      ref: RepositoryRef,
      options: { readonly allowMutableRefs: boolean },
    ): ReferenceResolutionOutcome {
      const repository = fixture[origin];
      if (repository === undefined) {
        return { status: "failed", reason: `Unknown repository origin "${origin}".` };
      }
      switch (ref.kind) {
        case "commit": {
          if (!COMMIT_SHA_PATTERN.test(ref.commit)) {
            return { status: "failed", reason: `Malformed commit "${ref.commit}".` };
          }
          if (!(ref.commit in repository.commits)) {
            return { status: "failed", reason: `Unknown commit "${ref.commit}".` };
          }
          return {
            status: "resolved",
            identity: { kind: "repository", origin, commit: ref.commit, requestedRef: ref },
          };
        }
        case "tag": {
          const commit = repository.tags[ref.tag];
          if (commit === undefined) {
            return { status: "failed", reason: `Unknown tag "${ref.tag}".` };
          }
          return {
            status: "resolved",
            identity: { kind: "repository", origin, commit, requestedRef: ref },
          };
        }
        case "branch": {
          if (!options.allowMutableRefs) {
            return { status: "refused", reason: MUTABLE_REF_REFUSAL };
          }
          const commit = repository.branches[ref.branch];
          if (commit === undefined) {
            return { status: "failed", reason: `Unknown branch "${ref.branch}".` };
          }
          return {
            status: "resolved",
            identity: { kind: "repository", origin, commit, requestedRef: ref },
          };
        }
      }
    },
  };
}

export interface CreateReferenceResolverOptions {
  readonly local?: ReferenceResolverPort;
  readonly repository?: ReferenceResolverPort;
  /** Reserved for API symmetry with the resolver family; the registry stamps times. */
  readonly now?: () => number;
}

/** Dispatch resolver: routes by source kind; a missing side fails closed as unavailable. */
export function createReferenceResolver(
  options: CreateReferenceResolverOptions = {},
): ReferenceResolverPort {
  return {
    async resolveIdentity(
      source: ReferenceSource,
      opts: { readonly allowMutableRefs: boolean },
    ): Promise<ReferenceResolutionOutcome> {
      if (source.kind === "local-directory") {
        if (options.local === undefined) {
          return { status: "unavailable", reason: "Local-directory resolution is not configured." };
        }
        return options.local.resolveIdentity(source, opts);
      }
      if (options.repository === undefined) {
        return { status: "unavailable", reason: "Repository resolution is not configured." };
      }
      return options.repository.resolveIdentity(source, opts);
    },
  };
}
