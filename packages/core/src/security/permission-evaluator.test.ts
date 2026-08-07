import { describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  DEVELOP_OFFLINE_PROFILE,
  evaluatePermission,
  INSPECT_PROFILE,
  type Capability,
  type CapabilityPolicy,
} from "../index.js";

function policyWith(
  overrides: Partial<Record<Capability, "allow" | "ask" | "deny">>,
): CapabilityPolicy {
  const base = createDefaultPolicy("inspect");
  return { rules: { ...base.rules, ...overrides } };
}

function policyWithout(capability: Capability): CapabilityPolicy {
  const rules = { ...createDefaultPolicy("inspect").rules };
  delete rules[capability];
  return { rules };
}

describe("built-in profiles", () => {
  it("defines the inspect profile capabilities", () => {
    expect(INSPECT_PROFILE).toMatchObject({
      id: "inspect",
      filesystem: { workspaceAccess: "read-only" },
      process: { enabled: false },
      network: { outbound: "deny" },
      environment: { policy: "minimal" },
    });
  });

  it("defines the develop-offline profile capabilities", () => {
    expect(DEVELOP_OFFLINE_PROFILE).toMatchObject({
      id: "develop-offline",
      filesystem: { workspaceAccess: "read-write" },
      process: { enabled: true },
      network: { outbound: "deny" },
      environment: { policy: "minimal" },
    });
  });
});

describe("evaluatePermission", () => {
  const inspectPolicy = createDefaultPolicy("inspect");
  const developPolicy = createDefaultPolicy("develop-offline");

  it("allows workspace reads under inspect", () => {
    expect(evaluatePermission("workspace.read", inspectPolicy, INSPECT_PROFILE)).toEqual({
      decision: "allow",
    });
  });

  it("denies workspace writes under inspect", () => {
    expect(evaluatePermission("workspace.write", inspectPolicy, INSPECT_PROFILE)).toMatchObject({
      decision: "deny",
    });
  });

  it("denies process execution under inspect", () => {
    expect(evaluatePermission("process.execute", inspectPolicy, INSPECT_PROFILE)).toMatchObject({
      decision: "deny",
    });
  });

  it("denies network access under inspect", () => {
    expect(evaluatePermission("network.outbound", inspectPolicy, INSPECT_PROFILE)).toMatchObject({
      decision: "deny",
    });
  });

  it("asks for approval of workspace writes under develop-offline", () => {
    expect(evaluatePermission("workspace.write", developPolicy, DEVELOP_OFFLINE_PROFILE)).toEqual({
      decision: "ask",
      reason: "Policy requires approval for workspace.write.",
    });
  });

  it("allows process execution under develop-offline", () => {
    expect(evaluatePermission("process.execute", developPolicy, DEVELOP_OFFLINE_PROFILE)).toEqual({
      decision: "allow",
    });
  });

  it("denies network access under develop-offline", () => {
    expect(
      evaluatePermission("network.outbound", developPolicy, DEVELOP_OFFLINE_PROFILE),
    ).toMatchObject({
      decision: "deny",
    });
  });

  it("fails closed when a capability rule is missing", () => {
    const missing = evaluatePermission(
      "network.outbound",
      policyWithout("network.outbound"),
      DEVELOP_OFFLINE_PROFILE,
    );
    expect(missing).toMatchObject({ decision: "deny" });
  });

  it("lets an explicit deny override an allowed profile", () => {
    const policy = policyWith({ "workspace.write": "deny" });
    expect(evaluatePermission("workspace.write", policy, DEVELOP_OFFLINE_PROFILE)).toMatchObject({
      decision: "deny",
    });
  });

  it("lets a profile constraint deny a policy-allowed capability", () => {
    const policy = policyWith({ "process.execute": "allow" });
    expect(evaluatePermission("process.execute", policy, INSPECT_PROFILE)).toMatchObject({
      decision: "deny",
    });
  });

  it("returns ask when the policy rule is ask and the profile permits", () => {
    const policy = policyWith({ "workspace.write": "ask" });
    expect(evaluatePermission("workspace.write", policy, DEVELOP_OFFLINE_PROFILE)).toMatchObject({
      decision: "ask",
    });
  });

  it("never allows network outbound through any profile", () => {
    const policy = policyWith({ "network.outbound": "allow" });
    expect(evaluatePermission("network.outbound", policy, DEVELOP_OFFLINE_PROFILE)).toMatchObject({
      decision: "deny",
    });
  });

  it("profiles cannot broaden a denied capability", () => {
    const policy = policyWith({ "workspace.write": "deny" });
    expect(evaluatePermission("workspace.write", policy, DEVELOP_OFFLINE_PROFILE)).toMatchObject({
      decision: "deny",
    });
  });
});
