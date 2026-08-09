import { canonicalizeJson, sha256Hex } from "../digest.js";
import type { GodotGDScriptDiagnostic } from "../gdscript.js";

/**
 * Provider-neutral GDScript development workflow model.
 *
 * Core owns the workflow vocabulary: phases, terminal statuses, the
 * bounded session view, per-iteration validation evidence, the final
 * development result, immutable limits, and UI-neutral workflow events.
 * Core never touches the filesystem, never spawns Godot, and never opens
 * sockets; the workflow implementation lives in the adapters behind the
 * `GDScriptDevelopmentService` port and orchestrates the existing
 * read-only, approval, checkpoint, parser, and language-session surfaces.
 */

/** Active workflow phases (§7). */
export type DevelopmentPhase =
  | "investigating"
  | "proposal_ready"
  | "awaiting_approval"
  | "applying"
  | "parser_validation"
  | "language_validation"
  | "reviewing";

/** Terminal workflow statuses (§35, §48). */
export type DevelopmentStatus =
  | "completed"
  | "completed_with_warnings"
  | "completed_with_errors"
  | "denied"
  | "conflict"
  | "cancelled"
  | "apply_failed"
  | "validation_failed"
  | "unavailable";

export type DevelopmentState =
  | {
      readonly kind: "active";
      readonly phase: DevelopmentPhase;
    }
  | {
      readonly kind: "terminal";
      readonly status: DevelopmentStatus;
    };

/**
 * Normalized validation outcome (§30). `warnings` may still permit a
 * successful completion; `errors` require provider review and repair;
 * `infrastructure_failure` means a required validation gate could not
 * run (never presented as source invalidity); `cancelled` means the
 * workflow stopped before validation completed.
 */
export type DevelopmentValidationStatus =
  "clean" | "warnings" | "errors" | "infrastructure_failure" | "cancelled";

/** One file of one approved change set (§26). */
export interface DevelopmentChangeRecord {
  readonly path: string;
  readonly operation: "create" | "update" | "delete";
  readonly beforeSha256: string | null;
  readonly afterSha256: string | null;
}

/** Per-iteration validation evidence (§26). */
export interface DevelopmentEvidence {
  readonly changeSetId: string;
  readonly files: readonly DevelopmentChangeRecord[];
  readonly parser: {
    readonly checkedFiles: number;
    readonly validFiles: number;
    readonly diagnostics: readonly GodotGDScriptDiagnostic[];
  };
  readonly lsp: {
    readonly started: boolean;
    readonly diagnosticCount: number;
    readonly diagnostics: readonly GodotGDScriptDiagnostic[];
  };
  readonly git: {
    /** False when Git inspection is unavailable or the status cannot be read. */
    readonly available: boolean;
    readonly changedFiles: readonly string[];
  };
  readonly workspaceIntegrity: {
    readonly verified: boolean;
    /** Files outside the approved change set that changed unexpectedly. */
    readonly unexpectedChanges: readonly string[];
  };
}

/** Bounded development session view (§8). */
export interface GDScriptDevelopmentSession {
  readonly id: string;
  /** Authored-file manifest digest recorded when the workflow started. */
  readonly projectFingerprint: string;
  /** Selected-engine executable SHA-256 at workflow start; null when none. */
  readonly engineFingerprint: string | null;
  readonly request: string;
  readonly state: DevelopmentState;
  /** Approved change sets applied so far. */
  readonly iteration: number;
  readonly repairProposalsUsed: number;
  readonly evidence: readonly DevelopmentEvidence[];
}

/** Provider- and CLI-visible bounded status; no mirror paths, no raw LSP data. */
export interface GDScriptDevelopmentStatus {
  readonly support: {
    readonly available: boolean;
    /** Exact reason when unavailable; null when available. */
    readonly reason: string | null;
    readonly platform: string;
  };
  readonly session: {
    readonly id: string;
    readonly request: string;
    readonly state: DevelopmentState;
    readonly iteration: number;
    readonly maxIterations: number;
    readonly repairProposalsRemaining: number;
    /** Most recent validation outcome; null before the first validation. */
    readonly validation: DevelopmentValidationStatus | null;
    readonly appliedChangeSets: number;
    readonly errors: number;
    readonly warnings: number;
  } | null;
}

/**
 * Bounded final development result returned to the provider and CLI
 * (§35). Never includes mirror paths, credentials, or raw transport data.
 */
export interface GDScriptDevelopmentResult {
  readonly status: DevelopmentStatus;
  readonly iterations: number;
  readonly changes: readonly DevelopmentChangeRecord[];
  readonly diagnostics: {
    readonly errors: number;
    readonly warnings: number;
  };
  readonly validation: {
    readonly parser: boolean;
    readonly lsp: boolean;
    readonly workspaceIntegrity: boolean;
  };
  readonly checkpointIds: readonly string[];
}

