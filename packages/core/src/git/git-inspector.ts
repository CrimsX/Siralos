import type {
  GitDiffScope,
  GitDiffResult,
  GitStatusResult,
  GitWorkspaceStatus,
} from "./git-models.js";

export interface GitStatusRequest {
  readonly signal?: AbortSignal;
}

export interface GitDiffRequest {
  readonly scope: GitDiffScope;
  readonly paths?: readonly string[];
  readonly signal?: AbortSignal;
}

export interface GitInspector {
  inspectRepository(signal?: AbortSignal): Promise<GitWorkspaceStatus>;

  getStatus(request: GitStatusRequest): Promise<GitStatusResult>;

  getDiff(request: GitDiffRequest): Promise<GitDiffResult>;
}
