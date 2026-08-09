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
    case "godot.inspect":
      // Godot inspection is fixed, project-independent, read-only, and
      // offline; no profile constraint applies beyond the policy rule.
      return null;
    case "godot.probe_project":
      // The recovery probe is a fixed, Solaris-owned workflow: the probe
      // service itself revalidates the sandbox, the disposable mirror, the
      // engine, and the project manifest before anything runs, and refuses
      // as unavailable when the platform cannot bind execution to the
      // approved bytes. The policy rule (ask in every user-facing profile)
      // is the approval gate; no additional profile constraint applies.
      return null;
    case "godot.api":
      // API knowledge is project-independent, offline, and fixed by
      // Solaris; the knowledge service revalidates the sandbox and the
      // selected engine. No additional profile constraint applies.
      return null;
    case "godot.diagnose":
      // GDScript diagnostics are a fixed, Solaris-owned workflow: the
      // diagnostics service itself revalidates the sandbox, the disposable
      // mirror, the engine, and the project manifest before anything runs,
      // and refuses as unavailable when the platform cannot bind execution
      // to the approved bytes. The policy rule (ask in every user-facing
      // profile) is the approval gate; no additional profile constraint
      // applies.
      return null;
    case "godot.lsp":
      // The GDScript language session is a fixed, Solaris-owned workflow:
      // the language service itself revalidates the sandbox, the loopback
      // LSP channel, the disposable mirror, the engine, and the project
      // manifest before anything runs, and refuses as unavailable when the
      // platform cannot bind the editor launch and the session lifecycle
      // to the approved bytes. The policy rule (ask in every user-facing
      // profile) is the approval gate; no additional profile constraint
      // applies.
      return null;
  }
}
