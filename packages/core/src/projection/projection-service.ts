import type { ConversationItem } from "../domain/conversation.js";
import type { ToolExecutionResult } from "../tools/tool.js";
import type { RegisteredToolInfo } from "../tools/tool-registry.js";
import type { TaskState } from "../tasks/task-model.js";
import type { ContextCapacity } from "./context-capacity.js";
import { estimateTokens } from "./context-estimator.js";
import { estimateConversationTokens } from "./conversation-trim.js";
import {
  createContextProjector,
  serializeContextPrefix,
  SOLARIS_SYSTEM_INSTRUCTIONS,
  type ContextProjection,
  type ContextProjector,
  type ContextStability,
} from "./context-projector.js";
import {
  classifyPressure,
  DEFAULT_CONTEXT_PRESSURE_LIMITS,
  type ContextPressure,
  type ContextPressureLimits,
} from "./context-pressure.js";
import {
  createEvidenceProjector,
  type EvidenceProjector,
  type EvidenceProjectionOptions,
  type ModelEvidenceView,
} from "./evidence-projector.js";
import { trimConversationPreservingPairs } from "./conversation-trim.js";
import { createWatermarkCache, type WatermarkCache } from "./watermark-cache.js";
import { createRevisionGuard } from "./stale-result.js";
import { sha256Hex } from "../godot/digest.js";
import { canonicalizeJson } from "../godot/digest.js";
import {
  createToolProjector,
  type ProjectionMode,
  type ToolProjector,
  type ToolProjection,
} from "./tool-projector.js";
import type { CapabilityPolicy } from "../security/capability.js";
import type { SandboxProfile } from "../security/profile.js";
import type { ResolvedInstructionSet } from "../instructions/instruction-model.js";
import { renderResolvedInstructions } from "../instructions/instruction-model.js";
import type {
  KnowledgeRetrievalQuery,
  KnowledgeRetrievalResult,
  ProjectKnowledgeFact,
} from "../knowledge/knowledge-model.js";
import {
  renderPinnedKnowledge,
  renderRetrievedKnowledge,
} from "../knowledge/knowledge-projection.js";

export type { ProjectionMode } from "./tool-projector.js";

/**
 * ProjectionService (Stage 3 milestone 2).
 *
 * The single application-owned composition of the three projection
 * boundaries. It builds the provider request exactly once per turn:
 *
 *   project -> estimate -> classify pressure -> fit or reduce -> provider
 *
 * The service never mutates authoritative state: it reads task snapshots
 * through a host-provided getter, projects disposable copies, and blocks
 * (rather than trims) requests that cannot fit. Provider adapters receive
 * already-projected provider-neutral inputs and never make their own
 * relevance/visibility/redaction decisions.
 */

export interface ProjectionServiceOptions {
  readonly policy: CapabilityPolicy;
  readonly profile: SandboxProfile;
  readonly capacity: ContextCapacity;
  readonly pressureLimits?: ContextPressureLimits;
  /** Read the current task snapshot for contextual segments; null when idle. */
  readonly getTaskSnapshot?: () => TaskState | null;
  /** Read the current task contract request text for contextual segments. */
  readonly getTaskRequest?: () => string | null;
  readonly evidence?: EvidenceProjectionOptions;
  /**
   * Host-owned project-instruction projection. The service consumes a
   * resolved set; it never resolves or discovers instructions itself.
   */
  readonly instructions?: {
    /** Resolve instructions for the given task focus paths; null when none. */
    resolve: (focusPaths: readonly string[]) => ResolvedInstructionSet | null;
  };
  /**
   * Host-owned knowledge projection. The service consumes bounded
   * projections; the KnowledgeCoordinator stays the single writer.
   */
  readonly knowledge?: {
    /** Bounded pinned facts for stable/contextual context. */
    pinned: () => readonly ProjectKnowledgeFact[];
    /** Deterministic bounded retrieval for the current turn. */
    retrieve: (query: KnowledgeRetrievalQuery) => KnowledgeRetrievalResult;
  }; /** Deterministic stable instructions; defaults to SOLARIS_SYSTEM_INSTRUCTIONS. */
  readonly stableInstructions?: string;
  readonly contextProjector?: ContextProjector;
  readonly toolProjector?: ToolProjector;
  readonly evidenceProjector?: EvidenceProjector;
}

export interface ProjectedRequest {
  readonly mode: ProjectionMode;
  readonly messages: readonly ConversationItem[];
  readonly tools: readonly RegisteredToolInfo[];
  readonly system: string | null;
  readonly pressure: ContextPressure;
  readonly toolProjection: ToolProjection;
  readonly contextProjection: ContextProjection;
  readonly estimatedTokens: number;
  /** Non-null when the request must NOT be sent to the provider. */
  readonly blocked: { readonly type: "hard" | "unsupported"; readonly reason: string } | null;
}

