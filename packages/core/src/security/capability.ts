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
  | "research.fetch"
  // Read-only inspection of the installed Solaris runtime itself (Stage 3
  // milestone 6): the `self.*` self-reference tools are read-only docs and
  // grant no mutation authority. Allowed in every built-in profile.
  | "self.inspect";

/** Every capability id, in canonical order (self-reference/drift checks). */
export const CAPABILITY_IDS: readonly Capability[] = [
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
  "reference.inspect",
  "research.fetch",
  "self.inspect",
] as const;

export type PermissionRule = "allow" | "ask" | "deny";

export interface CapabilityPolicy {
  readonly rules: Readonly<Record<Capability, PermissionRule>>;
}
