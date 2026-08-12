import type { CommandRunPaths } from "@siralos/core";

export type { CommandRunPaths };

export interface RunDirectoryProviderOptions {
  readonly workspaceRoot: string;
  /** Siralos-owned runs root; defaults to `~/.siralos/runs`. */
  readonly runsRoot?: string;
}

export type RunCleanupOutcome =
  | {
      readonly ok: true;
    }
  | {
      readonly ok: false;
      readonly reason: "unavailable" | "refused" | "failed";
      readonly message: string;
    };

export type RunCreateOutcome =
  | {
      readonly ok: true;
      readonly paths: CommandRunPaths;
    }
  | {
      readonly ok: false;
      readonly reason: "unavailable";
      readonly message: string;
    };

export interface RunDirectoryProvider {
  create(): Promise<RunCreateOutcome>;
  remove(runId: string): Promise<RunCleanupOutcome>;
}

export const RUN_DIRECTORY_CREATION_UNAVAILABLE_MESSAGE =
  "Private run-directory creation is unavailable: Node offers no directory-relative (openat/mkdirat-style) or handle-relative primitive, so a same-user process can substitute a verified parent between identity verification and the pathname-based create, placing a new entry outside the intended verified root. Siralos never creates run directories at this stage; nothing was created.";

export const RUN_DIRECTORY_CLEANUP_UNAVAILABLE_MESSAGE =
  "Run-directory cleanup is unavailable: without a delete-by-handle or directory-relative primitive, removal cannot be bound to the exact objects inspected and accepted in the removal transaction, and a substituted root, child, or leaf could be deleted by pathname. Nothing is deleted; any existing run directory is preserved for manual inspection.";

/**
 * Private run-directory provider that fails closed.
 *
 * Creation and cleanup are both UNAVAILABLE. The required invariants —
 * "no run-directory operation may create an entry outside the intended
 * verified root at any instruction boundary" and "cleanup may delete only
 * the exact objects inspected and accepted in the removal transaction" —
 * cannot be enforced with Node's pathname-based filesystem API against a
 * same-user adversary: there is no openat/mkdirat-style primitive to bind a
 * child create to an exact verified parent object, and no delete-by-handle
 * primitive to bind removal to the exact inspected objects. Rather than
 * weakening the threat model to keep the surface available, this provider
 * performs ZERO filesystem operations: `create()` reports a typed
 * unavailable outcome before creating anything (nothing is created and no
 * cleanup is attempted afterwards), and `remove()` reports a truthful
 * cleanup failure while preserving anything that exists. Every dependent
 * capability (sandboxed Git, command execution, Godot probing, conformance
 * workflows) must fail closed on the unavailable outcome; the provider will
 * become available only when a mechanically identity-bound create/delete
 * primitive exists.
 */
export function createRunDirectoryProvider(
  _options: RunDirectoryProviderOptions,
): RunDirectoryProvider {
  return {
    create(): Promise<RunCreateOutcome> {
      return Promise.resolve({
        ok: false,
        reason: "unavailable",
        message: RUN_DIRECTORY_CREATION_UNAVAILABLE_MESSAGE,
      });
    },
    remove(_runId: string): Promise<RunCleanupOutcome> {
      return Promise.resolve({
        ok: false,
        reason: "unavailable",
        message: RUN_DIRECTORY_CLEANUP_UNAVAILABLE_MESSAGE,
      });
    },
  };
}
