import type { SandboxBackendStatus } from "./sandbox-backend.js";

export type SandboxEvent =
  | {
      readonly type: "sandbox_check_started";
      readonly backendId: string;
    }
  | {
      readonly type: "sandbox_check_completed";
      readonly status: SandboxBackendStatus;
    }
  | {
      readonly type: "sandbox_violation";
      readonly category: string;
      readonly summary: string;
    };
