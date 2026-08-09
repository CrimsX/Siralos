import type {
  PreparedProjectMirror,
  ProjectMirror,
  ProjectMirrorPreparationResult,
  ProjectMirrorRequest,
  ProjectMirrorVerification,
} from "@solaris/core";

export const PROJECT_MIRROR_UNAVAILABLE_MESSAGE =
  "Disposable mirror construction is unavailable: Node offers no directory-relative (openat/mkdirat-style) create primitive, so a same-user process can substitute a verified parent between identity verification and the pathname-based create, and no delete-by-handle primitive, so cleanup cannot be bound to the exact created objects. The mirror would not be guaranteed to contain exactly the approved bytes, and its cleanup could delete a substituted object. Solaris never creates or deletes a mirror at this stage; nothing was created and nothing was deleted.";

/**
 * Disposable project mirror that fails closed.
 *
 * The required invariants — "no mirror operation may create an entry
 * outside the verified private root at any instruction boundary", "the
 * mirror contains exactly the approved bytes", and "cleanup deletes only
 * the exact objects created" — cannot be enforced with Node's
 * pathname-based filesystem API against a same-user adversary: there is no
 * directory-relative create primitive to bind a child create to an exact
 * verified parent object, and no delete-by-handle primitive to bind removal
 * to the exact inspected objects. Rather than weakening the threat model to
 * keep the surface available, this adapter performs ZERO filesystem
 * operations: `isAvailable()` is false, `prepare()` reports a typed
 * unavailable outcome before creating anything, `verify()` reports
 * unavailable, and `destroy()` reports a truthful cleanup failure while
 * preserving anything that exists. The mirror will become available only
 * when a mechanically identity-bound create/delete primitive exists.
 */
export function createProjectMirror(): ProjectMirror {
  return {
    isAvailable(): Promise<boolean> {
      return Promise.resolve(false);
    },
    prepare(_request: ProjectMirrorRequest): Promise<ProjectMirrorPreparationResult> {
      return Promise.resolve({
        status: "unavailable",
        message: PROJECT_MIRROR_UNAVAILABLE_MESSAGE,
      });
    },
    verify(_mirror: PreparedProjectMirror): Promise<ProjectMirrorVerification> {
      return Promise.resolve({
        ok: false,
        reason: "unavailable",
        message: PROJECT_MIRROR_UNAVAILABLE_MESSAGE,
      });
    },
    destroy(_mirror: PreparedProjectMirror) {
      return Promise.resolve({
        ok: false,
        reason: "unavailable" as const,
        message: PROJECT_MIRROR_UNAVAILABLE_MESSAGE,
      });
    },
  };
}
