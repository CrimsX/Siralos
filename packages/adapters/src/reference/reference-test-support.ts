import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import {
  REFERENCE_LIMITS,
  type MaterializationOutcome,
  type MaterializationStatus,
  type ReferenceId,
  type ReferenceMaterializerPort,
  type ResolvedReferenceIdentity,
} from "@solaris/core";
import type { FakeRepositoryFixture } from "./reference-resolver.js";

/**
 * Test/behavior-harness repository materializer (Stage 3 milestone 5).
 *
 * Writes a fixture commit's files into `<baseDir>/<origin-normalized>/<commit>/`
 * (deterministic and bounded). This is TEST SUPPORT ONLY: it is never used
 * in production paths — the production materializer is
 * `createReferenceMaterializer` (fail-closed, zero filesystem operations).
 * Kept in a dedicated module so the architecture check can allowlist
 * destructive fs APIs here without weakening the production module.
 */

export interface CreateFakeRepositoryMaterializerOptions {
  /** Deterministic base directory; defaults to a fresh `mkdtemp` under the OS temp dir. */
  readonly baseDir?: string;
}

function normalizeOriginForPath(origin: string): string {
  return origin.replace(/[^A-Za-z0-9._-]+/g, "_");
}

/** Reject fixture-relative paths that could escape the materialization root. */
function isSafeRelativePath(relativePath: string): boolean {
  if (relativePath.length === 0 || relativePath.includes("\0")) {
    return false;
  }
  for (const segment of relativePath.split(/[\\/]/)) {
    if (segment === ".." || segment === ".") {
      return false;
    }
  }
  return true;
}

export function createFakeRepositoryMaterializer(
  fixture: FakeRepositoryFixture,
  options: CreateFakeRepositoryMaterializerOptions = {},
): ReferenceMaterializerPort {
  const states = new Map<ReferenceId, MaterializationStatus>();
  let createdBaseDir: string | null = null;
  async function baseDir(): Promise<string> {
    if (options.baseDir !== undefined) {
      return options.baseDir;
    }
    if (createdBaseDir === null) {
      createdBaseDir = await mkdtemp(join(tmpdir(), "solaris-reference-"));
    }
    return createdBaseDir;
  }
  return {
    async materialize(
      referenceId: ReferenceId,
      identity: ResolvedReferenceIdentity,
    ): Promise<MaterializationOutcome> {
      if (identity.kind !== "repository") {
        states.set(referenceId, "failed");
        return {
          status: "failed",
          reason: "The fake repository materializer only materializes repository identities.",
        };
      }
      const repository = fixture[identity.origin];
      if (repository === undefined) {
        states.set(referenceId, "failed");
        return { status: "failed", reason: `Unknown repository origin "${identity.origin}".` };
      }
      const content = repository.commits[identity.commit];
      if (
        content === undefined ||
        typeof content !== "object" ||
        content === null ||
        Array.isArray(content)
      ) {
        states.set(referenceId, "failed");
        return {
          status: "failed",
          reason: `Commit "${identity.commit}" has no file content in the fixture.`,
        };
      }
      const files = content as Readonly<Record<string, unknown>>;
      const root = join(await baseDir(), normalizeOriginForPath(identity.origin), identity.commit);
      await mkdir(root, { recursive: true });
      let entries = 0;
      let bytes = 0;
      for (const [relativePath, value] of Object.entries(files)) {
        entries += 1;
        if (entries > REFERENCE_LIMITS.maxManifestEntries) {
          states.set(referenceId, "failed");
          return {
            status: "failed",
            reason: `Commit "${identity.commit}" exceeds ${REFERENCE_LIMITS.maxManifestEntries} files.`,
          };
        }
        if (typeof value !== "string") {
          states.set(referenceId, "failed");
          return {
            status: "failed",
            reason: `Commit "${identity.commit}" file "${relativePath}" is not text content.`,
          };
        }
        if (!isSafeRelativePath(relativePath)) {
          states.set(referenceId, "failed");
          return {
            status: "failed",
            reason: `Commit "${identity.commit}" file "${relativePath}" is not a safe relative path.`,
          };
        }
        bytes += Buffer.byteLength(value, "utf8");
        if (bytes > REFERENCE_LIMITS.maxManifestBytes) {
          states.set(referenceId, "failed");
          return {
            status: "failed",
            reason: `Commit "${identity.commit}" exceeds ${REFERENCE_LIMITS.maxManifestBytes} bytes.`,
          };
        }
        const target = join(root, relativePath);
        await mkdir(dirname(target), { recursive: true });
        await writeFile(target, value);
      }
      states.set(referenceId, "materialized");
      return { status: "materialized", root };
    },
    status(referenceId: ReferenceId): MaterializationStatus {
      return states.get(referenceId) ?? "not-materialized";
    },
  };
}
