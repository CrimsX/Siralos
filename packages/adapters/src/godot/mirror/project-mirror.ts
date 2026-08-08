import { createHash } from "node:crypto";
import { copyFile, lstat, mkdir, open, readdir, realpath, rm, stat } from "node:fs/promises";
import path from "node:path";
import { GODOT_LIMITS } from "@solaris/core";
import type {
  PreparedProjectMirror,
  ProjectMirror,
  ProjectMirrorFileEntry,
  ProjectMirrorRequest,
  ProjectMirrorPreparationResult,
  ProjectMirrorVerification,
} from "@solaris/core";

/**
 * Generated build/cache/metadata directories and Solaris-owned temporary
 * state that are never copied into a project mirror. `.gitignore` is never
 * interpreted as a security policy: this list is fixed and Solaris-owned.
 */
export const MIRROR_EXCLUDED_DIRECTORIES: readonly string[] = [
  ".git",
  ".godot",
  "node_modules",
  "dist",
  "coverage",
  ".solaris",
];

/** Solaris temporary/quarantine prefixes excluded wherever they appear. */
export const MIRROR_EXCLUDED_PREFIXES: readonly string[] = [
  ".solaris-mutation-",
  ".solaris-quarantine-",
];

export interface ProjectMirrorLimits {
  readonly maxFiles: number;
  readonly maxBytes: number;
  readonly maxSingleFileBytes: number;
  readonly maxRelativePathBytes: number;
  readonly maxDepth: number;
  readonly prepareTimeoutMs: number;
}

export const DEFAULT_PROJECT_MIRROR_LIMITS: ProjectMirrorLimits = {
  maxFiles: GODOT_LIMITS.maxMirrorFiles,
  maxBytes: GODOT_LIMITS.maxMirrorBytes,
  maxSingleFileBytes: GODOT_LIMITS.maxMirrorSingleFileBytes,
  maxRelativePathBytes: GODOT_LIMITS.maxMirrorRelativePathBytes,
  maxDepth: GODOT_LIMITS.maxMirrorDepth,
  prepareTimeoutMs: GODOT_LIMITS.mirrorPrepareTimeoutMs,
};

export interface ProjectMirrorDependencies {
  readonly limits?: Partial<ProjectMirrorLimits>;
  readonly exclusions?: readonly string[];
}

/**
 * Disposable Godot project mirror. Copy policy: regular files and regular
 * directories only; symbolic links, junctions, sockets, FIFOs, device and
 * other special files are rejected (never silently dereferenced); generated
 * directories and Solaris temporary state are excluded; every copied byte is
 * hash-verified; every bound is enforced during the copy and a partial
 * mirror is always cleaned; the mirror path is Solaris-generated and can
 * never be chosen by a provider or by the project.
 */