export interface ProjectionService {
  projectRequest(input: {
    readonly mode: ProjectionMode;
    readonly messages: readonly ConversationItem[];
    readonly tools: readonly RegisteredToolInfo[];
    readonly providerToolCalling: boolean;
  }): ProjectedRequest;
  projectToolResult(input: {
    readonly toolName: string;
    readonly result: ToolExecutionResult;
    readonly evidenceId?: string;
  }): ToolExecutionResult;
  /** Last projected request for observability (/context, status). */
  lastProjection(): ProjectedRequest | null;
  /** Disposable model-evidence view cache size (never durable evidence). */
  evidenceCacheSize(): number;
}

/** Modes whose workflows require tool calling; no silent text-only fallback. */
const TOOL_REQUIRING_MODES: readonly ProjectionMode[] = ["development", "review", "inspection"];

function taskContextSegments(
  snapshot: TaskState | null,
  request: string | null,
): Array<{
  readonly id: string;
  readonly stability: ContextStability;
  readonly title: string;
  readonly content: string;
}> {
  if (snapshot === null && request === null) {
    return [];
  }
  const segments: Array<{
    readonly id: string;
    readonly stability: ContextStability;
    readonly title: string;
    readonly content: string;
  }> = [];
  if (request !== null) {
    segments.push({
      id: "task-contract",
      stability: "contextual",
      title: "Task contract",
      content: request,
    });
  }
  if (snapshot !== null) {
    const acceptance = snapshot.acceptance
      .map(
        (criterion) =>
          `- ${criterion.criterionId} (${criterion.verificationKind}): ${criterion.status}`,
      )
      .join("\n");
    const steps = snapshot.steps
      .map((step) => `- ${step.id} (${step.kind}): ${step.status}`)
      .join("\n");
    const findings =
      snapshot.currentFindings.length === 0
        ? "none"
        : snapshot.currentFindings
            .map((finding) => `- ${finding.findingId} (${finding.severity})`)
            .join("\n");
    segments.push({
      id: "task-state",
      stability: "contextual",
      title: "Task state",
      content: [
        `Task: ${snapshot.taskId}`,
        `Contract revision: ${snapshot.contractRevision}`,
        `Phase: ${snapshot.phase}`,
        `Validation: ${snapshot.validationStatus}`,
        `Review: ${snapshot.reviewStatus}`,
        "Acceptance:",
        acceptance,
        "Steps:",
        steps,
        "Findings:",
        findings,
      ].join("\n"),
    });
  }
  return segments;
}

function volatileTaskSegments(snapshot: TaskState | null): Array<{
  readonly id: string;
  readonly stability: ContextStability;
  readonly title: string;
  readonly content: string;
}> {
  if (snapshot === null) {
    return [];
  }
  const latest = snapshot.evidence[snapshot.evidence.length - 1];
  if (latest === undefined) {
    return [];
  }
  const revision =
    latest.source.type === "workspace_read" || latest.source.type === "mutation"
      ? latest.source.revision
      : undefined;
  return [
    {
      id: "current-task-evidence",
      stability: "volatile",
      title: "Latest evidence",
      content: `evidence ${latest.id} (${latest.kind}) attached${revision === undefined ? "" : ` @ ${revision}`}`,
    },
  ];
}

