import {
  REFERENCE_LIMITS,
  createReferenceId,
  validateReferenceAlias,
  type ReferenceLimits,
} from "./reference-model.js";
import type {
  Reference,
  ReferenceAlias,
  ReferenceId,
  ReferenceRevision,
  ReferenceStatus,
  ReferenceTaskBinding,
  ReferenceTrustClass,
  ResolvedReferenceIdentity,
} from "./reference-model.js";
import type { ReferenceDeclaration } from "./reference-declaration.js";
import type { ReferenceResolverPort, ReferenceResolutionOutcome } from "./reference-ports.js";

/**
 * ReferenceRegistry (Stage 3 milestone 5) — the SINGLE application-owned
 * owner of reference identity.
 *
 * The registry parses nothing (declarations arrive pre-parsed), resolves
 * every declared reference at creation through the resolver port, records
 * immutable revisions, and exposes the ONLY way a revision changes:
 * `refresh`. CLI, provider adapters, ContextProjector, and EvidenceProjector
 * never resolve or refresh references themselves.
 *
 * Revisions are immutable values; a refresh REPLACES the current revision
 * and old revisions remain reachable through task bindings and evidence.
 * A failed refresh invalidates the current revision (fail closed — a stale
 * identity is never served silently; there is no silent branch advance).
 * Declined/unresolvable references remain listed with a precise
 * `failureReason` so the reference configuration stays visible/auditable.
 *
 * Task bindings (`bindTask`) snapshot the current revisions at task start;
 * bindings are bounded by `maxRevisionBindings` with FIFO eviction,
 * mirroring the watermark-cache pattern.
 */

export type ReferenceRefreshResult =
  | { readonly status: "refreshed"; readonly revision: ReferenceRevision }
  | { readonly status: "unchanged"; readonly revision: ReferenceRevision }
  | { readonly status: "unavailable" | "refused" | "failed"; readonly reason: string };

export interface ReferenceRegistryOptions {
  readonly declarations: readonly ReferenceDeclaration[];
  /** Classify each declaration's trust (host policy input). */
  readonly trustFor: (declaration: ReferenceDeclaration) => ReferenceTrustClass;
  /** Canonicalized workspace root; local-directory references must stay outside it. */
  readonly workspaceRoot: string;
  readonly resolver: ReferenceResolverPort;
  /**
   * When false (default), mutable repository refs (branch / absent ref)
   * are refused with a precise reason; only pinned commits/tags resolve.
   */
  readonly allowMutableRefs?: boolean;
  readonly now?: () => number;
  readonly limits?: Partial<ReferenceLimits>;
}

export interface ReferenceRegistry {
  /** All declared references, in declaration order (declined ones included). */
  list(): readonly Reference[];
  get(selector: ReferenceAlias | ReferenceId): Reference | undefined;
  /** The registry's CURRENT revision; never changes except via `refresh`. */
  revision(selector: ReferenceAlias | ReferenceId): ReferenceRevision | null;
  /** Snapshot current revisions for all ready references (immutable). */
  bindTask(taskId: string): ReferenceTaskBinding;
  boundRevision(
    binding: ReferenceTaskBinding,
    selector: ReferenceAlias | ReferenceId,
  ): ReferenceRevision | null;
  /** The ONLY way a reference revision changes. */
  refresh(selector: ReferenceAlias | ReferenceId): Promise<ReferenceRefreshResult>;
  /** Precise reason a declaration was declined; null otherwise. */
  declineReason(selector: ReferenceAlias | ReferenceId): string | null;
  readonly size: number;
}

interface ReferenceRecord {
  readonly id: ReferenceId;
  readonly alias: ReferenceAlias;
  reference: Reference;
  revision: ReferenceRevision | null;
}

const MUTABLE_REF_REFUSAL = "mutable repository ref requires an explicit pinned commit/tag";
const WORKSPACE_CONTAINMENT_REFUSAL = "reference root must be outside the workspace namespace";

/**
 * Pure path containment check with `path.resolve`-like semantics for
 * absolute paths, without importing Node (core is Node-module-free).
 *
 * Both inputs are normalized (separators unified, `.`/`..` resolved
 * lexically, trailing slashes dropped) and compared with a separator
 * boundary. Windows-form paths (drive letters / UNC) compare
 * case-insensitively, matching Windows semantics; POSIX paths compare
 * case-sensitively. The inputs are expected to be canonicalized by the
 * resolver/CLI; the normalization is defense in depth.
 */
