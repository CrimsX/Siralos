import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import type { Capability, CapabilityPolicy } from "../security/capability.js";
import { evaluatePermission } from "../security/permission-evaluator.js";
import type { SandboxProfile } from "../security/profile.js";
import type { RegisteredToolInfo } from "../tools/tool-registry.js";
import type { ToolDefinition } from "../tools/tool.js";

/**
 * Provider-neutral ToolProjector (Stage 3 milestone 2).
 *
 * Decides which registered tools become visible to the current model
 * session. Visibility is a projection of (task mode ∩ capability policy ∩
 * provider capability) — it is NOT the final security boundary. Every
 * invocation still passes the runtime capability policy, approval system,
 * scope checks, and sandbox requirements; a bug here must never grant real
 * authority.
 *
 *   available — visible and invocable under existing policy
 *   gated     — visible, but invocation requires approval/host decision
 *   hidden    — absent from the model-facing schema entirely (distinct
 *               from returning "permission denied")
 */

export type ToolVisibility = "available" | "gated" | "hidden";

export interface ProjectedTool {
  readonly name: string;
  readonly visibility: ToolVisibility;
  readonly description: string;
  readonly inputSchema: unknown;
}

export interface ToolProjection {
  /** Stable session ABI identity over the projected tool schemas. */
  readonly fingerprint: string;
  readonly tools: readonly ProjectedTool[];
  readonly counts: { readonly available: number; readonly gated: number; readonly hidden: number };
  /** Definitions to embed in the provider request (available + gated). */
  readonly requestTools: readonly ToolDefinition[];
}

export interface ToolProjectionInput {
  readonly mode: ProjectionMode;
  readonly registeredTools: readonly RegisteredToolInfo[];
  /** Provider capability metadata; undefined means tool calling is supported. */
  readonly providerToolCalling?: boolean;
}

/**
 * Task-context modes. `generic` is the unrestricted session surface; the
 * workflow modes project only the tools their workflow actually needs.
 */
export type ProjectionMode = "generic" | "development" | "review" | "inspection";

/** Capabilities each mode may expose at most. */
const MODE_CAPABILITY_ALLOWLIST: Readonly<Record<ProjectionMode, readonly Capability[]>> = {
  generic: [
    "workspace.read",
    "workspace.write",
    "git.inspect",
    "godot.inspect",
    "godot.probe_project",
    "godot.api",
    "godot.diagnose",
    "godot.lsp",
    "godot.development",
    "process.execute",
    "network.outbound",
  ],
  development: [
    "workspace.read",
    "workspace.write",
    "git.inspect",
    "godot.inspect",
    "godot.api",
    "godot.development",
  ],
  review: ["workspace.read", "git.inspect", "godot.inspect", "godot.api", "godot.lsp"],
  inspection: ["workspace.read", "git.inspect", "godot.inspect", "godot.api"],
};

/** Exact tool names a mode may expose (narrower than the capability). */
const MODE_TOOL_ALLOWLIST: Readonly<Record<ProjectionMode, readonly string[]>> = {
  generic: [],
  development: [
    "workspace.read",
    "workspace.search",
    "workspace.apply_text_changeset",
    "godot.development_status",
    "godot.inspect_engine",
    "godot.inspect_project",
    "godot.api_search",
    "godot.api_lookup",
  ],
  review: [
    "workspace.list",
    "workspace.read",
    "workspace.search",
    "git.status",
    "git.diff",
    "godot.inspect_project",
    "godot.api_search",
    "godot.api_lookup",
    "godot.lsp_diagnostics",
    "godot.hover",
    "godot.definition",
  ],
  inspection: [
    "workspace.list",
    "workspace.read",
    "workspace.search",
    "godot.inspect_engine",
    "godot.inspect_project",
    "godot.api_search",
    "godot.api_lookup",
  ],
};

export interface ToolProjectorOptions {
  readonly policy: CapabilityPolicy;
  readonly profile: SandboxProfile;
}

export interface ToolProjector {
  project(input: ToolProjectionInput): ToolProjection;
}

/** A tool that fails the mode allowlist is hidden regardless of policy. */
function modeAllows(mode: ProjectionMode, info: RegisteredToolInfo): boolean {
  const exact = MODE_TOOL_ALLOWLIST[mode];
  if (exact.length > 0) {
    return exact.includes(info.definition.name);
  }
  return MODE_CAPABILITY_ALLOWLIST[mode].includes(info.capability);
}

export function createToolProjector(options: ToolProjectorOptions): ToolProjector {
  return {
    project(input: ToolProjectionInput): ToolProjection {
      const projected: ProjectedTool[] = [];
      for (const info of input.registeredTools) {
        let visibility: ToolVisibility;
        if (!modeAllows(input.mode, info)) {
          visibility = "hidden";
        } else {
          const permission = evaluatePermission(info.capability, options.policy, options.profile);
          if (permission.decision === "deny") {
            visibility = "hidden";
          } else if (permission.decision === "ask") {
            visibility = "gated";
          } else {
            visibility = "available";
          }
        }
        projected.push({
          name: info.definition.name,
          visibility,
          description: info.definition.description,
          inputSchema: info.definition.inputSchema,
        });
      }
      const visible = projected.filter((tool) => tool.visibility !== "hidden");
      const fingerprint = sha256Hex(
        canonicalizeJson(
          visible.map((tool) => ({
            name: tool.name,
            description: tool.description,
            inputSchema: tool.inputSchema,
          })),
        ),
      );
      return {
        fingerprint,
        tools: projected,
        counts: {
          available: projected.filter((tool) => tool.visibility === "available").length,
          gated: projected.filter((tool) => tool.visibility === "gated").length,
          hidden: projected.filter((tool) => tool.visibility === "hidden").length,
        },
        requestTools: visible.map((tool) => ({
          name: tool.name,
          description: tool.description,
          inputSchema: tool.inputSchema as never,
        })),
      };
    },
  };
}
