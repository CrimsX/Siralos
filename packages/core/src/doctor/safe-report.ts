import { DOCTOR_SCHEMA_VERSION, type DoctorReport, type DoctorStatus } from "./doctor-model.js";

/**
 * Safe/public doctor report (Stage 3 milestone 6, Part K).
 *
 * `toSafeReport` renders a sanitized, bounded report suitable for bug
 * reports and support tooling. It deliberately excludes: source content,
 * prompts, credentials, provider request bodies, tool arguments, secret
 * env values, absolute user paths, and private endpoint/repository URLs.
 * Details and remediations are dropped entirely; summaries are passed
 * through a conservative text sanitizer as defense in depth (checks never
 * include secret values to begin with).
 *
 * The safe report is NOT anonymous: it keeps machine metadata such as OS
 * family, Node major, and Solaris version by design.
 */

export interface SafeDoctorCheck {
  readonly id: string;
  readonly area: string;
  readonly status: DoctorStatus;
  readonly summary: string;
}

export interface SafeDoctorErrorCategory {
  readonly area: string;
  readonly status: DoctorStatus;
  readonly count: number;
}

export interface SafeDoctorReport {
  readonly schemaVersion: number;
  readonly runtime: {
    readonly version: string;
    readonly nodeMajor: number;
    readonly platform: string;
  };
  readonly requestedAreas: readonly string[];
  readonly checks: readonly SafeDoctorCheck[];
  readonly counts: {
    readonly pass: number;
    readonly warn: number;
    readonly fail: number;
    readonly skip: number;
    readonly total: number;
  };
  readonly errorCategories: readonly SafeDoctorErrorCategory[];
}

const PATH_PATTERNS: readonly RegExp[] = [
  // Windows drive paths
  /[A-Za-z]:\\[^\s"']*/g,
  // POSIX absolute paths under common roots
  /\/(?:Users|home|tmp|var|etc|usr|opt|mnt|media|run|srv|root|workspaces|app|data)(?:\/[^\s"']*)?/g,
  // Defense in depth: any other multi-segment absolute POSIX path (two or
  // more segments, so single-segment tokens like "/doctor" stay intact).
  /\/(?:[A-Za-z0-9_.-]+\/){2,}[^\s"']*/g,
  // Leading home shorthand
  /~[^\s"']*/g,
];

const SECRET_PATTERNS: readonly RegExp[] = [
  // OpenAI-style keys
  /\bsk-[A-Za-z0-9_-]{8,}\b/g,
  // AWS access keys
  /\bAKIA[0-9A-Z]{16}\b/g,
  // GitHub tokens
  /\bgh[pso]_[A-Za-z0-9_]{20,}\b/g,
  // Bearer tokens
  /\bBearer\s+[A-Za-z0-9._~+/=-]{12,}\b/gi,
  // Long hex runs (hashes/keys)
  /\b[0-9a-fA-F]{32,}\b/g,
  // Long base64 runs
  /\b[A-Za-z0-9+/]{40,}={0,2}\b/g,
];

/**
 * Conservative sanitizer for doctor text: redacts absolute paths and
 * credential-shaped tokens. Deterministic and bounded.
 */
export function sanitizeSafeDoctorText(text: string): string {
  let sanitized = text;
  for (const pattern of PATH_PATTERNS) {
    sanitized = sanitized.replace(pattern, "<path>");
  }
  for (const pattern of SECRET_PATTERNS) {
    sanitized = sanitized.replace(pattern, "<secret>");
  }
  return sanitized;
}

export function toSafeReport(report: DoctorReport): SafeDoctorReport {
  const errorCategories: SafeDoctorErrorCategory[] = [];
  for (const area of report.requestedAreas) {
    const areaChecks = report.checks.filter((check) => check.area === area);
    for (const status of ["fail", "warn"] as const) {
      const count = areaChecks.filter((check) => check.status === status).length;
      if (count > 0) {
        errorCategories.push({ area, status, count });
      }
    }
  }
  return {
    schemaVersion: DOCTOR_SCHEMA_VERSION,
    runtime: {
      version: report.runtime.version,
      nodeMajor: report.runtime.nodeMajor,
      platform: report.runtime.platform,
    },
    requestedAreas: [...report.requestedAreas],
    checks: report.checks.map((check) => ({
      id: check.id,
      area: check.area,
      status: check.status,
      summary: sanitizeSafeDoctorText(check.summary),
    })),
    counts: { ...report.counts },
    errorCategories,
  };
}
