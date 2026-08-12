import type { SiralosRuntimeIdentity } from "../self/self-reference.js";

/**
 * CapabilityDoctor domain model (Stage 3 milestone 6).
 *
 * Deterministic, read-only, offline diagnostic vocabulary. Checks carry
 * structured status (never prose-only), and the report is machine
 * readable (schema-versioned) for CI, behavior tests, and support tooling.
 */

/** Version of the DoctorReport/JSON schema. */
export const DOCTOR_SCHEMA_VERSION = 1;

export type DoctorArea =
  | "runtime"
  | "configuration"
  | "providers"
  | "sandbox"
  | "workspace"
  | "godot"
  | "project"
  | "references"
  | "research"
  | "capabilities"
  | "determinism"
  | "readiness";

export const DOCTOR_AREAS: readonly DoctorArea[] = [
  "runtime",
  "configuration",
  "providers",
  "sandbox",
  "workspace",
  "godot",
  "project",
  "references",
  "research",
  "capabilities",
  "determinism",
  "readiness",
] as const;

export type DoctorStatus = "pass" | "warn" | "fail" | "skip";

export interface DoctorDetail {
  readonly label: string;
  readonly value: string;
}

export interface DoctorRemediation {
  readonly title: string;
  /** Instructions only — the doctor never repairs anything itself. */
  readonly steps: readonly string[];
}

export interface DoctorCheckResult {
  readonly id: string;
  readonly area: DoctorArea;
  readonly status: DoctorStatus;
  readonly summary: string;
  readonly details?: readonly DoctorDetail[];
  readonly remediation?: readonly DoctorRemediation[];
}

export interface DoctorRequest {
  /** Areas to diagnose; undefined/empty means every area. */
  readonly areas?: readonly DoctorArea[];
}

export interface DoctorReportCounts {
  readonly pass: number;
  readonly warn: number;
  readonly fail: number;
  readonly skip: number;
  readonly total: number;
}

/**
 * Capability state vocabulary (Part B §10). A capability is never just
 * true/false; the vocabulary distinguishes support, configuration,
 * availability, projection, and authorization:
 *
 *   available         — usable right now
 *   configured        — declared/selected, not necessarily usable
 *   unavailable       — present in the runtime but cannot run right now
 *   unsupported       — not part of this runtime at all
 *   degraded          — usable but with reduced guarantees
 *   blocked_by_policy — denied by the active capability policy
 *   requires_approval — usable only through the one-time approval protocol
 *   unknown           — genuinely not determinable from this runtime
 */
export type CapabilityState =
  | "available"
  | "configured"
  | "unavailable"
  | "unsupported"
  | "degraded"
  | "blocked_by_policy"
  | "requires_approval"
  | "unknown";

/**
 * Host-owned CapabilitySnapshot (Part B §9): a structured snapshot of
 * what the current Siralos runtime can actually do. The snapshot is
 * OBSERVATION, not policy — it grants nothing; existing runtime policy
 * (SandboxBackend, ToolProjector, security layer) remains authoritative.
 */
export interface ProviderCapabilityStatus {
  readonly profileId: string;
  readonly supported: boolean;
  readonly configured: boolean;
  readonly toolCalling: boolean | null;
  readonly state: CapabilityState;
  readonly reason: string | null;
}

export interface SandboxCapabilityStatus {
  readonly backendId: string;
  readonly backendState: string;
  readonly selectedProfileId: string;
  readonly enforcement: {
    readonly filesystemReadRestriction: boolean;
    readonly filesystemWriteRestriction: boolean;
    readonly networkRestriction: boolean;
    readonly processTreeRestriction: boolean;
  };
  readonly unrestrictedFallback: boolean;
  readonly state: CapabilityState;
  readonly reason: string | null;
}

export interface WorkspaceCapabilityStatus {
  readonly root: string | null;
  readonly readable: boolean;
  readonly protectedPathsActive: boolean;
  readonly gitAvailable: boolean | null;
  readonly checkpointStoreAccessible: boolean;
  readonly revisionRegistryOperational: boolean;
  readonly state: CapabilityState;
  readonly reason: string | null;
}

