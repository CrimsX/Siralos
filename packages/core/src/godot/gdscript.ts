import { canonicalizeJson, sha256Hex } from "./digest.js";
import { GODOT_LIMITS } from "./limits.js";

/**
 * Provider-neutral GDScript diagnostic model.
 *
 * Diagnostics come from the exact selected Godot engine running
 * `--check-only` against the disposable project mirror; the source
 * workspace is never opened by the engine. Engine console output is not a
 * formally versioned machine protocol, so parsing is conservative,
 * unmatched error-like lines are preserved as generic diagnostics, and
 * unknown line/column values are never fabricated.
 */
export interface GodotGDScriptDiagnostic {
  /** `godot-check-only` from the parser runner; `godot-lsp` from LSP. */
  readonly source: "godot-check-only" | "godot-lsp";
  readonly severity: "error" | "warning" | "info" | "unknown";
  /** Workspace-relative path; null when the engine output carries none. */
  readonly path: string | null;
  readonly line: number | null;
  readonly column: number | null;
  /** Stable diagnostic code when the engine output carries one; else null. */
  readonly code: string | null;
  /** Bounded, control-character-sanitized message. */
  readonly message: string;
  /** Raw category token preserved from the engine output; else null. */
  readonly rawCategory: string | null;
}

/** One script target of a prepared diagnostic check. */
export interface GodotScriptCheckTarget {
  /** Workspace-relative path with `/` separators. */
  readonly path: string;
  readonly sha256: string;
  readonly bytes: number;
}

/**
 * Immutable preview shown to the user before the one-time approval. No
 * absolute paths are ever included; the script list is bounded and the
 * single-script case shows the exact relative path.
 */
export interface GodotDiagnosticPreview {
  readonly projectName: string | null;
  readonly engineVersion: string;
  readonly installationId: string;
  readonly engineEdition: string;
  readonly support: string;
  readonly compatibility: string;
  readonly scripts: {
    readonly count: number;
    /** Exact relative paths for single-script checks; null for project-wide. */
    readonly paths: readonly string[] | null;
    readonly totalBytes: number;
  };
  readonly operation: "parse-only";
  readonly isolation: {
    readonly sourceWorkspace: "not-used-as-project";
    readonly disposableMirror: true;
    readonly checkOnly: true;
    readonly headless: true;
    readonly sceneExecution: "disabled";
    readonly gameExecution: "disabled";
    readonly network: "denied";
    readonly environment: "minimal";
    readonly stdin: "closed";
  };
  /** Risk-manifest digest the approval binds to. */
  readonly manifestDigest: string;
}

/**
 * Fixed Solaris-owned parts the prepared-check digest binds. The check-only
 * command digest is computed by the adapter over the fixed argument
 * template (mirror paths canonicalized to markers); the sandbox profile and
 * check limits are Solaris-fixed constants. Provider input cannot influence
 * any of them.
 */
export interface GodotCheckOnlyCommandDigestParts {
  readonly executableSha256: string;
  readonly argumentTemplate: readonly string[];
  readonly workingDirectoryPolicy: "disposable-mirror";
  readonly profileId: string;
  readonly environmentPolicy: "minimal";
  readonly stdinPolicy: "closed";
  readonly networkPolicy: "denied";
  readonly timeoutMs: number;
  readonly stdoutLimitBytes: number;
  readonly stderrLimitBytes: number;
}

/** Deterministic digest over the fixed check-only command shape. */
export function computeGodotCheckOnlyCommandDigest(
  parts: GodotCheckOnlyCommandDigestParts,
): string {
  return sha256Hex(canonicalizeJson(parts));
}

export interface GodotPreparedCheckDigestParts {
  /** Sorted script targets (path + content hash). */
  readonly scriptTargets: readonly GodotScriptCheckTarget[];
  readonly manifestDigest: string;
  readonly commandDigest: string;
  readonly sandboxProfileId: string;
  readonly checkLimits: {
    readonly timeoutMs: number;
    readonly maxScripts: number;
    readonly maxTotalBytes: number;
    readonly maxDiagnosticsPerScript: number;
    readonly maxDiagnosticsPerRun: number;
  };
}

export function computeGodotPreparedCheckDigest(parts: GodotPreparedCheckDigestParts): string {
  return sha256Hex(canonicalizeJson(parts));
}

const preparedCheckBrand: unique symbol = Symbol("preparedGDScriptCheck");

/**
 * Opaque single-use prepared check. The diagnostics service owns the
 * internal plan keyed by this object; the provider and the CLI can only
 * pass it back to `execute` together with the approved digest.
 */
export interface PreparedGDScriptCheck {
  readonly [preparedCheckBrand]: true;
}

export function createPreparedGDScriptCheck(): PreparedGDScriptCheck {
  return { [preparedCheckBrand]: true };
}

export type GodotCheckPreparationResult =
  | {
      readonly status: "ready";
      readonly check: PreparedGDScriptCheck;
      readonly preview: GodotDiagnosticPreview;
      /** Full prepared-check digest; approval binds to exactly this. */
      readonly digest: string;
    }
  | {
      readonly status: "unavailable" | "unsupported" | "invalid_input" | "failed";
      readonly message: string;
    };

/**
 * One script's check outcome. A script parse failure is a VALID diagnostic
 * result (`status: "checked"`, `valid: false`); it is never represented as
 * an infrastructure failure. Infrastructure failures are the other statuses.
 */
