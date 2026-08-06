export type Capability =
  "workspace.read" | "workspace.write" | "process.execute" | "network.outbound";

export type PermissionRule = "allow" | "ask" | "deny";

export interface CapabilityPolicy {
  readonly rules: Readonly<Record<Capability, PermissionRule>>;
}
