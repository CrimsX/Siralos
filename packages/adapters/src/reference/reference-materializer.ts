import {
  type MaterializationOutcome,
  type MaterializationStatus,
  type ReferenceId,
  type ReferenceMaterializerPort,
  type ResolvedReferenceIdentity,
} from "@siralos/core";

/**
 * Reference materializers (Stage 3 milestone 5).
 *
 * `createReferenceMaterializer` is the REAL production materializer:
 * local-directory references need no copy — the directory IS the reference
 * (`materialize` returns `{status:"materialized", root: canonicalPath}`
 * with zero filesystem operations and status "not-required"); repository
 * references fail closed as `unavailable` — materialization requires
 * sandboxed git execution, which is unavailable at this stage, so nothing
 * is ever created or fetched.
 *
 * `createReferenceRootProvider` maps a resolved identity to an accessible
 * root: local-directory identities resolve to their canonical path (the
 * materializer is still consulted so `status()` stays truthful),
 * repository identities go through the materializer. The root is INTERNAL
 * — never model-facing.
 *
 * The test/behavior-harness materializer (`createFakeRepositoryMaterializer`)
 * lives in `./reference-test-support.ts` so the architecture check can
 * allowlist its destructive fs usage without weakening this module.
 */

export const REPOSITORY_MATERIALIZATION_UNAVAILABLE_MESSAGE =
  "repository materialization requires sandboxed git execution, which is unavailable at this stage";

export interface CreateReferenceMaterializerOptions {
  /**
   * Reserved for API symmetry: local-directory materialization is a no-op
   * that returns the identity's canonical path, so no root is ever used.
   */
  readonly localRoot?: string;
}

export function createReferenceMaterializer(
  _options: CreateReferenceMaterializerOptions = {},
): ReferenceMaterializerPort {
  const states = new Map<ReferenceId, MaterializationStatus>();
  return {
    materialize(
      referenceId: ReferenceId,
      identity: ResolvedReferenceIdentity,
    ): Promise<MaterializationOutcome> {
      if (identity.kind === "local-directory") {
        // No copy: the directory IS the reference. Zero filesystem
        // operations are performed.
        states.set(referenceId, "not-required");
        return Promise.resolve({ status: "materialized", root: identity.canonicalPath });
      }
      states.set(referenceId, "unavailable");
      return Promise.resolve({
        status: "unavailable",
        reason: REPOSITORY_MATERIALIZATION_UNAVAILABLE_MESSAGE,
      });
    },
    status(referenceId: ReferenceId): MaterializationStatus {
      return states.get(referenceId) ?? "not-materialized";
    },
  };
}

export interface ReferenceRoot {
  readonly path: string;
  readonly kind: "local-directory" | "repository";
}

export interface RootProvider {
  rootFor(
    referenceId: ReferenceId,
    identity: ResolvedReferenceIdentity,
  ): Promise<ReferenceRoot | null>;
}

export function createReferenceRootProvider(options: {
  readonly materializer: ReferenceMaterializerPort;
}): RootProvider {
  return {
    async rootFor(
      referenceId: ReferenceId,
      identity: ResolvedReferenceIdentity,
    ): Promise<ReferenceRoot | null> {
      // The materializer is consulted for BOTH kinds so its status() stays
      // truthful; local-directory materialization is a zero-fs no-op.
      const outcome = await options.materializer.materialize(referenceId, identity);
      if (outcome.status !== "materialized") {
        return null;
      }
      return { path: outcome.root, kind: identity.kind };
    },
  };
}
