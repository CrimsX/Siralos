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
          // API knowledge is project-independent, offline, and uses the
          // selected trusted engine; local search/lookup needs no approval.
          "godot.api": "allow",
          // GDScript diagnostics load and parse project source from a
          // disposable mirror, so they stay one-time approved.
          "godot.diagnose": "ask",
          // A Godot LSP session runs a recovery editor against a disposable
          // mirror over loopback; it is one-time approved per session and
          // never unconditionally allowed by public configuration.
          "godot.lsp": "ask",
          // godot.development_status is a read-only bounded workflow-status
          // tool; the development workflow itself is started through the
          // CLI's one-time approval (never through the tool loop), and its
          // source mutations stay gated by workspace.write.
          "godot.development": "allow",
          // Read-only external reference inspection is allowed in every
          // built-in profile; bounded research retrieval is denied
          // everywhere by default (the policy rule is the gate).
          "reference.inspect": "allow",
          "research.fetch": "deny",
          // Read-only self-reference inspection (installed runtime docs) is
          // allowed in every built-in profile; it grants no authority.
          "self.inspect": "allow",
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
          "godot.api": "allow",
          "godot.diagnose": "ask",
          "godot.lsp": "ask",
          "godot.development": "allow",
          "reference.inspect": "allow",
          "research.fetch": "deny",
          "self.inspect": "allow",
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
          // Internal execution profile: never used for Godot tool
          // permission evaluation. There is no public unconditional
          // `allow` for diagnostics anywhere.
          "godot.api": "deny",
          "godot.diagnose": "deny",
          "godot.lsp": "deny",
          "godot.development": "deny",
          "reference.inspect": "allow",
          "research.fetch": "deny",
          "self.inspect": "allow",
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
          "godot.api": "deny",
          "godot.diagnose": "deny",
          "godot.lsp": "deny",
          "godot.development": "deny",
          "reference.inspect": "allow",
          "research.fetch": "deny",
          "self.inspect": "allow",
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
          "godot.api": "deny",
          "godot.diagnose": "deny",
          "godot.lsp": "deny",
          "godot.development": "deny",
          "reference.inspect": "allow",
          "research.fetch": "deny",
          "self.inspect": "allow",
          "workspace.write": "deny",
          "process.execute": "ask",
          "network.outbound": "deny",
        },
      };
    case "godot-diagnostics-offline":
      // Internal execution profile: never user-selectable and never used for
      // tool permission evaluation. Mirrors validation-offline so permission
      // evaluation is total; check-only diagnostics are Solaris-fixed.
      return {
        rules: {
          "workspace.read": "allow",
          "git.inspect": "allow",
          "godot.inspect": "allow",
          "godot.probe_project": "deny",
          "godot.api": "deny",
          "godot.diagnose": "deny",
          "godot.lsp": "deny",
          "godot.development": "deny",
          "reference.inspect": "allow",
          "research.fetch": "deny",
          "self.inspect": "allow",
          "workspace.write": "deny",
          "process.execute": "ask",
          "network.outbound": "deny",
        },
      };
    case "godot-lsp-local":
      // Internal execution profile: never user-selectable and never used for
      // tool permission evaluation. Mirrors validation-offline so permission
      // evaluation is total; the LSP session is Solaris-fixed.
      return {
        rules: {
          "workspace.read": "allow",
          "git.inspect": "allow",
          "godot.inspect": "allow",
          "godot.probe_project": "deny",
          "godot.api": "deny",
          "godot.diagnose": "deny",
          "godot.lsp": "deny",
          "godot.development": "deny",
          "reference.inspect": "allow",
          "research.fetch": "deny",
          "self.inspect": "allow",
          "workspace.write": "deny",
          "process.execute": "ask",
          "network.outbound": "deny",
        },
      };
  }
}
