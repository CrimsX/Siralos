import type { CapabilityPolicy } from "./capability.js";
import type { SandboxProfileId } from "./profile.js";

export function createDefaultPolicy(profileId: SandboxProfileId): CapabilityPolicy {
  switch (profileId) {
    case "inspect":
      return {
        rules: {
          "workspace.read": "allow",
          "git.inspect": "allow",
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
          "workspace.write": "ask",
          "process.execute": "allow",
          "network.outbound": "deny",
        },
      };
  }
}