export function createProjectMirror(dependencies: ProjectMirrorDependencies = {}): ProjectMirror {
  const limits: ProjectMirrorLimits = { ...DEFAULT_PROJECT_MIRROR_LIMITS, ...dependencies.limits };
  const exclusions = dependencies.exclusions ?? MIRROR_EXCLUDED_DIRECTORIES;
  const mirrorProjectName = "project";

  async function prepare(request: ProjectMirrorRequest): Promise<ProjectMirrorPreparationResult> {
    const parent = await verifyParentDirectory(request);
    if (parent === null) {
      return {
        status: "failed",
        message: "The mirror parent directory is not a verified Solaris-owned directory.",
      };
    }
    const projectPath = path.join(parent, mirrorProjectName);
    try {
      await mkdir(projectPath, { mode: 0o700 });
    } catch (error: unknown) {
      return {
        status: "failed",
        message: `The mirror directory could not be created: ${describeError(error)}`,
      };
    }
    const verified = await verifyRealDirectory(projectPath);
    if (!verified.ok) {
      await removeMirrorPath(projectPath, parent);
      return { status: "failed", message: verified.message };
    }
    const deadline = Date.now() + limits.prepareTimeoutMs;
    const entries: ProjectMirrorFileEntry[] = [];
    let copiedBytes = 0;
    const state: {
      limit: { readonly limit: string; readonly message: string } | null;
      conflict: string | null;
      unsupported: string | null;
    } = { limit: null, conflict: null, unsupported: null };

    const abortCheck = (): void => {
      if (request.signal?.aborted) {
        throw createAbortError();
      }
    };

    try {
      const outcome = await copyTree(request.workspaceRoot, {
        deadline,
        ...(request.signal === undefined ? {} : { signal: request.signal }),
        onFile: async (sourcePath, relativePath, metadata) => {
          const byteLength = Buffer.byteLength(relativePath, "utf8");
          if (byteLength > limits.maxRelativePathBytes) {
            state.limit = {
              limit: "path-length",
              message: `The mirror path ${relativePath} exceeds the ${limits.maxRelativePathBytes}-byte relative path limit.`,
            };
            return false;
          }
          if (metadata.size > limits.maxSingleFileBytes) {
            state.limit = {
              limit: "single-file",
              message: `The project file ${relativePath} exceeds the ${limits.maxSingleFileBytes}-byte single-file limit.`,
            };
            return false;
          }
          if (entries.length >= limits.maxFiles) {
            state.limit = {
              limit: "files",
              message: `The project exceeds the ${limits.maxFiles}-file mirror limit.`,
            };
            return false;
          }
          if (copiedBytes + metadata.size > limits.maxBytes) {
            state.limit = {
              limit: "bytes",
              message: `The project exceeds the ${limits.maxBytes}-byte mirror limit.`,
            };
            return false;
          }
          const sourceHash = await hashFile(sourcePath, abortCheck);
          if (sourceHash === null) {
            state.limit = {
              limit: "bytes",
              message: `The project file ${relativePath} could not be hashed.`,
            };
            return false;
          }
          const targetPath = path.join(projectPath, relativePath);
          await mkdir(path.dirname(targetPath), { recursive: true });
          await copyFile(sourcePath, targetPath);
          const mirrorHash = await hashFile(targetPath, abortCheck);
          if (mirrorHash === null || mirrorHash !== sourceHash) {
            return "hash-mismatch";
          }
          const after = await stat(sourcePath).catch(() => null);
          if (
            after === null ||
            after.size !== metadata.size ||
            after.mtimeMs !== metadata.mtimeMs
          ) {
            state.conflict = `The project file ${relativePath} changed while it was being mirrored.`;
            return false;
          }
          entries.push({
            relativePath: normalizeRelative(relativePath),
            bytes: metadata.size,
            sha256: sourceHash,
          });
          copiedBytes += metadata.size;
          return true;
        },
      });
      if (outcome.rejected !== null) {
        state.unsupported = outcome.rejected;
      }
      if (outcome.depthExceeded !== null) {
        state.limit = outcome.depthExceeded;
      }
    } catch (error: unknown) {
      if (request.signal?.aborted) {
        await removeMirrorPath(projectPath, parent);
        throw createAbortError();
      }
      await removeMirrorPath(projectPath, parent);
      return {
        status: "failed",
        message: `The project mirror could not be prepared: ${describeError(error)}`,
      };
    }
    if (Date.now() > deadline) {
      await removeMirrorPath(projectPath, parent);
      return {
        status: "failed",
        message: `The project mirror preparation exceeded its ${limits.prepareTimeoutMs}ms deadline.`,
      };
    }
    if (state.limit !== null) {
      await removeMirrorPath(projectPath, parent);
      return {
        status: "too_large",
        limit: state.limit.limit,
        message: state.limit.message,
      };
    }
    if (state.conflict !== null) {
      await removeMirrorPath(projectPath, parent);
      return { status: "conflict", message: state.conflict };
    }
    if (state.unsupported !== null) {
      await removeMirrorPath(projectPath, parent);
      return {
        status: "mirror_unsupported",
        message: state.unsupported,
      };
    }
    entries.sort((left, right) => left.relativePath.localeCompare(right.relativePath));
    const mirror: PreparedProjectMirror = {
      sourceRoot: request.workspaceRoot,
      parentDirectory: parent,
      projectPath,
      entries,
      copiedBytes,
    };
    const verification = await verify(mirror, request.signal);
    if (!verification.ok) {
      await removeMirrorPath(projectPath, parent);
      return {
        status: "conflict",
        message: `The mirror verification failed before the probe: ${verification.message}`,
      };
    }
    return { status: "ready", mirror };
  }

  async function verify(
    mirror: PreparedProjectMirror,
    signal?: AbortSignal,
  ): Promise<ProjectMirrorVerification> {
    const projectPath = mirror.projectPath;
    const entryMap = new Map(mirror.entries.map((entry) => [entry.relativePath, entry]));
    const seen = new Set<string>();
    const state: {
      unexpected: string | null;
      treeNote: string | null;
    } = { unexpected: null, treeNote: null };
    try {
      const tree = await walkTree(
        projectPath,
        signal,
        async (targetPath, relativePath, metadata, _depth, abortCheck) => {
          if (metadata.isSymbolicLink()) {
            return {
              stop: true,
              note: `A symbolic link appeared inside the mirror (${relativePath}); refusing to continue.`,
            };
          }
          if (!metadata.isFile()) {
            return { stop: false };
          }
          const normalized = normalizeRelative(relativePath);
          seen.add(normalized);
          const expected = entryMap.get(normalized);
          if (expected === undefined) {
            state.unexpected = `An unexpected file appeared inside the mirror (${relativePath}).`;
            return { stop: true };
          }
          const hash = await hashFile(targetPath, abortCheck);
          if (hash === null) {
            state.unexpected = `A mirrored file could not be hashed (${relativePath}).`;
            return { stop: true };
          }
          const metadataNow = await stat(targetPath).catch(() => null);
          if (hash !== expected.sha256 || metadataNow?.size !== expected.bytes) {
            state.unexpected = `The mirrored file ${relativePath} does not match its recorded hash.`;
            return { stop: true };
          }
          return { stop: false };
        },
      );
      state.treeNote = tree.note;
    } catch (error: unknown) {
      if (signal?.aborted) {
        throw createAbortError();
      }
      return {
        ok: false,
        reason: "inaccessible",
        message: `The mirror could not be verified: ${describeError(error)}`,
      };
    }
    if (state.treeNote !== null) {
      return { ok: false, reason: "symlink", message: state.treeNote };
    }
    if (state.unexpected !== null) {
      return {
        ok: false,
        reason: state.unexpected.startsWith("The mirrored file")
          ? "hash-mismatch"
          : "unexpected-files",
        message: state.unexpected,
      };
    }
    for (const entry of mirror.entries) {
      if (!seen.has(entry.relativePath)) {
        return {
          ok: false,
          reason: "missing",
          message: `The mirrored file ${entry.relativePath} is missing.`,
        };
      }
    }
    const sourceOutcome = await verifySourceAgainstEntries(mirror, signal);
    if (!sourceOutcome.ok) {
      return sourceOutcome;
    }
    return { ok: true };
  }

  async function verifySourceAgainstEntries(
    mirror: PreparedProjectMirror,
    signal?: AbortSignal,
  ): Promise<ProjectMirrorVerification> {
    const expected = new Map(mirror.entries.map((entry) => [entry.relativePath, entry]));
    let conflict: string | null = null;
    try {
      await walkTree(
        mirror.sourceRoot,
        signal,
        async (targetPath, relativePath, metadata, _depth, abortCheck) => {
          if (metadata.isSymbolicLink() || !metadata.isFile()) {
            return { stop: false };
          }
          const normalized = normalizeRelative(relativePath);
          const entry = expected.get(normalized);
          if (entry === undefined) {
            return { stop: false };
          }
          const hash = await hashFile(targetPath, abortCheck);
          if (hash === null) {
            conflict = `The source file ${relativePath} could not be rehashed.`;
            return { stop: true };
          }
          if (hash !== entry.sha256 || metadata.size !== entry.bytes) {
            conflict = `The source file ${relativePath} changed during the probe preparation.`;
            return { stop: true };
          }
          return { stop: false };
        },
      );
    } catch (error: unknown) {
      if (signal?.aborted) {
        throw createAbortError();
      }
      return {
        ok: false,
        reason: "inaccessible",
        message: `The source workspace could not be reverified: ${describeError(error)}`,
      };
    }
    if (conflict !== null) {
      return { ok: false, reason: "source-conflict", message: conflict };
    }
    return { ok: true };
  }

  async function destroy(
    mirror: PreparedProjectMirror,
  ): Promise<{ readonly ok: true } | { readonly ok: false; readonly message: string }> {
    if (!isPathInside(mirror.parentDirectory, mirror.projectPath)) {
      return {
        ok: false,
        message: "Cleanup refused: the mirror path is not inside its verified parent.",
      };
    }
    return removeMirrorPath(mirror.projectPath, mirror.parentDirectory);
  }

  /**
   * Walks the source tree and mirrors regular files. Every directory
   * component is created as a real directory; symbolic links, junctions,
   * and special files anywhere in the tree abort the copy as
   * `mirror_unsupported` (they are never dereferenced or preserved).
   */
  async function copyTree(
    sourceRoot: string,
    options: {
      readonly deadline: number;
      readonly signal?: AbortSignal;
      readonly onFile: (
        sourcePath: string,
        relativePath: string,
        metadata: { readonly size: number; readonly mtimeMs: number },
      ) => Promise<boolean | "hash-mismatch">;
    },
  ): Promise<{
    readonly rejected: string | null;
    readonly depthExceeded: { readonly limit: string; readonly message: string } | null;
  }> {
    let rejected: string | null = null;
    let depthExceeded: { readonly limit: string; readonly message: string } | null = null;
    const abortCheck = (): void => {
      if (options.signal?.aborted) {
        throw createAbortError();
      }
    };
    const walk = async (
      directory: string,
      relativeDirectory: string,
      depth: number,
    ): Promise<void> => {
      if (rejected !== null || depthExceeded !== null) {
        return;
      }
      abortCheck();
      if (Date.now() > options.deadline) {
        throw new Error("The mirror preparation deadline was exceeded.");
      }
      if (depth > limits.maxDepth) {
        depthExceeded = {
          limit: "depth",
          message: `The project exceeds the ${limits.maxDepth}-level directory depth limit.`,
        };
        return;
      }
      let entries: { readonly name: string; readonly isDirectory: () => boolean }[] = [];
      try {
        entries = await readdir(directory, { withFileTypes: true });
      } catch (error: unknown) {
        rejected = `A project directory could not be listed: ${describeError(error)}`;
        return;
      }
      entries.sort((left, right) => left.name.localeCompare(right.name));
      for (const entry of entries) {
        if (rejected !== null || depthExceeded !== null) {
          return;
        }
        abortCheck();
        if (Date.now() > options.deadline) {
          throw new Error("The mirror preparation deadline was exceeded.");
        }
        if (isExcludedName(entry.name)) {
          continue;
        }
        const entryPath = path.join(directory, entry.name);
        let metadata;
        try {
          metadata = await lstat(entryPath);
        } catch {
          rejected = `A project entry could not be inspected: ${entry.name}`;
          return;
        }
        const relativePath = path.join(relativeDirectory, entry.name);
        if (metadata.isSymbolicLink()) {
          rejected = `The project contains a symbolic link (${normalizeRelative(relativePath)}); mirroring is unsupported without dereferencing it.`;
          return;
        }
        if (metadata.isDirectory()) {
          await walk(entryPath, relativePath, depth + 1);
          continue;
        }
        if (!metadata.isFile()) {
          rejected = `The project contains a special file (${normalizeRelative(relativePath)}); mirroring is unsupported.`;
          return;
        }
        const outcome = await options.onFile(entryPath, normalizeRelative(relativePath), {
          size: metadata.size,
          mtimeMs: metadata.mtimeMs,
        });
        if (outcome === "hash-mismatch") {
          rejected = `The mirrored content of ${normalizeRelative(relativePath)} did not match its source hash.`;
          return;
        }
        if (!outcome) {
          return;
        }
      }
    };
    await walk(sourceRoot, "", 0);
    return { rejected, depthExceeded };
  }

  /**
   * Symlink-safe traversal of a tree under a verified root. Symlinks are
   * never followed; the visitor decides what happens per entry.
   */
  async function walkTree(
    root: string,
    signal: AbortSignal | undefined,
    visitor: (
      targetPath: string,
      relativePath: string,
      metadata: Awaited<ReturnType<typeof lstat>>,
      depth: number,
      abortCheck: () => void,
    ) => Promise<{ readonly stop: boolean; readonly note?: string }>,
  ): Promise<{ readonly note: string | null }> {
    let note: string | null = null;
    const walk = async (
      directory: string,
      relativeDirectory: string,
      depth: number,
    ): Promise<void> => {
      if (note !== null) {
        return;
      }
      if (signal?.aborted) {
        throw createAbortError();
      }
      let entries;
      try {
        entries = await readdir(directory, { withFileTypes: true });
      } catch {
        note = `A directory could not be listed during verification: ${relativeDirectory}`;
        return;
      }
      entries.sort((left, right) => left.name.localeCompare(right.name));
      for (const entry of entries) {
        if (note !== null) {
          return;
        }
        if (signal?.aborted) {
          throw createAbortError();
        }
        if (isExcludedName(entry.name)) {
          continue;
        }
        const entryPath = path.join(directory, entry.name);
        let metadata;
        try {
          metadata = await lstat(entryPath);
        } catch {
          note = `An entry could not be inspected during verification: ${entry.name}`;
          return;
        }
        const relativePath = path.join(relativeDirectory, entry.name);
        if (metadata.isDirectory() && !metadata.isSymbolicLink()) {
          await walk(entryPath, relativePath, depth + 1);
          continue;
        }
        const outcome = await visitor(entryPath, relativePath, metadata, depth + 1, () => {
          if (signal?.aborted) {
            throw createAbortError();
          }
        });
        if (outcome.note !== undefined) {
          note = outcome.note;
          return;
        }
        if (outcome.stop) {
          return;
        }
      }
    };
    await walk(root, "", 0);
    return { note };
  }

  function isExcludedName(name: string): boolean {
    if (exclusions.includes(name)) {
      return true;
    }
    return MIRROR_EXCLUDED_PREFIXES.some((prefix) => name.startsWith(prefix));
  }

  return {
    prepare,
    verify,
    destroy,
  };
}

