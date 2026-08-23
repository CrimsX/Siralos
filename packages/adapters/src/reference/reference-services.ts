import type {
  ReferenceAccessPort,
  ReferenceDeclaration,
  ReferenceMaterializerPort,
  ReferenceRegistry,
  ReferenceResolverPort,
  ReferenceTrustClass,
} from "@siralos/core";
import { createReferenceRegistry } from "@siralos/core";
import { realpath } from "node:fs/promises";
import { createReferenceAccess } from "./reference-access.js";
import { createReferenceCacheStore, type ReferenceCacheStore } from "./reference-cache.js";
import {
  createReferenceMaterializer,
  createReferenceRootProvider,
  type RootProvider,
} from "./reference-materializer.js";
import { assertReferenceRoot } from "./reference-path.js";
import {
  createLocalDirectoryResolver,
  createReferenceResolver,
  createRepositoryResolver,
  createUnavailableRepositoryBackend,
} from "./reference-resolver.js";
import {
  createReferenceTools,
  type ReferenceTool,
} from "../tools/reference/reference-list-tool.js";

/**
 * Reference services (Stage 3 milestone 5) — the single assembly point the
 * CLI and behavior harness use to build the reference surface:
 * registry (the SINGLE owner of reference identity), materializer,
 * cache store, access port, and the read-only `reference.*` tools.
 *
 * Defaults are deliberately fail-closed: repository resolution and
 * materialization report `unavailable` (no sandboxed git execution exists
 * at this stage — nothing is spawned, fetched, or created), the cache
 * store is a no-op, and trust defaults to `explicit-user`.
 *
 * Defense in depth: beyond the registry's own workspace-containment
 * checks at resolution/refresh, the root provider re-verifies every
 * local-directory root against the workspace namespace before any access.
 */

export interface ReferenceServicesOptions {
  readonly declarations: readonly ReferenceDeclaration[];
  /** Canonicalized workspace root; local-directory references must stay outside it. */
  readonly workspaceRoot: string;
  readonly trustFor?: (declaration: ReferenceDeclaration) => ReferenceTrustClass;
  readonly resolver?: ReferenceResolverPort;
  readonly materializer?: ReferenceMaterializerPort;
  readonly cacheStore?: ReferenceCacheStore;
  readonly now?: () => number;
}

export interface ReferenceServices {
  readonly registry: ReferenceRegistry;
  readonly materializer: ReferenceMaterializerPort;
  readonly cacheStore: ReferenceCacheStore;
  readonly access: ReferenceAccessPort;
  readonly tools: readonly ReferenceTool[];
  /** Releases nothing at this stage (no background resources are held); kept for interface stability. */
  close(): void;
}

export async function createReferenceServices(
  options: ReferenceServicesOptions,
): Promise<ReferenceServices> {
  const resolver: ReferenceResolverPort =
    options.resolver ??
    createReferenceResolver({
      local: createLocalDirectoryResolver(),
      repository: createRepositoryResolver(createUnavailableRepositoryBackend()),
    });
  const materializer: ReferenceMaterializerPort =
    options.materializer ?? createReferenceMaterializer();
  const cacheStore: ReferenceCacheStore = options.cacheStore ?? createReferenceCacheStore();
  const trustFor = options.trustFor ?? ((): ReferenceTrustClass => "explicit-user");

  // Defense in depth: callers are documented to pass a canonicalized
  // workspace root, but symlinked temp roots (macOS /var ->
  // /private/var) would otherwise let a materialized canonical
  // reference root escape the containment comparison and compare
  // against mixed path forms. Canonicalize here so the outside-the-
  // workspace guarantee cannot be bypassed by an uncanonicalized input.
  const canonicalWorkspaceRoot = await realpath(options.workspaceRoot).catch(
    () => options.workspaceRoot,
  );

  const registry = await createReferenceRegistry({
    declarations: options.declarations,
    trustFor,
    workspaceRoot: canonicalWorkspaceRoot,
    resolver,
    ...(options.now === undefined ? {} : { now: options.now }),
  });

  const rootProvider = createReferenceRootProvider({ materializer });
  const guardedRootProvider: RootProvider = {
    async rootFor(referenceId, identity) {
      const root = await rootProvider.rootFor(referenceId, identity);
      if (root === null || root.kind !== "local-directory") {
        return root;
      }
      try {
        assertReferenceRoot(root.path, canonicalWorkspaceRoot);
      } catch {
        return null;
      }
      return root;
    },
  };

  const access = createReferenceAccess({
    roots: guardedRootProvider,
    referenceInfo: (referenceId) => {
      const reference = registry.get(referenceId);
      if (reference === undefined) {
        return null;
      }
      return { alias: reference.alias, revision: registry.revision(referenceId) };
    },
  });

  const tools = createReferenceTools({ registry, access });

  return {
    registry,
    materializer,
    cacheStore,
    access,
    tools,
    close(): void {
      // Nothing to close: no background resources, handles, or timers are
      // held by the reference services at this stage.
    },
  };
}
