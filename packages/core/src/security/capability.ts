export type Capability =
  | "workspace.read"
  | "workspace.write"
  | "git.inspect"
  | "godot.inspect"
  | "godot.probe_project"
  | "godot.api"
  | "godot.diagnose"
  | "godot.lsp"
  | "godot.development"
  | "process.execute"
  | "network.outbound";

export type PermissionRule = "allow" | "ask" | "deny";

export interface CapabilityPolicy {
  readonly rules: Readonly<Record<Capability, PermissionRule>>;
}