async function verifyParentDirectory(request: ProjectMirrorRequest): Promise<string | null> {
  let canonicalWorkspace: string | null;
  try {
    canonicalWorkspace = await realpath(request.workspaceRoot);
  } catch {
    return null;
  }
  let canonicalParent: string | null;
  try {
    canonicalParent = await realpath(request.parentDirectory);
  } catch {
    return null;
  }
  if (
    canonicalParent === canonicalWorkspace ||
    isPathInside(canonicalWorkspace, canonicalParent) ||
    isPathInside(canonicalParent, canonicalWorkspace)
  ) {
    return null;
  }
  for (const forbidden of request.forbiddenRoots ?? []) {
    let canonicalForbidden: string | null;
    try {
      canonicalForbidden = await realpath(forbidden);
    } catch {
      continue;
    }
    if (
      canonicalForbidden !== null &&
      (canonicalForbidden === canonicalParent || isPathInside(canonicalForbidden, canonicalParent))
    ) {
      return null;
    }
  }
  let metadata;
  try {
    metadata = await lstat(canonicalParent);
  } catch {
    return null;
  }
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
    return null;
  }
  const verified = await verifyRealDirectory(canonicalParent);
  return verified.ok ? canonicalParent : null;
}

async function verifyRealDirectory(
  target: string,
): Promise<{ readonly ok: true } | { readonly ok: false; readonly message: string }> {
  let metadata;
  try {
    metadata = await lstat(target);
  } catch (error: unknown) {
    return { ok: false, message: `The path could not be inspected: ${describeError(error)}` };
  }
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
    return {
      ok: false,
      message: "The path is not a real directory.",
    };
  }
  let canonical: string;
  try {
    canonical = await realpath(target);
  } catch {
    return {
      ok: false,
      message: "The path cannot be resolved canonically.",
    };
  }
  if (canonical !== target) {
    return {
      ok: false,
      message: "The path resolves through a link; refusing to use it.",
    };
  }
  return { ok: true };
}