export function createProjectionService(options: ProjectionServiceOptions): ProjectionService {
  const policy = options.policy;
  const profile = options.profile;
  const capacity = options.capacity;
  const pressureLimits = options.pressureLimits ?? DEFAULT_CONTEXT_PRESSURE_LIMITS;
  const contextProjector = options.contextProjector ?? createContextProjector();
  const toolProjector = options.toolProjector ?? createToolProjector({ policy, profile });
  const evidenceProjector = options.evidenceProjector ?? createEvidenceProjector(options.evidence);
  const stableInstructions = options.stableInstructions ?? SOLARIS_SYSTEM_INSTRUCTIONS;
  const getTaskSnapshot = options.getTaskSnapshot ?? (() => null);
  const getTaskRequest = options.getTaskRequest ?? (() => null);
  // Disposable model-evidence view cache with high/low watermark hysteresis.
  // It never holds durable evidence: task evidence records and the raw
  // history remain authoritative and are unaffected by eviction.
  const evidenceCache: WatermarkCache<ModelEvidenceView> = createWatermarkCache({
    highWatermark: 64,
    lowWatermark: 32,
  });
  // Revision-bound cache semantics: evidence views computed under one task
  // contract revision are stale under the next. The guard is the single
  // production consumer of the stale-result mechanism; advancing the
  // contract revision invalidates the disposable views, never the raw
  // evidence.
  const cacheGuard = createRevisionGuard(1);
  let cacheBoundRevision: number | null = null;
  let last: ProjectedRequest | null = null;

  function projectToolResultLocked(input: {
    readonly toolName: string;
    readonly result: ToolExecutionResult;
    readonly evidenceId?: string;
  }): ToolExecutionResult {
    const contractRevision = getTaskSnapshot()?.contractRevision ?? null;
    if (contractRevision !== cacheBoundRevision) {
      evidenceCache.clear();
      cacheBoundRevision = contractRevision;
      cacheGuard.advance();
    }
    // Workspace-derived results (reads/structural/summary) carry an opaque
    // revision handle in their output; the model view preserves it so
    // revision-aware evidence stays distinguishable.
    const revision =
      input.result.status === "success" &&
      typeof input.result.output === "object" &&
      input.result.output !== null &&
      !Array.isArray(input.result.output) &&
      typeof (input.result.output as Record<string, unknown>)["revision"] === "string"
        ? ((input.result.output as Record<string, string>)["revision"] as string)
        : undefined;
    const result = input.result;
    if (result.status === "success") {
      const key = `${input.toolName}:${sha256Hex(canonicalizeJson(result))}`;
      const cached = evidenceCache.get(key);
      const view =
        cached ??
        evidenceProjector.projectForModel({
          ...(input.evidenceId === undefined ? {} : { evidenceId: input.evidenceId }),
          ...(revision === undefined ? {} : { revision }),
          rawText: result.summary,
        });
      evidenceCache.set(key, view);
      return { ...result, summary: view.text };
    }
    const view = evidenceProjector.projectForModel({ rawText: result.message });
    return { ...result, message: view.text };
  }

  /** Paths the current task demonstrably focused on (from read evidence). */
  function taskFocusPaths(snapshot: TaskState | null): readonly string[] {
    if (snapshot === null) {
      return [];
    }
    const paths: string[] = [];
    const seen = new Set<string>();
    for (const record of snapshot.evidence) {
      if (record.source.type !== "workspace_read") {
        continue;
      }
      for (const path of record.source.paths) {
        if (!seen.has(path)) {
          seen.add(path);
          paths.push(path);
        }
        if (paths.length >= 8) {
          return paths;
        }
      }
    }
    return paths;
  }

  function instructionSegments(
    snapshot: TaskState | null,
    resolve: (focusPaths: readonly string[]) => ResolvedInstructionSet | null,
  ): Array<{
    readonly id: string;
    readonly stability: ContextStability;
    readonly title: string;
    readonly content: string;
  }> {
    const focusPaths = taskFocusPaths(snapshot);
    const resolved = resolve(focusPaths.length === 0 ? ["."] : focusPaths);
    if (resolved === null || resolved.instructions.length === 0) {
      return [];
    }
    return [
      {
        id: "project-instructions",
        stability: "contextual",
        title: "Project instructions",
        content: renderResolvedInstructions(resolved),
      },
    ];
  }

  function knowledgeSegments(
    snapshot: TaskState | null,
    request: string | null,
    knowledge: {
      pinned: () => readonly ProjectKnowledgeFact[];
      retrieve: (query: KnowledgeRetrievalQuery) => KnowledgeRetrievalResult;
    },
  ): Array<{
    readonly id: string;
    readonly stability: ContextStability;
    readonly title: string;
    readonly content: string;
  }> {
    const segments: Array<{
      readonly id: string;
      readonly stability: ContextStability;
      readonly title: string;
      readonly content: string;
    }> = [];
    const pinned = knowledge.pinned();
    if (pinned.length > 0) {
      segments.push({
        id: "pinned-project-knowledge",
        stability: "contextual",
        title: "Project knowledge",
        content: renderPinnedKnowledge(pinned),
      });
    }
    // Retrieval basis is task-stable: the task request and the paths the
    // task demonstrably read. Per-turn user phrasing is intentionally NOT a
    // signal, so retrieval stays deterministic within a task and the
    // contextual prefix does not churn between turns.
    const queryText = request?.trim() ?? "";
    if (queryText.length > 0) {
      const retrieved = knowledge.retrieve({
        text: queryText,
        paths: taskFocusPaths(snapshot),
      });
      if (retrieved.facts.length > 0) {
        segments.push({
          id: "retrieved-project-knowledge",
          stability: "contextual",
          title: "Task-relevant knowledge",
          content: renderRetrievedKnowledge(retrieved),
        });
      }
    }
    return segments;
  }

  function projectContext(): ContextProjection {
    const snapshot = getTaskSnapshot();
    const request = getTaskRequest();
    const segments = [
      {
        id: "solaris-core-instructions",
        stability: "stable" as const,
        title: "Solaris instructions",
        content: stableInstructions,
      },
      ...(options.instructions === undefined
        ? []
        : instructionSegments(snapshot, options.instructions.resolve)),
      ...(options.knowledge === undefined
        ? []
        : knowledgeSegments(snapshot, request, options.knowledge)),
      ...taskContextSegments(snapshot, request),
      ...volatileTaskSegments(snapshot),
    ];
    return contextProjector.project({ segments });
  }

  return {
    projectRequest(input: {
      readonly mode: ProjectionMode;
      readonly messages: readonly ConversationItem[];
      readonly tools: readonly RegisteredToolInfo[];
      readonly providerToolCalling: boolean;
    }): ProjectedRequest {
      // Tool-calling compatibility fails clearly before any request is built:
      // a development task must never silently degrade into a text-only session.
      if (!input.providerToolCalling && TOOL_REQUIRING_MODES.includes(input.mode)) {
        const contextProjection = projectContext();
        const emptyTools: ToolProjection = {
          fingerprint: "unsupported",
          tools: [],
          counts: { available: 0, gated: 0, hidden: 0 },
          requestTools: [],
        };
        const blockedRequest: ProjectedRequest = {
          mode: input.mode,
          messages: [...input.messages],
          tools: input.tools,
          system: serializeContextPrefix(contextProjection),
          pressure: classifyPressure(0, capacity.workingMaximum, pressureLimits),
          toolProjection: emptyTools,
          contextProjection,
          estimatedTokens: 0,
          blocked: {
            type: "unsupported",
            reason:
              "The selected provider route does not support tool calling, which this task requires; the session cannot proceed with hidden or missing tools.",
          },
        };
        last = blockedRequest;
        return blockedRequest;
      }

      const contextProjection = projectContext();
      const toolProjection = toolProjector.project({
        mode: input.mode,
        registeredTools: input.tools,
      });
      const hiddenNames = new Set(
        toolProjection.tools
          .filter((tool) => tool.visibility === "hidden")
          .map((tool) => tool.name),
      );
      const projectedTools = input.tools.filter((info) => !hiddenNames.has(info.definition.name));
      const systemText = serializeContextPrefix(contextProjection);
      const systemTokens = estimateTokens(systemText);
      const toolTokens = projectedTools.reduce(
        (sum, info) =>
          sum +
          estimateTokens(info.definition.name) +
          estimateTokens(info.definition.description) +
          estimateTokens(JSON.stringify(info.definition.inputSchema)),
        0,
      );
      let messages = input.messages;
      let estimatedTokens = systemTokens + estimateConversationTokens(messages) + toolTokens;
      let pressure = classifyPressure(estimatedTokens, capacity.workingMaximum, pressureLimits);
      let reduced = false;
      if (pressure.state === "auto" || pressure.state === "hard") {
        // Deterministic reduction: drop oldest tool-call/result pairs
        // (whole units) until the working budget fits. The message budget
        // accounts for the irreducible system prefix (stable + contextual);
        // authoritative TaskState is untouched.
        const messageBudget = Math.max(0, capacity.workingMaximum - systemTokens - toolTokens);
        const trimmed = trimConversationPreservingPairs(messages, messageBudget);
        if (trimmed.droppedItems > 0) {
          messages = trimmed.items;
          reduced = true;
          estimatedTokens = systemTokens + trimmed.estimatedTokens + toolTokens;
          pressure = classifyPressure(estimatedTokens, capacity.workingMaximum, pressureLimits);
        }
      }
      // Evidence projection applies to the disposable request copy: the
      // raw history and task evidence records stay untouched, while the
      // model sees bounded/redacted result summaries with truncation
      // disclosure.
      const projectedMessages = messages.map((item) => {
        if (item.type !== "tool_result") {
          return item;
        }
        const projectedResult = projectToolResultLocked({
          toolName: item.toolName,
          result: item.result,
        });
        return { ...item, result: projectedResult };
      });
      const projected: ProjectedRequest = {
        mode: input.mode,
        messages: projectedMessages,
        tools: projectedTools,
        system: systemText,
        pressure,
        toolProjection,
        contextProjection,
        estimatedTokens,
        blocked:
          pressure.state === "hard"
            ? {
                type: "hard",
                reason: `Projected context is ${estimatedTokens} tokens against a working maximum of ${capacity.workingMaximum}; the provider call was blocked.${reduced ? " (reduction was already applied)" : ""}`,
              }
            : null,
      };
      last = projected;
      return projected;
    },

    projectToolResult(input: {
      readonly toolName: string;
      readonly result: ToolExecutionResult;
      readonly evidenceId?: string;
    }): ToolExecutionResult {
      return projectToolResultLocked(input);
    },

    lastProjection(): ProjectedRequest | null {
      return last;
    },

    evidenceCacheSize(): number {
      return evidenceCache.size;
    },
  };
}
