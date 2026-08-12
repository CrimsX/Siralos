import { canonicalizeJson, sha256Hex } from "./digest.js";
import type { SafeDiagnostic } from "./diagnostics.js";

/**
 * Project probe trust state. Approval is always one-time and never
 * persisted; these states describe the current session only.
 *
 * - `untrusted`: no approval is live for the current project state.
 * - `probe-approved`: a recovery probe was approved and is executing (or a
 *   prepared probe is awaiting execution under its bound digest).
 * - `probe-invalidated`: a previously prepared/approved probe no longer
 *   matches the current project or engine state; a new approval is required.
 */
export type GodotProjectTrustState = "untrusted" | "probe-approved" | "probe-invalidated";

/**
 * Truthful platform-level support state for recovery-mode project probing.
 * Execution is available only when every mechanical safety gate can be
 * enforced on this platform by the current Node.js and sandbox primitives
 * (identity-bound launch, identity-bound mirror creation, identity-bound
 * cleanup, and an enforcing sandbox backend). Anything less is reported as
 * unavailable and the runner refuses before creating a mirror or launching
 * Godot.
 */
export interface GodotRecoveryProbeSupport {
  readonly state: "available" | "unavailable";
  /** Exact reason when unavailable; null when available. */
  readonly reason: string | null;
  readonly platform: string;
}

/** Workspace-relative file with its bounded content hash. */
export interface GodotFileRiskEntry {
  readonly path: string;
  readonly sha256: string;
  readonly bytes: number;
}

/** Editor-plugin descriptor with its script content hash. */
export interface GodotPluginRiskEntry {
  readonly path: string;
  readonly name: string;
  readonly enabled: boolean;
  readonly sha256: string;
  readonly bytes: number;
}

/**
 * Referenced native library of a GDExtension descriptor. The hash is null
 * when the library is missing, unreadable, or beyond the bounded size, so a
 * missing library never breaks the digest determinism.
 */
export interface GodotLibraryRiskEntry {
  readonly path: string;
  readonly sha256: string | null;
  readonly bytes: number | null;
}

/** GDExtension descriptor with its content hash and referenced libraries. */
export interface GodotGDExtensionRiskEntry {
  readonly path: string;
  readonly sha256: string;
  readonly bytes: number;
  readonly referencedLibraries: readonly GodotLibraryRiskEntry[];
}

export interface GodotAutoloadRiskEntry {
  readonly name: string;
  readonly target: string;
}

/**
 * Fresh static risk inventory for one recovery probe. Every security-
 * relevant component is hashed at prepare time; the digest below binds the
 * approval, so any change to the project or the selected engine invalidates
 * the approval instead of being silently re-probed.
 */
export interface GodotProjectRiskManifest {
  readonly projectFileSha256: string;
  readonly engineSelection: {
    readonly installationId: string;
    readonly executableSha256: string;
    readonly version: string;
  };
  readonly toolScripts: readonly GodotFileRiskEntry[];
  readonly enabledEditorPlugins: readonly GodotPluginRiskEntry[];
  readonly gdextensionDescriptors: readonly GodotGDExtensionRiskEntry[];
  readonly autoloads: readonly GodotAutoloadRiskEntry[];
  readonly dotnetProjects: readonly string[];
  readonly authoredFileManifest: {
    readonly fileCount: number;
    readonly totalBytes: number;
    readonly digest: string;
    readonly truncated: boolean;
  };
  readonly scanWarnings: readonly SafeDiagnostic[];
  /** Deterministic SHA-256 over every security-relevant field above. */
  readonly digest: string;
}

/**
 * Deterministic SHA-256 over the security-relevant fields of the risk
 * manifest (excluding the digest field itself). Key order inside objects is
 * canonicalized so equal manifests always produce equal digests.
 */
export function computeGodotRiskManifestDigest(
  manifest: Omit<GodotProjectRiskManifest, "digest">,
): string {
  return sha256Hex(canonicalizeJson(manifest));
}

/**
 * Fixed Siralos-owned parts the prepared-probe digest binds. The recovery
 * command digest is computed by the adapter over the fixed argument template
 * (the mirror path is Siralos-generated and canonicalized to a placeholder);
 * the mirror policy version, sandbox profile, and probe limits are
 * Siralos-fixed constants. Provider input cannot influence any of them.
 */
export interface GodotPreparedProbeDigestParts {
  readonly manifestDigest: string;
  readonly commandDigest: string;
  readonly mirrorPolicyVersion: number;
  readonly sandboxProfileId: string;
  readonly probeLimits: {
    readonly timeoutMs: number;
    readonly maxFiles: number;
    readonly maxBytes: number;
    readonly maxSingleFileBytes: number;
    readonly maxDepth: number;
    readonly maxRelativePathBytes: number;
  };
}

export function computeGodotPreparedProbeDigest(parts: GodotPreparedProbeDigestParts): string {
  return sha256Hex(canonicalizeJson(parts));
}

/**
 * Immutable preview shown to the user before the one-time approval. No
 * absolute paths are ever included; the mirror estimate comes from the
 * bounded static scan.
 */
