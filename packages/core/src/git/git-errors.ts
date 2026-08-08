export type GitErrorCode =
  | "git_unavailable"
  | "git_not_repository"
  | "git_root_mismatch"
  | "git_status_failed"
  | "git_diff_failed"
  | "git_cancelled"
  | "git_timeout"
  | "git_parse_failed";

export class GitError extends Error {
  readonly code: GitErrorCode;
  readonly detail: unknown;

  constructor(code: GitErrorCode, message: string, detail: unknown = undefined) {
    super(message);
    this.name = "GitError";
    this.code = code;
    this.detail = detail;
  }
}

export function describeGitError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unknown Git failure occurred.";
}
