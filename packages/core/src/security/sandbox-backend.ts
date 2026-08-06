import type { SandboxProfile } from "./profile.js";

export interface SandboxBackendStatus {
  readonly backendId: string;
  readonly state:
    "available" | "setup-required" | "dependency-missing" | "unsupported" | "degraded" | "failed";
  readonly platform: string;
  readonly version: string;
  readonly capabilities: {
    readonly filesystemReadRestriction: boolean;
    readonly filesystemWriteRestriction: boolean;
    readonly networkRestriction: boolean;
    readonly processTreeRestriction: boolean;
    readonly violationReporting: boolean;
  };
  readonly message?: string;
}

export interface SandboxedProcessRequest {
  readonly executable: string;
  readonly arguments: readonly string[];
  readonly workingDirectory: string;
  readonly profile: SandboxProfile;
  readonly environment: Readonly<Record<string, string>>;
  readonly signal?: AbortSignal;
}

export interface SandboxViolation {
  readonly category: string;
  readonly summary: string;
}

export type SandboxedProcessStatus =
  "completed" | "cancelled" | "timed-out" | "sandbox-denied" | "sandbox-unavailable" | "failed";

export interface SandboxedProcessResult {
  readonly status: SandboxedProcessStatus;
  readonly exitCode: number | null;
  readonly signal: string | null;
  readonly stdout: string;
  readonly stderr: string;
  readonly stdoutTruncated: boolean;
  readonly stderrTruncated: boolean;
  readonly durationMs: number;
  readonly violations: readonly SandboxViolation[];
}

export interface SandboxBackend {
  readonly id: string;

  inspect(): Promise<SandboxBackendStatus>;

  execute(request: SandboxedProcessRequest): Promise<SandboxedProcessResult>;

  close(): Promise<void>;
}