export interface GodotProbePreview {
  readonly projectName: string | null;
  readonly engineVersion: string;
  readonly installationId: string;
  readonly engineEdition: string;
  readonly support: string;
  readonly compatibility: string;
  readonly risks: {
    readonly toolScripts: number;
    readonly enabledEditorPlugins: number;
    readonly gdextensions: number;
    readonly autoloads: number;
    readonly dotnetProjects: number;
  };
  readonly mirror: {
    readonly estimatedFileCount: number;
    readonly estimatedBytes: number;
  };
  readonly isolation: {
    readonly sourceWorkspace: "not-used-as-project";
    readonly disposableMirror: true;
    readonly recoveryMode: true;
    readonly headless: true;
    readonly network: "denied";
    readonly environment: "minimal";
    readonly stdin: "closed";
  };
  readonly manifestDigest: string;
}

export type GodotProbeStatus =
  | "completed"
  | "completed_with_diagnostics"
  | "denied"
  | "conflict"
  | "unsupported"
  | "mirror_too_large"
  | "unavailable"
  | "timed_out"
  | "cancelled"
  | "sandbox_failed"
  | "workspace_changed"
  | "failed";

/** Normalized, sanitized, bounded diagnostic from the recovery probe output. */
export interface GodotDiagnostic {
  readonly severity: "error" | "warning" | "info";
  readonly category: "startup" | "parser" | "import" | "resource" | "script" | "editor" | "unknown";
  readonly message: string;
}

export type GodotImportState =
  | "project opened"
  | "resources scanned"
  | "imports observed"
  | "imports not observed"
  | "import state unknown";

/**
 * Structured evidence returned to the provider and the CLI. Absolute mirror
 * and source paths, credentials, and environment variables never appear;
 * raw engine output is bounded and normalized conservatively.
 */
export interface GodotRecoveryProbeResult {
  readonly status: GodotProbeStatus;
  readonly engine: {
    readonly installationId: string;
    readonly version: string;
    readonly executableFingerprint: string;
  };
  readonly recoveryMode: true;
  readonly mirror: {
    readonly sourceFiles: number;
    readonly sourceBytes: number;
    readonly generatedGodotDirectory: boolean;
    readonly generatedBytes: number | null;
    readonly generatedFiles: number | null;
    readonly importState: GodotImportState;
  };
  readonly diagnostics: {
    readonly errors: readonly GodotDiagnostic[];
    readonly warnings: readonly GodotDiagnostic[];
    readonly truncated: boolean;
  };
  readonly process: {
    readonly exitCode: number | null;
    readonly durationMs: number;
    readonly timedOut: boolean;
  };
  readonly workspaceIntegrity: {
    /** True when the bounded source baseline is unchanged. */
    readonly unchanged: boolean;
    /** True when the baseline itself was truncated by its bounds. */
    readonly bounded: boolean;
  };
  readonly cleanup: {
    readonly completed: boolean;
    readonly message?: string;
  };
  /** Human-readable outcome summary (sanitized). */
  readonly message: string;
}

const preparedProbeBrand: unique symbol = Symbol("preparedGodotProbe");

/**
 * Opaque single-use prepared probe. The probe service owns the internal
 * plan keyed by this object; the provider and the CLI can only pass it back
 * to `execute` together with the approved digest.
 */
export interface PreparedGodotProbe {
  readonly [preparedProbeBrand]: true;
}

export function createPreparedGodotProbe(): PreparedGodotProbe {
  return { [preparedProbeBrand]: true };
}

export type GodotProbePreparationResult =
  | {
      readonly status: "ready";
      readonly probe: PreparedGodotProbe;
      readonly preview: GodotProbePreview;
      /** Full prepared-probe digest; approval binds to exactly this. */
      readonly digest: string;
    }
  | {
      readonly status: "unavailable" | "unsupported" | "failed";
      readonly message: string;
    };

/**
 * Bounded in-memory probe state for CLI diagnostics. Nothing here is a
 * persistent trust grant; approval is one-time and never persisted.
 */
export interface GodotProjectProbeStatus {
  readonly state: GodotProjectTrustState;
  readonly lastResult: GodotRecoveryProbeResult | null;
  readonly lastManifestDigest: string | null;
  readonly lastEngineVersion: string | null;
}

export interface GodotProbeExecutionContext {
  readonly approvedDigest: string;
  readonly signal?: AbortSignal;
}

/**
 * Narrow project-probe port owned by core. The adapter implements it; core
 * and the CLI consume it. The provider cannot choose the executable, its
 * arguments, the mirror location, the sandbox configuration, or any limit:
 * `prepare` refreshes the static risk inventory and freezes the plan, and
 * `execute` refuses to run unless the fresh state still matches the frozen
 * digest under the approved digest. Prepared probes are single-use,
 * bounded, expiring, and disposable.
 */
export interface GodotProjectProbe {
  /** Truthful platform-level support state (never claims availability). */
  support(): Promise<GodotRecoveryProbeSupport>;

  /**
   * Refresh the static risk inventory and freeze the prepared probe. When
   * execution is unavailable on this platform, returns a typed
   * `unavailable` result before requesting approval and creates nothing.
   */
  prepare(signal?: AbortSignal): Promise<GodotProbePreparationResult>;

  execute(
    prepared: PreparedGodotProbe,
    context: GodotProbeExecutionContext,
  ): Promise<GodotRecoveryProbeResult>;

  status(): GodotProjectProbeStatus;

  /** Dispose all prepared probes (session shutdown, denial, supersession). */
  disposeAll(): void;
}