/**
 * Removes the mirror directory with no-follow semantics: the leaf must be a
 * real directory resolving canonically to itself inside the verified parent
 * immediately before deletion, so a link planted in between can never
 * redirect the recursive removal.
 */
async function removeMirrorPath(
  target: string,
  parentDirectory: string,
): Promise<{ readonly ok: true } | { readonly ok: false; readonly message: string }> {
  if (!isPathInside(parentDirectory, target)) {
    return {
      ok: false,
      message: "Cleanup refused: the mirror path is not inside its verified parent.",
    };
  }
  let metadata;
  try {
    metadata = await lstat(target);
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return { ok: true };
    }
    return { ok: false, message: `Cleanup refused: ${describeError(error)}` };
  }
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
    return {
      ok: false,
      message: "Cleanup refused: the mirror path is not a real directory.",
    };
  }
  let canonical: string;
  try {
    canonical = await realpath(target);
  } catch {
    return {
      ok: false,
      message: "Cleanup refused: the mirror path cannot be resolved canonically.",
    };
  }
  if (canonical !== target) {
    return {
      ok: false,
      message: "Cleanup refused: the mirror path resolves through a link.",
    };
  }
  try {
    await rm(target, { recursive: true, force: true });
    return { ok: true };
  } catch (error: unknown) {
    return {
      ok: false,
      message: `Cleanup failed: ${describeError(error)}`,
    };
  }
}

