import type { GodotCommandCapabilities } from "./capabilities.js";
import type { GodotCompatibilityAssessment } from "./compatibility.js";
import type { SafeDiagnostic } from "./diagnostics.js";
import type {
  GodotEdition,
  GodotEditionConfidence,
  SolarisGodotSupport,
} from "./engine-profile.js";
import type { GodotInstallationSource } from "./installations.js";
import type { GodotProjectProfile } from "./project.js";
import type { GodotReleaseChannel, GodotVersion } from "./version.js";

/**
 * Provider-safe summary of one installation. Absolute executable paths are
 * never included; results use installation ids and executable fingerprints.
 */
export interface GodotInstallationOverview {
  readonly installationId: string;
  readonly version: GodotVersion | null;
  readonly edition: GodotEdition | null;
  readonly editionConfidence: GodotEditionConfidence | null;
  readonly releaseChannel: GodotReleaseChannel | null;
  readonly sourceLabel: string;
  readonly source: GodotInstallationSource;
  readonly support: SolarisGodotSupport | null;
  /** Bounded error for invalid candidates; null when the candidate is valid. */
  readonly invalid: string | null;
  readonly isDuplicate: boolean;
  readonly selected: boolean;
  /** Short executable SHA-256 prefix (provider-safe fingerprint). */
  readonly fingerprint: string | null;
  /** Missing when the candidate has not been profiled yet. */
  readonly profiled: boolean;
}

export interface GodotDiscoveryResult {
  readonly candidates: readonly GodotInstallationOverview[];
  /** Effective configuration summary, safe for display. */
  readonly configuration: {
    readonly activeInstallation: string | null;
    readonly configuredCount: number;
    readonly discoverOnPath: boolean;
    readonly overrides: readonly string[];
  };
  readonly selected: GodotInstallationOverview | null;
  /** Bounded automatic-selection rationale. */
  readonly rationale: readonly string[];
  /** Bounded configuration and discovery diagnostics. */
  readonly diagnostics: readonly SafeDiagnostic[];
}

/** Provider-safe view of the selected installation (safe subset). */
export interface GodotSelectedInstallation {
  readonly installationId: string;
  readonly sourceLabel: string;
  readonly source: GodotInstallationSource;
  readonly version: GodotVersion;
  readonly edition: GodotEdition;
  readonly editionConfidence: GodotEditionConfidence;
  readonly releaseChannel: GodotReleaseChannel;
  readonly support: SolarisGodotSupport;
  readonly capabilities: GodotCommandCapabilities;
  readonly verifiedCapabilities: readonly string[];
  readonly degradedCapabilities: readonly string[];
  /** Short executable SHA-256 prefix. */
  readonly executableFingerprint: string;
  readonly apiDumpSha256: string | null;
  readonly diagnostics: readonly SafeDiagnostic[];
}

export interface GodotDoctorReport {
  readonly discovery: GodotDiscoveryResult;
  readonly project: GodotProjectProfile;
  readonly compatibility: GodotCompatibilityAssessment;
  readonly cache: {
    readonly schemaVersion: number;
    readonly cachedProfileCount: number;
    readonly enabled: boolean;
  };
  readonly sandbox: {
    readonly state: string;
    readonly backendId: string;
    readonly filesystemReadRestriction: boolean;
    readonly networkRestriction: boolean;
    readonly filesystemWriteRestriction: boolean;
    readonly processTreeRestriction: boolean;
  };
  /** Bounded per-probe status lines. */
  readonly probes: readonly {
    readonly installationId: string;
    readonly probe: string;
    readonly status: string;
  }[];
}

/**
 * Provider-neutral Godot inspection port. Implemented by the adapter layer;
 * the CLI and provider tools consume only this port.
 */
export interface GodotInspector {
  /**
   * Discover, validate, and select installations. Re-collects candidates
   * and revalidates executable fingerprints on every call; profiles are
   * served from the engine-profile cache only while the full SHA-256
   * matches.
   */
  discover(signal?: AbortSignal): Promise<GodotDiscoveryResult>;

  /** Selected installation after discovery, or null. */
  selected(signal?: AbortSignal): Promise<GodotSelectedInstallation | null>;

  /** Static project profile from the workspace root. */
  projectProfile(signal?: AbortSignal): Promise<GodotProjectProfile>;

  /** Compatibility between the selected engine and the static project. */
  compatibility(signal?: AbortSignal): Promise<GodotCompatibilityAssessment>;

  /** Full bounded diagnostics report. */
  doctor(signal?: AbortSignal): Promise<GodotDoctorReport>;
}