/**
 * Immutable development-workflow limits (§60). Provider input and user
 * configuration can never raise them.
 */
export const DEVELOPMENT_LIMITS = {
  /** One active development workflow per Solaris session. */
  maxConcurrentWorkflows: 1,
  /** Maximum files per change set (§19). */
  maxFilesPerChangeSet: 16,
  /** Maximum complete change-set diff shown before approval (§19). */
  maxChangeSetDiffBytes: 512 * 1024,
  /** Maximum total resulting bytes of one change set (§19). */
  maxChangeSetResultBytes: 4 * 1024 * 1024,
  /** Maximum exact-text replacements per edited file (§19). */
  maxReplacementsPerFile: 32,
  /** Maximum UTF-8 bytes of one replacement text. */
  maxReplacementTextBytes: 64 * 1024,
  /** Maximum UTF-8 bytes of one created-file content. */
  maxCreateContentBytes: 512 * 1024,
  /** Maximum UTF-8 bytes of one text file the change set may address. */
  maxTextFileBytes: 1024 * 1024,
  /** Maximum automatic repair proposals (§32). */
  maxRepairProposals: 3,
  /** Maximum total development iterations (§32). */
  maxTotalIterations: 4,
  /** Per-script `--check-only` parser timeout (§60). */
  parserTimeoutMs: 30_000,
  /** Fresh LSP session startup timeout (§60). */
  lspStartupTimeoutMs: 30_000,
  /** Validation budget per iteration (§60). */
  validationBudgetMs: 2 * 60 * 1000,
  /** Total development workflow budget (§60). */
  totalWorkflowBudgetMs: 15 * 60 * 1000,
  /** Maximum retained diagnostics in one evidence record. */
  maxEvidenceDiagnostics: 500,
  /** Maximum changed GDScript files checked per iteration. */
  maxChangedScriptsChecked: 16,
  /** Maximum lifetime of a prepared change set before it expires. */
  preparedChangeSetTtlMs: 10 * 60 * 1000,
} as const;

/** UI-neutral development-workflow events (§41); no general event bus. */
export type DevelopmentEvent =
  | {
      readonly type: "development_started";
      readonly id: string;
    }
  | {
      readonly type: "development_investigating";
      readonly id: string;
    }
  | {
      readonly type: "development_change_prepared";
      readonly id: string;
      readonly files: number;
    }
  | {
      readonly type: "development_change_approved";
      readonly id: string;
      readonly changeSetId: string;
    }
  | {
      readonly type: "development_language_suspending";
      readonly id: string;
    }
  | {
      readonly type: "development_language_suspended";
      readonly id: string;
    }
  | {
      readonly type: "development_change_applied";
      readonly id: string;
      readonly files: number;
    }
  | {
      readonly type: "development_validation_started";
      readonly id: string;
    }
  | {
      readonly type: "development_parser_completed";
      readonly id: string;
      readonly checkedFiles: number;
      readonly validFiles: number;
    }
  | {
      readonly type: "development_language_restarted";
      readonly id: string;
    }
  | {
      readonly type: "development_validation_completed";
      readonly id: string;
      readonly errors: number;
      readonly warnings: number;
    }
  | {
      readonly type: "development_repair_requested";
      readonly id: string;
      readonly iteration: number;
    }
  | {
      readonly type: "development_completed";
      readonly id: string;
      readonly status: DevelopmentStatus;
    };

/** Immutable prepared workflow start, shown before the one-time approval. */
export interface GDScriptDevelopmentPreview {
  readonly request: string;
  readonly projectName: string | null;
  readonly projectFingerprint: string;
  readonly engineVersion: string | null;
  readonly engineFingerprint: string | null;
  readonly limits: {
    readonly maxIterations: number;
    readonly maxRepairProposals: number;
    readonly maxFilesPerChangeSet: number;
  };
  readonly authorization: {
    readonly sourceWrites: "each change set approved separately";
    readonly languageSession: "read-only; recreated after approved edits under this approval";
    readonly checkOnlyParsing: "covered";
    readonly apiLookup: "covered";
    readonly workspaceInspection: "covered";
    readonly gitInspection: "covered";
    readonly network: "denied";
    readonly gameExecution: "disabled";
  };
}

/** Immutable digest parts the workflow-start approval binds to. */
export interface GDScriptDevelopmentDigestParts {
  readonly request: string;
  readonly projectFingerprint: string;
  readonly engineFingerprint: string | null;
  readonly limits: {
    readonly maxIterations: number;
    readonly maxRepairProposals: number;
    readonly maxFilesPerChangeSet: number;
  };
  readonly authorizationPolicyVersion: number;
}

/** Deterministic digest over the immutable workflow start (§20 shape). */
export function computeGDScriptDevelopmentDigest(parts: GDScriptDevelopmentDigestParts): string {
  return sha256Hex(canonicalizeJson(parts));
}
