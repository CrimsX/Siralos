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
  | "network.outbound"
  // Read-only external reference inspection (Stage 3 milestone 5).
  // Bounded external research retrieval (Stage 3 milestone 5): a separate
  // capability so research is independently projectable/gated. Built-in
  // profiles deny it (research stays hidden unless a higher policy
  // explicitly permits it); unlike network.outbound there is no profile
  // constraint — the policy rule is the gate.
  | "reference.inspect"
  | "research.fetch";

export type PermissionRule = "allow" | "ask" | "deny";

export interface CapabilityPolicy {
  readonly rules: Readonly<Record<Capability, PermissionRule>>;
}
