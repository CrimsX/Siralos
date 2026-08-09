import { canonicalizeJson, sha256Hex } from "./digest.js";
import type { GodotGDScriptDiagnostic } from "./gdscript.js";

/**
 * Provider-neutral GDScript language-session model.
 *
 * Core owns the language-session port and every provider-facing model;
 * it must not know TCP sockets, Godot port numbers, mirror host paths,
 * JSON-RPC framing, or the Godot process implementation. Positions are
 * 1-based (line and column count from 1), matching the CLI and the
 * check-only diagnostic convention; the LSP adapter converts to and from
 * the 0-based LSP line/character convention explicitly at its boundary.
 */

/** 1-based position (line and column start at 1). */
export interface GDScriptPosition {
  readonly line: number;
  readonly column: number;
}

export interface GDScriptSourceRange {
  readonly start: GDScriptPosition;
  readonly end: GDScriptPosition;
}

/** Server capabilities Solaris actually intends to use. */
export interface GDScriptLSPCapabilities {
  readonly diagnostics: boolean;
  readonly hover: boolean;
  readonly completion: boolean;
  readonly definition: boolean;
}

export const EMPTY_GDScript_LSP_CAPABILITIES: GDScriptLSPCapabilities = {
  diagnostics: false,
  hover: false,
  completion: false,
  definition: false,
};

export type GDScriptSessionState = "starting" | "ready" | "stale" | "closed" | "unavailable";

/**
 * Bounded session status for CLI and provider rendering. Absolute mirror
 * paths, LSP tokens, and internal transport data never appear here.
 */
export interface GDScriptSessionStatus {
  readonly state: GDScriptSessionState;
  readonly sessionId: string | null;
  readonly engineVersion: string | null;
  readonly projectName: string | null;
  readonly startedAtMs: number | null;
  /** Milliseconds since the last query; null when never queried. */
  readonly idleMs: number | null;
  readonly capabilities: GDScriptLSPCapabilities;
  readonly openDocumentCount: number;
  readonly diagnosticCount: number;
  /** Truthful network-isolation scope; never claimed beyond what is enforced. */
  readonly networkIsolation: "loopback-only" | "unverified" | "unavailable";
}

/** Immutable session preview shown before the one-time approval. */
export interface GDScriptLSPSessionPreview {
  readonly projectName: string | null;
  readonly engineVersion: string;
  readonly installationId: string;
  readonly engineEdition: string;
  readonly support: string;
  readonly compatibility: string;
  readonly projectIntelligence: {
    readonly gdscriptFiles: number;
    readonly toolScripts: number;
    readonly editorPlugins: number;
    readonly gdextensions: number;
  };
  readonly session: {
    readonly sourceProject: "disposable mirror";
    readonly godotMode: "headless recovery editor";
    readonly lspNetwork: "loopback only";
    readonly externalNetwork: "denied";
    readonly sourceWrites: "denied";
    readonly providerSecrets: "removed";
    readonly lspMutations: "disabled";
  };
  readonly capabilities: GDScriptLSPCapabilities;
  /** Risk-manifest digest the approval binds to. */
  readonly manifestDigest: string;
}

/** Fixed Solaris-owned parts the prepared-session digest binds. */
export interface GDScriptPreparedSessionDigestParts {
  readonly manifestDigest: string;
  readonly executableSha256: string;
  readonly engineVersion: string;
  readonly mirrorPolicyVersion: number;
  readonly capabilities: GDScriptLSPCapabilities;
  readonly sandboxProfileId: string;
  readonly lspPolicyVersion: number;
  readonly sessionLimits: {
    readonly startupTimeoutMs: number;
    readonly idleTimeoutMs: number;
    readonly maxLifetimeMs: number;
    readonly requestTimeoutMs: number;
    readonly shutdownTimeoutMs: number;
  };
}

export function computeGDScriptPreparedSessionDigest(
  parts: GDScriptPreparedSessionDigestParts,
): string {
  return sha256Hex(canonicalizeJson(parts));
}

/** One hover section; markup is data, never executed or rendered. */
export interface GDScriptHoverSection {
  readonly kind: "plaintext" | "markdown";
  readonly text: string;
}

export interface GDScriptHoverResult {
  readonly path: string;
  readonly range: GDScriptSourceRange | null;
  readonly contents: readonly GDScriptHoverSection[];
}

/** Completion item; `insertText` is data only and never applied. */
export interface GDScriptCompletionItem {
  readonly label: string;
  readonly kind: string | null;
  readonly detail: string | null;
  readonly documentation: string | null;
  readonly insertText: string | null;
}

export interface GDScriptCompletionResult {
  readonly path: string;
  readonly items: readonly GDScriptCompletionItem[];
  readonly truncated: boolean;
}

/**
 * One definition location. `external` is true for engine/internal or
 * out-of-mirror locations, which are represented conservatively without
 * absolute paths.
 */
export interface GDScriptDefinitionLocation {
  readonly path: string;
  readonly range: GDScriptSourceRange;
  readonly external: boolean;
}

export interface GDScriptDefinitionResult {
  readonly path: string;
  readonly locations: readonly GDScriptDefinitionLocation[];
  readonly truncated: boolean;
}

export interface GDScriptDiagnosticResult {
  readonly path: string;
  readonly diagnostics: readonly GodotGDScriptDiagnostic[];
  readonly truncated: boolean;
}

export interface GDScriptDocumentRequest {
  /** Workspace-relative `.gd` path. */
  readonly path: string;
}