export type GDScriptCheckResult =
  | {
      readonly status: "checked";
      readonly valid: boolean;
      readonly diagnostics: readonly GodotGDScriptDiagnostic[];
      readonly engineVersion: string;
      readonly scriptSha256: string;
    }
  | {
      readonly status:
        | "denied"
        | "conflict"
        | "cancelled"
        | "timed_out"
        | "unsupported"
        | "unavailable"
        | "sandbox_failed"
        | "failed";
      readonly message: string;
    };

/** Aggregated project-wide check outcome. */
export type GodotProjectCheckResult =
  | {
      readonly status: "checked";
      readonly engineVersion: string;
      readonly scriptsChecked: number;
      readonly validCount: number;
      readonly invalidCount: number;
      readonly diagnostics: readonly GodotGDScriptDiagnostic[];
      readonly truncated: boolean;
    }
  | {
      readonly status:
        | "denied"
        | "conflict"
        | "cancelled"
        | "timed_out"
        | "unsupported"
        | "unavailable"
        | "sandbox_failed"
        | "failed";
      readonly message: string;
    };

/**
 * Deterministic diagnostic aggregation policy: exact duplicates are
 * collapsed, results are sorted by (path, line, column, message), and the
 * run-wide bound is applied with explicit truncation. Duplicates from the
 * same engine output are never double-counted.
 */
export function aggregateGDScriptDiagnostics(
  diagnostics: readonly GodotGDScriptDiagnostic[],
  maxDiagnostics: number = GODOT_LIMITS.maxDiagnosticsPerRun,
): { readonly diagnostics: readonly GodotGDScriptDiagnostic[]; readonly truncated: boolean } {
  const seen = new Set<string>();
  const unique: GodotGDScriptDiagnostic[] = [];
  for (const diagnostic of diagnostics) {
    const key = [
      diagnostic.path ?? "",
      diagnostic.line ?? -1,
      diagnostic.column ?? -1,
      diagnostic.code ?? "",
      diagnostic.message,
    ].join("\u0000");
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    unique.push(diagnostic);
  }
  unique.sort((left, right) => {
    const leftPath = left.path ?? "";
    const rightPath = right.path ?? "";
    if (leftPath !== rightPath) {
      return leftPath < rightPath ? -1 : 1;
    }
    const leftLine = left.line ?? -1;
    const rightLine = right.line ?? -1;
    if (leftLine !== rightLine) {
      return leftLine - rightLine;
    }
    const leftColumn = left.column ?? -1;
    const rightColumn = right.column ?? -1;
    if (leftColumn !== rightColumn) {
      return leftColumn - rightColumn;
    }
    const leftMessage = left.message;
    const rightMessage = right.message;
    if (leftMessage !== rightMessage) {
      return leftMessage < rightMessage ? -1 : 1;
    }
    return 0;
  });
  const truncated = unique.length > maxDiagnostics;
  return { diagnostics: unique.slice(0, maxDiagnostics), truncated };
}

/**
 * Truthful platform-level support state for GDScript check-only
 * diagnostics. Execution is available only when every mechanical safety
 * gate can be enforced on this platform (identity-bound launch,
 * identity-bound mirror creation, identity-bound cleanup, and an enforcing
 * sandbox backend). Anything less is reported as unavailable and the
 * service refuses before creating a mirror or launching Godot.
 */
export interface GodotDiagnosticsSupport {
  readonly state: "available" | "unavailable";
  /** Exact reason when unavailable; null when available. */
  readonly reason: string | null;
  readonly platform: string;
}

export type GodotDiagnosticsState = "untrusted" | "check-invalidated";

/**
 * Bounded in-memory diagnostics state for CLI diagnostics. Nothing here is
 * a persistent trust grant; approval is one-time and never persisted.
 */
export interface GodotDiagnosticsStatus {
  readonly state: GodotDiagnosticsState;
  readonly lastResult: GodotProjectCheckResult | null;
  readonly lastManifestDigest: string | null;
  readonly lastEngineVersion: string | null;
}

/**
 * Diagnostic request. `paths` is an optional bounded subset of
 * workspace-relative `.gd` files; absent means project-wide enumeration.
 * The provider cannot select an unlimited subset and cannot choose Godot
 * arguments.
 */
export interface GodotDiagnosticsRequest {
  readonly paths?: readonly string[];
}

export interface GodotDiagnosticsExecutionContext {
  readonly approvedDigest: string;
  readonly signal?: AbortSignal;
}

/**
 * Narrow GDScript checker port owned by core. The adapter implements it;
 * core and the CLI consume it. The provider cannot choose the executable,
 * its arguments, the mirror location, the sandbox configuration, or any
 * limit: `prepare` validates and hashes the exact scripts, refreshes the
 * static risk inventory, and freezes the plan; `execute` refuses to run
 * unless the fresh state still matches the frozen digest under the
 * approved digest. Prepared checks are single-use, bounded, expiring, and
 * disposable.
 */
export interface GodotDiagnostics {
  /** Truthful platform-level support state (never claims availability). */
  support(): Promise<GodotDiagnosticsSupport>;

  /**
   * Validate the requested scripts and freeze the prepared check. When
   * execution is unavailable on this platform, returns a typed
   * `unavailable` result before requesting approval and creates nothing.
   */
  prepare(
    request: GodotDiagnosticsRequest,
    signal?: AbortSignal,
  ): Promise<GodotCheckPreparationResult>;

  execute(
    check: PreparedGDScriptCheck,
    context: GodotDiagnosticsExecutionContext,
  ): Promise<GodotProjectCheckResult>;

  status(): GodotDiagnosticsStatus;

  /** Dispose all prepared checks (session shutdown, denial, supersession). */
  disposeAll(): void;
}
