import type { ChangePreview } from "./change-preview.js";
import type { CommandPreview } from "../commands/command-runners.js";
import type { GodotProbePreview } from "../godot/probe.js";
import type { GodotDiagnosticPreview } from "../godot/gdscript.js";
import type { GDScriptLSPSessionPreview } from "../godot/lsp.js";
import type { GDScriptDevelopmentPreview } from "../godot/development/development-model.js";

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

export interface GodotLSPSessionApprovalRequest {
  readonly id: string;
  readonly capability: "godot.lsp";
  readonly toolName: string;
  readonly summary: string;
  /** The immutable language-session preview; approval applies to the digest below. */
  readonly preview: GDScriptLSPSessionPreview;
  /**
   * SHA-256 over the prepared LSP session: the risk-manifest digest, the
   * executable identity and version, the mirror-copy policy, the LSP
   * capability set, the sandbox profile, the LSP policy version, and the
   * session limits. Approval applies to exactly one session; anything
   * changing before startup is a conflict.
   */
  readonly digest: string;
}

export interface GDScriptDevelopmentApprovalRequest {
  readonly id: string;
  readonly capability: "godot.development";
  readonly toolName: string;
  readonly summary: string;
  /**
   * The immutable development-workflow preview: the request text, the
   * project and engine fingerprints, and the immutable iteration limits.
   * This one-time approval covers the read-only validation context (LSP
   * recreation after approved edits, check-only parsing, API lookup,
   * workspace and Git inspection); every source change set still requires
   * its own exact one-time approval.
   */
  readonly preview: GDScriptDevelopmentPreview;
  /** SHA-256 over the immutable workflow start; approval binds to it. */
  readonly digest: string;
}

/**
 * Plan-approval request (Stage 3 milestone 7). Approving a plan binds the
 * host decision to the EXACT immutable plan revision and TaskContract
 * revision recorded in the request. Plan approval authorizes ONLY the
 * plan's acceptance as the execution reference — it never authorizes
 * source edits, commands, or capabilities; those keep their own exact
 * one-time approval paths.
 */
export interface TaskPlanApprovalRequest {
  readonly id: string;
  readonly capability: "plan.approve";
  readonly toolName: string;
  readonly summary: string;
  readonly planId: string;
  readonly planRevision: number;
  readonly taskContractRevision: number;
  /** SHA-256 over the exact plan revision (approval binds to it). */
  readonly digest: string;
}

export type ApprovalRequest =
  | WorkspaceWriteApprovalRequest
  | ProcessExecutionApprovalRequest
  | GodotProjectProbeApprovalRequest
  | GodotDiagnosticApprovalRequest
  | GodotLSPSessionApprovalRequest
  | GDScriptDevelopmentApprovalRequest
  | TaskPlanApprovalRequest;

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
