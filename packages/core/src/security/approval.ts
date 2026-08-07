import type { ChangePreview } from "./change-preview.js";

export interface ApprovalRequest {
  readonly id: string;
  readonly capability: "workspace.write";
  readonly toolName: string;
  readonly summary: string;
  readonly paths: readonly string[];
  readonly preview: ChangePreview;
}

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
