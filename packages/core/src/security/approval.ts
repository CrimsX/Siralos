import type { ChangePreview } from "./change-preview.js";
import type { CommandPreview } from "../commands/command-runners.js";
import type { GodotProbePreview } from "../godot/probe.js";
import type { GodotDiagnosticPreview } from "../godot/gdscript.js";

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

export interface GodotProjectProbeApprovalRequest {
  readonly id: string;
  readonly capability: "godot.probe_project";
  readonly toolName: string;
  readonly summary: string;
  /** The immutable recovery-probe preview; approval applies to the digest below. */
  readonly preview: GodotProbePreview;
  /**
   * SHA-256 over the prepared recovery probe: the risk-manifest digest, the
   * fixed recovery command, the mirror-copy policy version, the sandbox
   * profile, and the probe limits. Approval binds to this exact plan;
   * anything changing before execution is a conflict.
   */
  readonly digest: string;
}

export interface GodotDiagnosticApprovalRequest {
  readonly id: string;
  readonly capability: "godot.diagnose";
  readonly toolName: string;
  readonly summary: string;
  /** The immutable check-only preview; approval applies to the digest below. */
  readonly preview: GodotDiagnosticPreview;
  /**
   * SHA-256 over the prepared GDScript check: the script targets (paths and
   * content hashes), the risk-manifest digest, the fixed check-only
   * command, the sandbox profile, and the check limits. Approval binds to
   * this exact plan; anything changing before execution is a conflict.
   */
  readonly digest: string;
}

export type ApprovalRequest =
  | WorkspaceWriteApprovalRequest
  | ProcessExecutionApprovalRequest
  | GodotProjectProbeApprovalRequest
  | GodotDiagnosticApprovalRequest;

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
