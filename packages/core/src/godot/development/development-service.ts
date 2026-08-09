import type { ChangePreview } from "../../security/change-preview.js";
import type { ToolExecutionContext } from "../../tools/tool.js";
import type { DevelopmentQualityReport } from "../quality/quality-model.js";
import type { ChangeReviewResult } from "../quality/quality-review.js";
import type {
  DevelopmentEvent,
  DevelopmentValidationStatus,
  GDScriptDevelopmentPreview,
  GDScriptDevelopmentResult,
  GDScriptDevelopmentSession,
  GDScriptDevelopmentStatus,
} from "./development-model.js";

/**
 * Provider-neutral GDScript development workflow port.
 *
 * The workflow orchestrates existing capabilities — workspace
 * inspection, Godot API knowledge, GDScript LSP intelligence, approved
 * exact text change sets, checkpoints, `--check-only` parsing, fresh LSP
 * sessions, validation evidence, and bounded repair iterations — without
 * bypassing any of them: every mutation still requires its own exact
 * one-time approval, every change set is still checkpointed before
 * application, and validation gates cannot be reordered or omitted.
 *
 * The workflow implementation lives in the adapters; core owns the
 * vocabulary, the immutable limits, the digest contract, and this port.
 * The workflow never mutates the workspace directly: `prepareChangeSet`
 * freezes an immutable plan (read-only), the one-time approval binds to
 * its digest, and `applyChangeSet` executes only under the approved
 * digest through the change-set applier (which fails closed as
 * unavailable until a mechanically identity-bound commit primitive
 * exists).
 */

export type DevelopmentSupportState = "available" | "unavailable";

export interface DevelopmentSupport {
  readonly state: DevelopmentSupportState;
  /** Exact reason when unavailable; null when available. */
  readonly reason: string | null;
  readonly platform: string;
}

export type DevelopmentStartPreparationResult =
  | {
      readonly status: "ready";
      readonly workflowId: string;
      readonly preview: GDScriptDevelopmentPreview;
      /** Full workflow-start digest; approval binds to exactly this. */
      readonly digest: string;
    }
  | {
      readonly status: "unavailable" | "invalid_input" | "conflict" | "failed";
      readonly message: string;
    };

export type DevelopmentStartResult =
  | {
      readonly status: "ready";
      readonly session: GDScriptDevelopmentSession;
    }
  | {
      readonly status: "denied" | "conflict" | "cancelled" | "unavailable" | "failed";
      readonly message: string;
    };

export type DevelopmentCancelResult =
  | {
      readonly status: "cancelled";
      readonly result: GDScriptDevelopmentResult | null;
    }
  | {
      readonly status: "inactive";
      readonly message: string;
    };

export type DevelopmentChangeSetPreparationResult =
  | {
      readonly status: "ready";
      readonly changeSetId: string;
      readonly preview: ChangePreview;
      /** SHA-256 over the immutable prepared change set; binds approval. */
      readonly digest: string;
      readonly repair: boolean;
    }
  | {
      readonly status:
        | "invalid_input"
        | "conflict"
        | "changeset_too_large"
        | "repair_budget_exhausted"
        | "iteration_budget_exhausted"
        | "unavailable"
        | "cancelled"
        | "failed";
      readonly message: string;
    };

export type DevelopmentChangeSetApplicationResult =
  | {
      readonly status: "applied";
      readonly result: GDScriptDevelopmentResult;
    }
  | {
      readonly status:
        | "denied"
        | "conflict"
        | "cancelled"
        | "apply_failed"
        | "validation_failed"
        | "unavailable"
        | "failed";
      readonly message: string;
      readonly result: GDScriptDevelopmentResult | null;
    };

export interface DevelopmentChangeSetExecutionContext {
  readonly approvedDigest: string;
  readonly signal?: AbortSignal;
}

/**
 * Bounded development workflow service. The application (and the CLI)
 * drives `start` through the one-time approval protocol; the provider
 * proposes changes through the change-set tool, which delegates here.
 * `completeFromProviderTurn` is invoked by the application when a
 * provider turn finishes without tool calls, so a cleanly reviewed
 * workflow terminates deterministically instead of lingering.
 */
export interface GDScriptDevelopmentService {
  support(): Promise<DevelopmentSupport>;

  /** Prepare the immutable workflow start (no approval yet). */
  prepareStart(request: string, signal?: AbortSignal): Promise<DevelopmentStartPreparationResult>;

  /** Start the workflow under the approved digest. */
  start(
    workflowId: string,
    context: { readonly approvedDigest: string; readonly signal?: AbortSignal },
  ): Promise<DevelopmentStartResult>;

  status(): GDScriptDevelopmentStatus;

  /**
   * Prepare an exact text change set in the active workflow. Read-only:
   * validates, reads, hashes, produces every resulting file in memory,
   * and freezes the immutable digest. Refuses before any approval when
   * the change-set applier is unavailable on this platform.
   */
  prepareChangeSet(
    input: unknown,
    context: ToolExecutionContext,
  ): Promise<DevelopmentChangeSetPreparationResult>;

  /**
   * Apply the approved change set exactly once under the approved digest:
   * suspend the language session, revalidate the source state, checkpoint
   * every affected file, apply sequentially with hash verification, run
   * the post-edit `--check-only` gate, restart a fresh language session,
   * collect bounded diagnostics, and record validation evidence.
   */
  applyChangeSet(
    changeSetId: string,
    context: DevelopmentChangeSetExecutionContext,
  ): Promise<DevelopmentChangeSetApplicationResult>;

  /**
   * Gate consulted by LSP query tools while the workflow suspends the
   * language session for an approved edit: new queries are rejected with
   * a typed outcome until the fresh session starts.
   */
  languageQueryGate(): { readonly blocked: boolean; readonly message: string | null };

  /** Most recent validation outcome; null before the first validation. */
  validationStatus(): DevelopmentValidationStatus | null;

  /** Most recent quality report; null before the quality stage ran. */
  qualityReport(): DevelopmentQualityReport | null;

  /**
   * Run a fresh independent review of the current tracked development
   * change (the `/review-change` command). Read-only: requires no write
   * approval, modifies nothing, and never starts a repair automatically.
   * Returns `validation_incomplete`-style outcomes through the result
   * statuses (`failed`/`cancelled`/`too_large`) when the review cannot
   * complete; a clean result never claims deterministic gates passed.
   */
  runIndependentReview(signal?: AbortSignal): Promise<ChangeReviewResult>;

  /** Application hook: a provider turn completed without tool calls. */
  completeFromProviderTurn(): void;

  cancel(signal?: AbortSignal): Promise<DevelopmentCancelResult>;

  /** Stop the workflow and dispose prepared state (session shutdown). */
  close(): Promise<void>;

  onEvent?(event: DevelopmentEvent): void;
}
