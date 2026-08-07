import type { ChangePreview } from "./change-preview.js";
import type { CommandPreview } from "../commands/command-runners.js";

export interface WorkspaceWriteApprovalRequest {
  readonly id: string;
  readonly capability: "workspace.write";
  readonly toolName: string;
  readonly summary: string;
  readonly paths: readonly string[];
  readonly preview: ChangePreview;
  /**
   * SHA-256 digest over the immutable prepared mutation plan (path,
   * operation, and before/after content hashes). Approval binds to this
   * exact plan; the mutation refuses to apply under any other digest.
   */
  readonly digest: string;
}

export interface ProcessExecutionApprovalRequest {
  readonly id: string;
  readonly capability: "process.execute";
  readonly toolName: string;
  readonly summary: string;
  /** The immutable command preview; approval applies to the digest below. */
  readonly preview: CommandPreview;
  /** Full prepared-command digest; approval binds to this exact plan. */
  readonly digest: string;
}

export type ApprovalRequest = WorkspaceWriteApprovalRequest | ProcessExecutionApprovalRequest;

export type ApprovalDecision =
  | {
      readonly type: "approve_once";
    }
  | {
      readonly type: "deny";
      readonly reason?: string;
    }
  | {
      readonly type: "cancelled";
    };

export interface ApprovalReviewer {
  review(request: ApprovalRequest, signal?: AbortSignal): Promise<ApprovalDecision>;
}

export const DEFAULT_MAX_PENDING_APPROVAL_MS = 10 * 60 * 1000;
