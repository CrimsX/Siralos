import type { CapabilityPolicy } from "./capability.js";
import type { SandboxProfileId } from "./profile.js";

export function createDefaultPolicy(profileId: SandboxProfileId): CapabilityPolicy {
  switch (profileId) {
    case "inspect":
      return {
        rules: {
          "workspace.read": "allow",
          "git.inspect": "allow",
          "godot.inspect": "allow",
          "godot.probe_project": "ask",
          "workspace.write": "deny",
          "process.execute": "deny",
          "network.outbound": "deny",
        },
      };
    case "develop-offline":
      return {
        rules: {
          "workspace.read": "allow",
          "git.inspect": "allow",
          "godot.inspect": "allow",
          "godot.probe_project": "ask",
          "workspace.write": "ask",
          "process.execute": "ask",
          "network.outbound": "deny",
        },
      };
    case "validation-offline":
      return {
        rules: {
          "workspace.read": "allow",
          "git.inspect": "allow",
          "godot.inspect": "allow",
          "godot.probe_project": "deny",
          "workspace.write": "deny",
          "process.execute": "ask",
          "network.outbound": "deny",
        },
      };
    case "godot-probe-offline":
      // Internal execution profile: never user-selectable and never used for
      // tool permission evaluation. Mirrors validation-offline so permission
      // evaluation is total; probes themselves are Solaris-fixed.
      return {
        rules: {
          "workspace.read": "allow",
          "git.inspect": "allow",
          "godot.inspect": "allow",
          "godot.probe_project": "deny",
          "workspace.write": "deny",
          "process.execute": "ask",
          "network.outbound": "deny",
        },
      };
    case "godot-recovery-probe-offline":
      // Internal execution profile: never user-selectable and never used for
      // tool permission evaluation. Mirrors validation-offline so permission
      // evaluation is total; recovery probes are Solaris-fixed.
      return {
        rules: {
          "workspace.read": "allow",
          "git.inspect": "allow",
          "godot.inspect": "allow",
          "godot.probe_project": "deny",
          "workspace.write": "deny",
          "process.execute": "ask",
          "network.outbound": "deny",
        },
      };
  }
}
