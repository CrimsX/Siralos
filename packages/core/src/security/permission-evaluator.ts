import type { Capability, CapabilityPolicy } from "./capability.js";
import type { SandboxProfile } from "./profile.js";

export type PermissionEvaluation =
  | {
      readonly decision: "allow";
    }
  | {
      readonly decision: "ask";
      readonly reason: string;
    }
  | {
      readonly decision: "deny";
      readonly reason: string;
    };

export function evaluatePermission(
  capability: Capability,
  policy: CapabilityPolicy,
  profile: SandboxProfile,
): PermissionEvaluation {
  const rule = policy.rules[capability];
  if (rule === undefined) {
    return {
      decision: "deny",
      reason: `No permission rule is defined for ${capability}; failing closed.`,
    };
  }
  if (rule === "deny") {
    return { decision: "deny", reason: `Policy denies ${capability}.` };
  }
  const profileIssue = profileConstraintIssue(capability, profile);
  if (profileIssue !== null) {
    return { decision: "deny", reason: profileIssue };
  }
  if (rule === "ask") {
    return { decision: "ask", reason: `Policy requires approval for ${capability}.` };
  }
  return { decision: "allow" };
}

function profileConstraintIssue(capability: Capability, profile: SandboxProfile): string | null {
  switch (capability) {
    case "process.execute":
      if (!profile.process.enabled) {
        return `Profile ${profile.id} does not enable process execution.`;
      }
      return null;
    case "network.outbound":
      return "No built-in sandbox profile enables outbound network access.";
    case "workspace.write":
      if (profile.filesystem.workspaceAccess === "read-only") {
        return `Profile ${profile.id} provides read-only workspace access.`;
      }
      return null;
    case "workspace.read":
      return null;
    case "git.inspect":
      return null;
  }
}
