import type { GitInspector, Tool, ToolExecutionContext, ToolExecutionResult } from "@siralos/core";
import { GitError } from "@siralos/core";
import { errorMessage } from "../../support/error-message.js";

export function createGitStatusTool(git: GitInspector): Tool {
  return {
    definition: {
      name: "git.status",
      description: "Show structured Git repository status.",
      inputSchema: {
        type: "object",
        additionalProperties: false,
      },
    },
    capability: "git.inspect",
    async execute(_input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      try {
        const result = await git.getStatus({
          ...(context.signal === undefined ? {} : { signal: context.signal }),
        });
        return {
          status: "success",
          output: {
            repository: result.repository,
            branch: {
              head: result.branch.head,
              oid: result.branch.oid,
              upstream: result.branch.upstream,
              ahead: result.branch.ahead,
              behind: result.branch.behind,
              detached: result.branch.detached,
              unborn: result.branch.unborn,
            },
            changes: result.changes.map((change) => ({
              path: change.path,
              originalPath: change.originalPath,
              indexStatus: change.indexStatus,
              worktreeStatus: change.worktreeStatus,
              kind: change.kind,
            })),
            conflicts: result.conflicts.map((conflict) => ({
              path: conflict.path,
              stage1Oid: conflict.stage1Oid,
              stage2Oid: conflict.stage2Oid,
              stage3Oid: conflict.stage3Oid,
            })),
            untracked: [...result.untracked],
            truncated: result.truncated,
          },
          summary: `${result.changes.length} changed files, ${result.untracked.length} untracked`,
        };
      } catch (error: unknown) {
        if (error instanceof GitError) {
          return { status: "failed", message: error.message };
        }
        return {
          status: "failed",
          message: errorMessage(error, "An unknown Git inspection failure occurred."),
        };
      }
    },
  };
}
