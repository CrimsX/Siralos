import type { GodotDoctorReport } from "../godot/inspector.js";
import type { PermissionRule } from "../security/capability.js";
import type { SandboxBackendStatus } from "../security/sandbox-backend.js";
import type { CapabilityState } from "./doctor-model.js";

/**
 * Doctor diagnostic sources (Stage 3 milestone 6).
 *
 * The CapabilityDoctor orchestrates EXISTING subsystem owners through
 * these ports — it never re-implements provider, sandbox, Godot,
 * reference, research, or projection logic, and it never imports
 * concrete implementations. The composition root (CLI bootstrap) wires
 * real instances; tests wire deterministic fakes.
 *
 * Every probe here is read-only and offline by default: no network, no
 * spawns, no mutations, no refreshes. Each probe is bounded by the
 * doctor's per-check timeout.
 */

export interface RuntimeDiagnosticResult {
  readonly version: string;
  readonly nodeMajor: number;
  /** Whether the running Node major is supported (engines >= 24). */
  readonly nodeSupported: boolean;
  readonly platform: string;
  readonly configurationFile: {
    readonly state: "readable" | "missing" | "unreadable";
    readonly detail: string | null;
  };
  readonly checkpointStoreAccessible: boolean;
}

export interface CredentialRefStatus {
  /** Environment-variable name only — never a value. */
  readonly name: string;
  readonly referenced: boolean;
  readonly present: boolean;
}

export interface ConfigurationDiagnosticResult {
  readonly loaded: boolean;
  readonly sections: readonly { readonly name: string; readonly present: boolean }[];
  readonly unknownFields: readonly string[];
  /** Bounded, sanitized validation errors (no values, no secrets). */
  readonly validationErrors: readonly string[];
  readonly credentialRefs: readonly CredentialRefStatus[];
  readonly overrideInUse: boolean;
}

export interface ProviderEndpointStatus {
  readonly label: string;
  readonly https: boolean;
  readonly loopback: boolean;
  /** False when the endpoint violates the HTTPS/loopback transport rule. */
  readonly valid: boolean | null;
  readonly reason: string | null;
}

export interface ProviderModelStatus {
  readonly id: string | null;
  readonly toolCalling: boolean | null;
  readonly contextBudgetTokens: number | null;
}

export interface ProviderDiagnosticResult {
  readonly active: {
    readonly profileId: string;
    readonly toolCalling: boolean | null;
    readonly state: CapabilityState;
    readonly reason: string | null;
  };
  readonly reviewProvider: {
    readonly configured: boolean;
    readonly resolved: boolean;
    readonly profileId: string | null;
    readonly state: CapabilityState;
    readonly reason: string | null;
  };
  readonly credentials: readonly CredentialRefStatus[];
  readonly endpoints: readonly ProviderEndpointStatus[];
  readonly model: ProviderModelStatus;
}

export interface SandboxDiagnosticResult {
  /** Authoritative backend status from the sandbox backend itself. */
  readonly backend: SandboxBackendStatus;
  readonly selectedProfileId: string;
  /** Whether the selected profile requires process execution. */
  readonly profileRequiresProcess: boolean;
  /** Whether the selected profile requires workspace writes. */
  readonly profileRequiresWrite: boolean;
  /** Enforcement capabilities the profile requires but the backend lacks. */
  readonly requiredCapabilitiesMissing: readonly string[];
  /** True only if an unrestricted fallback path exists (never in this runtime). */
  readonly unrestrictedFallback: boolean;
}

export interface WorkspaceDiagnosticResult {
  readonly root: string | null;
  readonly readable: boolean;
  readonly protectedPathsActive: boolean;
  readonly gitAvailable: boolean | null;
  readonly gitState: string | null;
  readonly checkpointStoreAccessible: boolean;
  readonly revisionRegistryOperational: boolean;
  /** Workspace/reference namespace separation intact. */
  readonly namespaceIntegrity: boolean;
}

export interface GodotVersionMatchStatus {
  readonly state: "exact" | "stale" | "absent" | "unknown";
  readonly reason: string | null;
}

export interface GodotDiagnosticResult {
  /** The authoritative Godot inspector doctor report (reused, not reimplemented). */
  readonly report: GodotDoctorReport;
  /** Version-match of any cached API/docs surface against the selected engine. */
  readonly versionMatch: GodotVersionMatchStatus;
  /** Canonical workspace root the project profile was derived from. */
  readonly projectRoot: string | null;
  /** Policy rules for approval-requiring Godot operations (from the active policy). */
  readonly policyRules: {
    readonly recoveryProbe: PermissionRule;
    readonly lsp: PermissionRule;
    readonly diagnose: PermissionRule;
  };
}

export interface ReferenceEntryStatus {
  readonly alias: string;
  readonly kind: string;
  readonly trust: string;
  readonly status: string;
  readonly failureReason: string | null;
  readonly revision: {
    readonly kind: string;
    readonly fingerprint: string | null;
    readonly commit: string | null;
  } | null;
  readonly materialized: string;
}

export interface ReferenceDiagnosticResult {
  readonly configError: string | null;
  readonly references: readonly ReferenceEntryStatus[];
}

export interface ResearchDiagnosticResult {
  readonly sources: readonly {
    readonly kind: string;
    readonly id: string;
    readonly label: string;
  }[];
  readonly policyRule: PermissionRule;
  readonly gate: "allowed" | "blocked_by_policy";
  readonly adapterAvailability: readonly {
    readonly kind: string;
    readonly available: boolean;
    readonly reason: string | null;
  }[];
  readonly latestEvidenceCount: number;
}

export interface ProjectedToolStatus {
  readonly name: string;
  readonly state: "available" | "gated" | "hidden";
  /** Policy-rule explanation behind the projected state, when known. */
  readonly reason: string | null;
}

export interface CapabilityDiagnosticResult {
  readonly mode: string;
  /** Diagnostic trace steps (registered → profile → policy → mode → model → projected). */
  readonly trace: readonly { readonly step: string; readonly detail: string }[];
  readonly tools: readonly ProjectedToolStatus[];
}

export interface TaskSnapshotDifference {
  readonly field: string;
  readonly snapshotValue: string | null;
  readonly currentValue: string | null;
}

export interface TaskSnapshotDiagnosticResult {
  readonly activeTask: boolean;
  readonly runtimeVersion: string | null;
  readonly differences: readonly TaskSnapshotDifference[];
}

export interface DoctorSources {
  readonly runtime: () => Promise<RuntimeDiagnosticResult>;
  readonly configuration: () => Promise<ConfigurationDiagnosticResult>;
  readonly providers: () => Promise<ProviderDiagnosticResult>;
  readonly sandbox: () => Promise<SandboxDiagnosticResult>;
  readonly workspace: () => Promise<WorkspaceDiagnosticResult>;
  readonly godot: () => Promise<GodotDiagnosticResult>;
  readonly references: () => Promise<ReferenceDiagnosticResult>;
  readonly research: () => Promise<ResearchDiagnosticResult>;
  readonly capabilities: () => Promise<CapabilityDiagnosticResult>;
  /** Current-task immutable-snapshot comparison (Part J §33). */
  readonly tasks: () => Promise<TaskSnapshotDiagnosticResult>;
}