export interface GodotCapabilityStatus {
  readonly detected: boolean;
  readonly selected: boolean;
  readonly version: string | null;
  readonly edition: string | null;
  readonly fingerprint: string | null;
  readonly support: string | null;
  readonly engineProfileAvailable: boolean;
  readonly apiCacheStale: boolean | null;
  readonly recoveryProbeState: string;
  readonly lspState: string;
  readonly state: CapabilityState;
  readonly reason: string | null;
}

export interface ReferenceCapabilityStatus {
  readonly configuredCount: number;
  readonly readyCount: number;
  readonly failedCount: number;
  readonly state: CapabilityState;
  readonly reason: string | null;
}

export interface ResearchCapabilityStatus {
  readonly sourceKinds: readonly string[];
  readonly policyRule: string;
  readonly gate: "allowed" | "blocked_by_policy";
  readonly state: CapabilityState;
  readonly reason: string | null;
}

export interface ToolCapabilityStatus {
  readonly projectedAvailable: number;
  readonly projectedGated: number;
  readonly projectedHidden: number;
  readonly state: CapabilityState;
  readonly reason: string | null;
}

export interface CapabilitySnapshot {
  readonly runtime: SiralosRuntimeIdentity;
  readonly providers: readonly ProviderCapabilityStatus[];
  readonly sandbox: SandboxCapabilityStatus;
  readonly workspace: WorkspaceCapabilityStatus;
  readonly godot: GodotCapabilityStatus;
  readonly references: ReferenceCapabilityStatus;
  readonly research: ResearchCapabilityStatus;
  readonly tools: ToolCapabilityStatus;
}

export interface DoctorReport {
  readonly schemaVersion: number;
  readonly generatedAtMs: number;
  readonly runtime: SiralosRuntimeIdentity;
  readonly requestedAreas: readonly DoctorArea[];
  readonly checks: readonly DoctorCheckResult[];
  readonly counts: DoctorReportCounts;
  /** Capability snapshot, attached when the capabilities area was requested. */
  readonly snapshot: CapabilitySnapshot | null;
}

/**
 * Doctor exit-code contract:
 *   0 = no diagnostic failures,
 *   1 = one or more diagnostic failures,
 *   2 = doctor invocation/infrastructure failure (unknown area, sources
 *       could not be constructed).
 * Warnings never produce a failure exit code.
 */
export const DOCTOR_EXIT_OK = 0;
export const DOCTOR_EXIT_FAILURES = 1;
export const DOCTOR_EXIT_INVOCATION = 2;

export function doctorExitCodeFor(report: DoctorReport): number {
  return report.counts.fail > 0 ? DOCTOR_EXIT_FAILURES : DOCTOR_EXIT_OK;
}

export function countDoctorReport(report: Pick<DoctorReport, "checks">): DoctorReportCounts {
  const counts = { pass: 0, warn: 0, fail: 0, skip: 0, total: 0 };
  for (const check of report.checks) {
    counts[check.status] += 1;
    counts.total += 1;
  }
  return counts;
}

/** Typed invocation failure (unknown area, bad request) — exit code 2. */
export class DoctorInvocationError extends Error {
  readonly code = "doctor_invocation";
  constructor(message: string) {
    super(message);
    this.name = "DoctorInvocationError";
  }
}

export function normalizeDoctorRequest(request: DoctorRequest): readonly DoctorArea[] {
  const requested = request.areas ?? DOCTOR_AREAS;
  for (const area of requested) {
    if (!(DOCTOR_AREAS as readonly string[]).includes(area)) {
      throw new DoctorInvocationError(`unknown doctor area: ${area}`);
    }
  }
  if (requested.length === 0) {
    return DOCTOR_AREAS;
  }
  // Deterministic canonical order, deduplicated.
  return DOCTOR_AREAS.filter((area) => (requested as readonly string[]).includes(area));
}