export interface GDScriptPositionRequest extends GDScriptDocumentRequest {
  /** 1-based line. */
  readonly line: number;
  /** 1-based column. */
  readonly column: number;
}

export type GDScriptQueryOutcome<T> =
  | {
      readonly status: "ready";
      readonly result: T;
    }
  | {
      readonly status:
        | "session_required"
        | "session_stale"
        | "unavailable"
        | "unsupported"
        | "invalid_input"
        | "failed"
        | "cancelled";
      readonly message: string;
    };

const preparedSessionBrand: unique symbol = Symbol("preparedGDSScriptSession");

/**
 * Opaque single-use prepared session plan. The language service owns the
 * internal plan; the provider and CLI can only pass it back to `start`
 * together with the approved digest.
 */
export interface PreparedGDScriptSession {
  readonly [preparedSessionBrand]: true;
}

export function createPreparedGDScriptSession(): PreparedGDScriptSession {
  return { [preparedSessionBrand]: true };
}

export type GDScriptSessionPreparationResult =
  | {
      readonly status: "ready";
      readonly session: PreparedGDScriptSession;
      readonly preview: GDScriptLSPSessionPreview;
      /** Full prepared-session digest; approval binds to exactly this. */
      readonly digest: string;
    }
  | {
      readonly status: "unavailable" | "unsupported" | "invalid_input" | "failed";
      readonly message: string;
    };

export interface GDScriptSessionStartContext {
  readonly approvedDigest: string;
  readonly signal?: AbortSignal;
}

export type GDScriptSessionStartResult =
  | {
      readonly status: "ready";
      readonly session: GDScriptLanguageSession;
    }
  | {
      readonly status:
        | "denied"
        | "conflict"
        | "cancelled"
        | "timed_out"
        | "unavailable"
        | "unsupported"
        | "failed";
      readonly message: string;
    };

/**
 * One bounded GDScript language session hosted by a recovery-mode Godot
 * editor over a loopback-only LSP channel against the disposable mirror.
 */
export interface GDScriptLanguageSession {
  readonly id: string;
  readonly engineVersion: string;

  getStatus(): GDScriptSessionStatus;

  openDocument(
    request: GDScriptDocumentRequest,
    signal?: AbortSignal,
  ): Promise<GDScriptQueryOutcome<void>>;
  closeDocument(
    request: GDScriptDocumentRequest,
    signal?: AbortSignal,
  ): Promise<GDScriptQueryOutcome<void>>;

  hover(
    request: GDScriptPositionRequest,
    signal?: AbortSignal,
  ): Promise<GDScriptQueryOutcome<GDScriptHoverResult>>;
  completion(
    request: GDScriptPositionRequest,
    signal?: AbortSignal,
  ): Promise<GDScriptQueryOutcome<GDScriptCompletionResult>>;
  definition(
    request: GDScriptPositionRequest,
    signal?: AbortSignal,
  ): Promise<GDScriptQueryOutcome<GDScriptDefinitionResult>>;
  diagnostics(
    request: GDScriptDocumentRequest,
    signal?: AbortSignal,
  ): Promise<GDScriptQueryOutcome<GDScriptDiagnosticResult>>;

  /** Graceful shutdown: LSP shutdown/exit, process termination, cleanup. */
  close(): Promise<void>;
}

/** Truthful platform-level support state for the language session surface. */
export interface GDScriptLanguageSupport {
  readonly state: "available" | "unavailable";
  /** Exact reason when unavailable; null when available. */
  readonly reason: string | null;
  readonly platform: string;
}

/** Read-only view of the selected engine (bounded; no paths). */
export interface GodotSelectedEngine {
  readonly installationId: string;
  readonly sha256: string;
  readonly version: string;
}

/**
 * Provider-neutral GDScript language service port. `prepare` freezes the
 * immutable session plan (risk manifest, executable identity, mirror copy
 * policy, LSP capability set, sandbox profile, limits); `start` revalidates
 * everything under the approved digest and then refuses with a typed
 * `unavailable` outcome unless the platform can mechanically bind the
 * Godot launch, the disposable mirror, and the cleanup to the approved
 * bytes. At this stage every session start fails closed before a mirror is
 * created or an editor is launched.
 */
export interface GDScriptLanguageService {
  support(): Promise<GDScriptLanguageSupport>;

  /** The active session, or null when none is running. */
  activeSession(): GDScriptLanguageSession | null;

  /** The selected engine's bounded identity, or null when none is selected. */
  selectedEngine(signal?: AbortSignal): Promise<GodotSelectedEngine | null>;

  prepare(signal?: AbortSignal): Promise<GDScriptSessionPreparationResult>;

  start(
    session: PreparedGDScriptSession,
    context: GDScriptSessionStartContext,
  ): Promise<GDScriptSessionStartResult>;

  status(): GDScriptSessionStatus;

  /** Stop the active session (if any) and dispose prepared plans. */
  closeAll(): Promise<void>;
}

/** UI-neutral language-session events (no general event bus). */
export type LanguageSessionEvent =
  | {
      readonly type: "lsp_starting";
      readonly sessionId: string;
    }
  | {
      readonly type: "lsp_ready";
      readonly sessionId: string;
    }
  | {
      readonly type: "lsp_diagnostic_update";
      readonly sessionId: string;
      readonly path: string;
      readonly count: number;
    }
  | {
      readonly type: "lsp_stopping";
      readonly sessionId: string;
    }
  | {
      readonly type: "lsp_stopped";
      readonly sessionId: string;
      readonly reason: string;
    }
  | {
      readonly type: "lsp_failed";
      readonly sessionId: string;
      readonly message: string;
    };