export function isPathWithin(root: string, target: string): boolean {
  const rootNorm = normalizeAbsolutePath(root);
  const targetNorm = normalizeAbsolutePath(target);
  // Relative inputs would anchor to the process CWD — never meaningful for
  // containment. Fail closed (not within) rather than guess.
  if (!isAbsoluteForm(rootNorm) || !isAbsoluteForm(targetNorm)) {
    return false;
  }
  if (isWindowsForm(rootNorm) && isWindowsForm(targetNorm)) {
    const rootLower = rootNorm.toLowerCase();
    return (
      targetNorm.toLowerCase() === rootLower || targetNorm.toLowerCase().startsWith(`${rootLower}/`)
    );
  }
  return targetNorm === rootNorm || targetNorm.startsWith(`${rootNorm}/`);
}

function isAbsoluteForm(path: string): boolean {
  return path.startsWith("/") || isWindowsForm(path);
}

function isWindowsForm(path: string): boolean {
  return /^[A-Za-z]:\//.test(path) || path.startsWith("//");
}

function normalizeAbsolutePath(path: string): string {
  const collapsed = path.replace(/\\/g, "/").replace(/\/+/g, "/");
  let prefix = "";
  let body = collapsed;
  if (/^[A-Za-z]:\//.test(collapsed)) {
    prefix = collapsed.slice(0, 2); // "C:"
    body = collapsed.slice(2); // "/Users/x"
  } else if (collapsed.startsWith("//")) {
    prefix = "//"; // UNC "//server/share"
    body = collapsed.slice(2);
  }
  const segments = body.split("/");
  const out: string[] = [];
  for (const segment of segments) {
    if (segment === "" || segment === ".") {
      continue;
    }
    if (segment === "..") {
      out.pop(); // clamp at the root: ".." above "/" stays "/"
      continue;
    }
    out.push(segment);
  }
  return prefix === "//" ? `${prefix}${out.join("/")}` : `${prefix}/${out.join("/")}`;
}

function identitiesEqual(a: ResolvedReferenceIdentity, b: ResolvedReferenceIdentity): boolean {
  if (a.kind !== b.kind) {
    return false;
  }
  if (a.kind === "local-directory") {
    return (
      b.kind === "local-directory" &&
      a.canonicalPath === b.canonicalPath &&
      a.fingerprint === b.fingerprint
    );
  }
  return b.kind === "repository" && a.origin === b.origin && a.commit === b.commit;
}

/** Map a resolver outcome to a reference status (total, fail-closed). */
function outcomeToStatus(outcome: ReferenceResolutionOutcome): {
  readonly status: ReferenceStatus;
  readonly reason: string;
} {
  switch (outcome.status) {
    case "resolved":
      return { status: "ready", reason: "" };
    case "refused":
      return { status: "declined", reason: outcome.reason };
    case "unavailable":
      return { status: "unavailable", reason: outcome.reason };
    case "failed":
      return { status: "resolution-failed", reason: outcome.reason };
  }
}

async function mapBounded<T, R>(
  items: readonly T[],
  limit: number,
  fn: (item: T) => Promise<R>,
): Promise<R[]> {
  const results = new Array<R>(items.length);
  let next = 0;
  async function worker(): Promise<void> {
    for (;;) {
      const index = next;
      next += 1;
      if (index >= items.length) {
        return;
      }
      results[index] = await fn(items[index] as T);
    }
  }
  const workers = Array.from({ length: Math.min(limit, items.length) }, () => worker());
  await Promise.all(workers);
  return results;
}

