import type { Capability, CapabilityPolicy } from "./capability.js";
import { normalizeSandboxError } from "./sandbox-error.js";
import type { PermissionEvaluation } from "./permission-evaluator.js";
import { evaluatePermission } from "./permission-evaluator.js";
import type { SandboxBackend, SandboxBackendStatus } from "./sandbox-backend.js";
import type { SandboxProfile } from "./profile.js";
import type { SandboxEvent } from "./sandbox-events.js";

export interface SiralosSecurityDependencies {
  readonly backend: SandboxBackend;
  readonly policy: CapabilityPolicy;
  readonly profile: SandboxProfile;
}

export interface SiralosSecurity {
  readonly profile: SandboxProfile;
  readonly policy: CapabilityPolicy;

  evaluateCapability(capability: Capability): PermissionEvaluation;

  checkSandbox(signal?: AbortSignal): AsyncIterable<SandboxEvent>;
}

export function createSiralosSecurity(dependencies: SiralosSecurityDependencies): SiralosSecurity {
  async function* checkSandbox(signal?: AbortSignal): AsyncIterable<SandboxEvent> {
    if (signal?.aborted) {
      yield {
        type: "sandbox_check_completed",
        status: failedStatus(dependencies.backend.id, "Sandbox check was cancelled."),
      };
      return;
    }
    yield { type: "sandbox_check_started", backendId: dependencies.backend.id };
    let status: SandboxBackendStatus;
    try {
      status = await dependencies.backend.inspect();
    } catch (error: unknown) {
      status = failedStatus(dependencies.backend.id, normalizeSandboxError(error).message);
    }
    yield { type: "sandbox_check_completed", status };
  }

  return {
    profile: dependencies.profile,
    policy: dependencies.policy,
    evaluateCapability(capability: Capability): PermissionEvaluation {
      return evaluatePermission(capability, dependencies.policy, dependencies.profile);
    },
    checkSandbox,
  };
}

function failedStatus(backendId: string, message: string): SandboxBackendStatus {
  return {
    backendId,
    state: "failed",
    platform: "unknown",
    version: "unknown",
    capabilities: {
      filesystemReadRestriction: false,
      filesystemWriteRestriction: false,
      networkRestriction: false,
      processTreeRestriction: false,
      violationReporting: false,
    },
    message,
  };
}
