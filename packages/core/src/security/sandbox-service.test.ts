import { describe, expect, it } from "vitest";
import {
  createDefaultPolicy,
  createSolarisSecurity,
  INSPECT_PROFILE,
  SandboxError,
  type SandboxBackend,
  type SandboxBackendStatus,
  type SandboxEvent,
} from "../index.js";

function createStubBackend(status: SandboxBackendStatus): {
  backend: SandboxBackend;
  inspectCalls: () => number;
} {
  let count = 0;
  const backend: SandboxBackend = {
    id: "stub-backend",
    inspect(): Promise<SandboxBackendStatus> {
      count += 1;
      return Promise.resolve(status);
    },
    execute(): Promise<never> {
      throw new SandboxError("sandbox_policy_denied", "No execution under inspect.");
    },
    close(): Promise<void> {
      return Promise.resolve();
    },
  };
  return { backend, inspectCalls: () => count };
}

function createFailingBackend(): SandboxBackend {
  return {
    id: "failing-backend",
    inspect(): Promise<SandboxBackendStatus> {
      throw new Error("backend exploded");
    },
    execute(): Promise<never> {
      throw new Error("backend exploded");
    },
    close(): Promise<void> {
      return Promise.resolve();
    },
  };
}

async function collectEvents(events: AsyncIterable<SandboxEvent>): Promise<SandboxEvent[]> {
  const collected: SandboxEvent[] = [];
  for await (const event of events) {
    collected.push(event);
  }
  return collected;
}

describe("createSolarisSecurity", () => {
  it("exposes the profile and policy", () => {
    const policy = createDefaultPolicy("inspect");
    const { backend } = createStubBackend({
      backendId: "stub-backend",
      state: "available",
      platform: "linux",
      version: "0.0.0-test",
      capabilities: {
        filesystemReadRestriction: true,
        filesystemWriteRestriction: true,
        networkRestriction: true,
        processTreeRestriction: true,
        violationReporting: true,
      },
    });
    const security = createSolarisSecurity({ backend, policy, profile: INSPECT_PROFILE });
    expect(security.profile).toBe(INSPECT_PROFILE);
    expect(security.policy).toBe(policy);
  });

  it("delegates capability evaluation", () => {
    const policy = createDefaultPolicy("inspect");
    const { backend } = createStubBackend({
      backendId: "stub-backend",
      state: "available",
      platform: "linux",
      version: "0.0.0-test",
      capabilities: {
        filesystemReadRestriction: true,
        filesystemWriteRestriction: true,
        networkRestriction: true,
        processTreeRestriction: true,
        violationReporting: true,
      },
    });
    const security = createSolarisSecurity({ backend, policy, profile: INSPECT_PROFILE });
    expect(security.evaluateCapability("workspace.write")).toMatchObject({ decision: "deny" });
    expect(security.evaluateCapability("workspace.read")).toEqual({ decision: "allow" });
  });

  it("emits check events around a backend inspection", async () => {
    const status: SandboxBackendStatus = {
      backendId: "stub-backend",
      state: "available",
      platform: "linux",
      version: "0.0.0-test",
      capabilities: {
        filesystemReadRestriction: true,
        filesystemWriteRestriction: true,
        networkRestriction: true,
        processTreeRestriction: true,
        violationReporting: true,
      },
    };
    const { backend, inspectCalls } = createStubBackend(status);
    const security = createSolarisSecurity({
      backend,
      policy: createDefaultPolicy("inspect"),
      profile: INSPECT_PROFILE,
    });
    const events = await collectEvents(security.checkSandbox());
    expect(events).toEqual([
      { type: "sandbox_check_started", backendId: "stub-backend" },
      { type: "sandbox_check_completed", status },
    ]);
    expect(inspectCalls()).toBe(1);
  });

  it("normalizes backend failures into a failed status", async () => {
    const security = createSolarisSecurity({
      backend: createFailingBackend(),
      policy: createDefaultPolicy("inspect"),
      profile: INSPECT_PROFILE,
    });
    const events = await collectEvents(security.checkSandbox());
    const completed = events.at(-1);
    expect(completed).toMatchObject({
      type: "sandbox_check_completed",
      status: { state: "failed", backendId: "failing-backend" },
    });
  });
});
