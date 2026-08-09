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
  return probeSha256(canonicalizeJson(manifest));
}

/**
 * Fixed Solaris-owned parts the prepared-probe digest binds. The recovery
 * command digest is computed by the adapter over the fixed argument template
 * (the mirror path is Solaris-generated and canonicalized to a placeholder);
 * the mirror policy version, sandbox profile, and probe limits are
 * Solaris-fixed constants. Provider input cannot influence any of them.
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
  return probeSha256(canonicalizeJson(parts));
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

function probeSha256(text: string): string {
  return sha256Hex(text);
}

function canonicalizeJson(value: unknown): string {
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map((entry) => canonicalizeJson(entry)).join(",")}]`;
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  return `{${keys.map((key) => `${JSON.stringify(key)}:${canonicalizeJson(record[key])}`).join(",")}}`;
}

// --- pure SHA-256 (FIPS 180-4) for digest computation in Node-free core ---
// Core imports no Node modules (enforced by the architecture check), so the
// digest primitive is a small pure implementation over platform globals.

const SHA256_K: readonly number[] = [
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

function sha256Hex(text: string): string {
  const bytes = new Uint8Array(new TextEncoder().encode(text));
  const lengthBits = BigInt(bytes.length) * 8n;
  const paddedLength = (((bytes.length + 8) >> 6) << 6) + 64;
  const padded = new Uint8Array(paddedLength);
  padded.set(bytes);
  padded[bytes.length] = 0x80;
  const view = new DataView(padded.buffer);
  view.setUint32(paddedLength - 8, Number(lengthBits >> 32n));
  view.setUint32(paddedLength - 4, Number(lengthBits & 0xffffffffn));
  let h0 = 0x6a09e667;
  let h1 = 0xbb67ae85;
  let h2 = 0x3c6ef372;
  let h3 = 0xa54ff53a;
  let h4 = 0x510e527f;
  let h5 = 0x9b05688c;
  let h6 = 0x1f83d9ab;
  let h7 = 0x5be0cd19;
  const words = new Uint32Array(64);
  for (let offset = 0; offset < paddedLength; offset += 64) {
    for (let index = 0; index < 16; index += 1) {
      words[index] = view.getUint32(offset + index * 4);
    }
    for (let index = 16; index < 64; index += 1) {
      const w15 = words[index - 15] as number;
      const w2 = words[index - 2] as number;
      const s0 = rotr(w15, 7) ^ rotr(w15, 18) ^ (w15 >>> 3);
      const s1 = rotr(w2, 17) ^ rotr(w2, 19) ^ (w2 >>> 10);
      words[index] =
        ((((words[index - 16] as number) + s0) | 0) + (((words[index - 7] as number) + s1) | 0)) |
        0;
    }
    let a = h0;
    let b = h1;
    let c = h2;
    let d = h3;
    let e = h4;
    let f = h5;
    let g = h6;
    let h = h7;
    for (let index = 0; index < 64; index += 1) {
      const s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const temp1 =
        (((h + s1) | 0) + (((ch + (SHA256_K[index] as number)) | 0) + (words[index] as number))) |
        0 |
        0;
      const s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const temp2 = (s0 + maj) | 0 | 0;
      h = g;
      g = f;
      f = e;
      e = (d + temp1) | 0 | 0;
      d = c;
      c = b;
      b = a;
      a = (temp1 + temp2) | 0 | 0;
    }
    h0 = (h0 + a) | 0 | 0;
    h1 = (h1 + b) | 0 | 0;
    h2 = (h2 + c) | 0 | 0;
    h3 = (h3 + d) | 0 | 0;
    h4 = (h4 + e) | 0 | 0;
    h5 = (h5 + f) | 0 | 0;
    h6 = (h6 + g) | 0 | 0;
    h7 = (h7 + h) | 0 | 0;
  }
  return [h0, h1, h2, h3, h4, h5, h6, h7]
    .map((word) => (word >>> 0).toString(16).padStart(8, "0"))
    .join("");
}

function rotr(value: number, shift: number): number {
  return (value >>> shift) | (value << (32 - shift));
}