export async function createReferenceRegistry(
  options: ReferenceRegistryOptions,
): Promise<ReferenceRegistry> {
  const now = options.now ?? Date.now;
  const allowMutableRefs = options.allowMutableRefs ?? false;
  const limits = { ...REFERENCE_LIMITS, ...options.limits };
  // Containment compares the resolver's REALPATH'd identity against the
  // workspace namespace, so the namespace itself must be canonical:
  // symlinked temp roots (macOS /var -> /private/var) would otherwise
  // make a workspace-inside reference read as outside.
  const { realpath } = await import("node:fs/promises");
  const canonicalWorkspaceRoot = await realpath(options.workspaceRoot).catch(
    () => options.workspaceRoot,
  );
  const records: ReferenceRecord[] = [];
  const byAlias = new Map<string, ReferenceRecord>();
  const byId = new Map<ReferenceId, ReferenceRecord>();
  const bindings: ReferenceTaskBinding[] = [];

  function resolveRecord(selector: ReferenceAlias | ReferenceId): ReferenceRecord | null {
    const byAliasEntry = byAlias.get(selector);
    if (byAliasEntry !== undefined) {
      return byAliasEntry;
    }
    return byId.get(selector as ReferenceId) ?? null;
  }

  /**
   * Resolve one declaration into a (reference, revision) pair, applying
   * registry-level policy BEFORE the resolver: mutable repository refs are
   * refused without calling the resolver, and resolved local-directory
   * identities are checked against the workspace namespace.
   */
  async function resolveDeclaration(
    declaration: ReferenceDeclaration,
  ): Promise<{ reference: Reference; revision: ReferenceRevision | null }> {
    const id = createReferenceId(declaration.alias);
    const trust = options.trustFor(declaration);
    if (validateReferenceAlias(declaration.alias) === null) {
      return {
        reference: {
          id,
          alias: declaration.alias as ReferenceAlias,
          kind: declaration.kind,
          source: declaration.source,
          trust,
          description: declaration.description,
          status: "declined",
          failureReason: "invalid alias",
        },
        revision: null,
      };
    }
    if (
      declaration.source.kind === "repository" &&
      declaration.source.ref.kind === "branch" &&
      !allowMutableRefs
    ) {
      return {
        reference: {
          id,
          alias: declaration.alias as ReferenceAlias,
          kind: declaration.kind,
          source: declaration.source,
          trust,
          description: declaration.description,
          status: "declined",
          failureReason: MUTABLE_REF_REFUSAL,
        },
        revision: null,
      };
    }
    const outcome = await options.resolver.resolveIdentity(declaration.source, {
      allowMutableRefs,
    });
    if (outcome.status === "resolved") {
      if (
        declaration.source.kind === "local-directory" &&
        outcome.identity.kind === "local-directory" &&
        isPathWithin(canonicalWorkspaceRoot, outcome.identity.canonicalPath)
      ) {
        return {
          reference: {
            id,
            alias: declaration.alias as ReferenceAlias,
            kind: declaration.kind,
            source: declaration.source,
            trust,
            description: declaration.description,
            status: "declined",
            failureReason: WORKSPACE_CONTAINMENT_REFUSAL,
          },
          revision: null,
        };
      }
      return {
        reference: {
          id,
          alias: declaration.alias as ReferenceAlias,
          kind: declaration.kind,
          source: declaration.source,
          trust,
          description: declaration.description,
          status: "ready",
          failureReason: null,
        },
        revision: { identity: outcome.identity, resolvedAtMs: now() },
      };
    }
    const mapped = outcomeToStatus(outcome);
    return {
      reference: {
        id,
        alias: declaration.alias as ReferenceAlias,
        kind: declaration.kind,
        source: declaration.source,
        trust,
        description: declaration.description,
        status: mapped.status,
        failureReason: mapped.reason,
      },
      revision: null,
    };
  }

  // Resolve every declaration in parallel with bounded concurrency, then
  // record outcomes in declaration order. Duplicate aliases are declined:
  // the first occurrence wins and stays addressable (both records share the
  // same alias AND the same derived id, so the duplicate must not enter the
  // lookup maps — it is still LISTED with status "declined" for audit).
  const outcomes = await mapBounded(options.declarations, 4, resolveDeclaration);
  const seenAliases = new Set<string>();
  for (let index = 0; index < outcomes.length; index += 1) {
    const declaration = options.declarations[index] as ReferenceDeclaration;
    const resolved = outcomes[index] as {
      reference: Reference;
      revision: ReferenceRevision | null;
    };
    const duplicate = seenAliases.has(declaration.alias);
    seenAliases.add(declaration.alias);
    const record: ReferenceRecord = {
      id: resolved.reference.id,
      alias: resolved.reference.alias,
      reference: duplicate
        ? Object.freeze({
            ...resolved.reference,
            status: "declined",
            failureReason: "duplicate alias",
          })
        : Object.freeze(resolved.reference),
      revision: duplicate ? null : resolved.revision,
    };
    records.push(record);
    if (!duplicate) {
      byAlias.set(record.alias, record);
      byId.set(record.id, record);
    }
  }

  function evictBindingsIfNeeded(): void {
    while (bindings.length > limits.maxRevisionBindings) {
      bindings.shift();
    }
  }

  return {
    list(): readonly Reference[] {
      return records.map((record) => record.reference);
    },

    get(selector: ReferenceAlias | ReferenceId): Reference | undefined {
      return resolveRecord(selector)?.reference;
    },

    revision(selector: ReferenceAlias | ReferenceId): ReferenceRevision | null {
      return resolveRecord(selector)?.revision ?? null;
    },

    bindTask(taskId: string): ReferenceTaskBinding {
      const revisions = new Map<ReferenceId, ReferenceRevision>();
      for (const record of records) {
        if (record.reference.status === "ready" && record.revision !== null) {
          revisions.set(record.id, record.revision);
        }
      }
      const binding: ReferenceTaskBinding = {
        taskId,
        revisions,
        boundAtMs: now(),
      };
      bindings.push(binding);
      evictBindingsIfNeeded();
      return binding;
    },

    boundRevision(
      binding: ReferenceTaskBinding,
      selector: ReferenceAlias | ReferenceId,
    ): ReferenceRevision | null {
      // Evicted bindings are no longer authoritative: an evicted binding
      // must not keep serving its task-start revisions.
      if (!bindings.includes(binding)) {
        return null;
      }
      const record = resolveRecord(selector);
      if (record === null) {
        return null;
      }
      return binding.revisions.get(record.id) ?? null;
    },

    async refresh(selector: ReferenceAlias | ReferenceId): Promise<ReferenceRefreshResult> {
      const record = resolveRecord(selector);
      if (record === null) {
        return { status: "failed", reason: `Unknown reference: ${String(selector)}` };
      }
      const current = record.reference;
      if (current.status === "declined") {
        // Host-declined declarations (duplicate alias, workspace containment,
        // mutable-ref refusal) cannot refresh; the declaration itself is the
        // problem.
        return {
          status: "refused",
          reason: current.failureReason ?? "the reference is declined",
        };
      }
      const outcome = await options.resolver.resolveIdentity(current.source, {
        allowMutableRefs,
      });
      if (outcome.status === "resolved") {
        if (
          current.source.kind === "local-directory" &&
          outcome.identity.kind === "local-directory" &&
          isPathWithin(canonicalWorkspaceRoot, outcome.identity.canonicalPath)
        ) {
          record.reference = Object.freeze({
            ...current,
            status: "declined",
            failureReason: WORKSPACE_CONTAINMENT_REFUSAL,
          });
          record.revision = null;
          return { status: "refused", reason: WORKSPACE_CONTAINMENT_REFUSAL };
        }
        const revision: ReferenceRevision = { identity: outcome.identity, resolvedAtMs: now() };
        const currentRevision = record.revision;
        if (
          currentRevision !== null &&
          identitiesEqual(currentRevision.identity, revision.identity)
        ) {
          record.reference = Object.freeze({ ...current, status: "ready", failureReason: null });
          return { status: "unchanged", revision: currentRevision };
        }
        record.revision = revision;
        record.reference = Object.freeze({ ...current, status: "ready", failureReason: null });
        return { status: "refreshed", revision };
      }
      const mapped = outcomeToStatus(outcome);
      // Fail closed: a failed refresh invalidates the current revision.
      // Historical revisions stay reachable through task bindings/evidence.
      record.reference = Object.freeze({
        ...current,
        status: mapped.status,
        failureReason: mapped.reason,
      });
      record.revision = null;
      if (outcome.status === "refused") {
        return { status: "refused", reason: mapped.reason };
      }
      if (outcome.status === "unavailable") {
        return { status: "unavailable", reason: mapped.reason };
      }
      return { status: "failed", reason: mapped.reason };
    },

    declineReason(selector: ReferenceAlias | ReferenceId): string | null {
      const record = resolveRecord(selector);
      if (record === null || record.reference.status !== "declined") {
        return null;
      }
      return record.reference.failureReason;
    },

    get size(): number {
      return records.length;
    },
  };
}