async function hashFile(filePath: string, abortCheck: () => void): Promise<string | null> {
  let handle;
  try {
    handle = await open(filePath, "r");
  } catch {
    return null;
  }
  try {
    const hash = createHash("sha256");
    const buffer = Buffer.alloc(1024 * 1024);
    for (;;) {
      abortCheck();
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, null);
      if (bytesRead === 0) {
        break;
      }
      hash.update(buffer.subarray(0, bytesRead));
    }
    return hash.digest("hex");
  } catch (error: unknown) {
    if (isAbortError(error)) {
      throw error;
    }
    return null;
  } finally {
    await handle.close().catch(() => undefined);
  }
}

function isAbortError(error: unknown): boolean {
  return (
    error instanceof Error &&
    (error.name === "AbortError" || ("code" in error && error.code === "ABORT_ERR"))
  );
}

function normalizeRelative(relativePath: string): string {
  return relativePath.split(path.sep).join("/");
}

function isPathInside(root: string, target: string): boolean {
  const rootPrefix = root.endsWith(path.sep) ? root : `${root}${path.sep}`;
  return target === root || target.startsWith(rootPrefix);
}

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown filesystem error occurred";
}

function createAbortError(): Error {
  return new DOMException("The project mirror operation was aborted.", "AbortError");
}
