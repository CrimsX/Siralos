import type { ConversationItem } from "../domain/conversation.js";
import type { ToolExecutionResult } from "../tools/tool.js";
import type { RegisteredToolInfo } from "../tools/tool-registry.js";
import type { TaskState } from "../tasks/task-model.js";
import type { TaskPlan } from "../planning/planning-model.js";
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
import type { Reference } from "../reference/reference-model.js";
import type { ReferenceEvidenceView } from "../reference/reference-evidence.js";
import { formatReferenceEvidenceLine } from "../reference/reference-evidence.js";
import type { GodotSceneEvidenceView } from "../godot/scene/intelligence.js";
import type { ResearchEvidence } from "../research/research-service.js";
import { formatResearchEvidenceView } from "../research/research-service.js";
import { truncateText } from "./evidence-projector.js";

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
  /**
   * Read the current immutable plan for contextual rendering (Stage 3
   * milestone 7); null when absent. Only the CURRENT plan revision is ever
   * projected — historical revisions are never injected.
   */
  readonly getCurrentPlan?: () => TaskPlan | null;
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
  /**
   * Host-owned reference projection (Stage 3 milestone 5). The service
   * consumes bounded views; the registry stays the single owner of
   * reference identity.
   */
  readonly references?: {
    list: () => readonly Reference[];
    /** Bounded recent reference observations, oldest first. */
    latestEvidence: () => readonly ReferenceEvidenceView[];
  };
  /**
   * Host-owned research projection. The service consumes bounded views;
   * the ResearchService stays the single coordinator.
   */
  readonly research?: {
    /** Bounded recent research evidence, oldest first. */
    latestEvidence: () => readonly ResearchEvidence[];
  };
  /**
   * Host-owned Godot scene/resource evidence projection (Stage 3 milestone
   * 8). Bounded inspection observations recorded by the composition root;
   * the scene intelligence service stays the single owner of parsed state.
   */
  readonly scenes?: {
    /** Bounded recent scene/resource inspection observations, oldest first. */
    latestEvidence: () => readonly GodotSceneEvidenceView[];
  };
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
const TOOL_REQUIRING_MODES: readonly ProjectionMode[] = [
  "development",
  "review",
  "inspection",
  "planning",
];

/** Bounded rendering budget for the current-plan context segment. */
export const MAX_PLAN_SEGMENT_BYTES = 4 * 1024;

function planSegment(
  snapshot: TaskState | null,
  plan: TaskPlan | null,
): Array<{
  readonly id: string;
  readonly stability: ContextStability;
  readonly title: string;
  readonly content: string;
}> {
  const ref = snapshot?.plan;
  if (ref === undefined || ref === null || ref.state === "none") {
    return [];
  }
  const lines = [
    `Plan: ${ref.planId ?? "<unknown>"} rev ${ref.planRevision} (${ref.depth})`,
    `Plan state: ${ref.state}${ref.staleReason === null ? "" : ` — ${ref.staleReason}`}`,
    `Plan approval: ${ref.approval}`,
  ];
  if (plan !== null && plan.id === ref.planId) {
    const verified = plan.touchpoints
      .filter((touchpoint) => touchpoint.confidence === "verified")
      .map((touchpoint) => touchpoint.path);
    const candidates = plan.touchpoints
      .filter((touchpoint) => touchpoint.confidence === "candidate")
      .map((touchpoint) => touchpoint.path);
    lines.push(`Objective: ${plan.objective}`);
    if (verified.length > 0) {
      lines.push(`Verified: ${verified.join(", ")}`);
    }
    if (candidates.length > 0) {
      lines.push(`Candidate: ${candidates.join(", ")}`);
    }
    lines.push(`Steps: ${plan.steps.map((step) => `${step.id} ${step.title}`).join(" | ")}`);
    lines.push(`Validation: ${plan.validation.checks.join("; ")}`);
    if (plan.risks.length > 0) {
      lines.push(`Risks: ${plan.risks.map((risk) => risk.description).join("; ")}`);
    }
  }
  return [
    {
      id: "task-plan",
      stability: "contextual",
      title: "Task plan",
      content: truncateText(lines.join("\n"), MAX_PLAN_SEGMENT_BYTES).text,
    },
  ];
}

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

/** Combined volatile budget for the reference + research + scene evidence sections. */
export const REFERENCE_RESEARCH_VOLATILE_BUDGET_BYTES = 12 * 1024;
/** Most recent reference observations rendered per turn. */
export const MAX_REFERENCE_EVIDENCE_VIEWS = 4;
/** Most recent research evidence entries rendered per turn. */
export const MAX_RESEARCH_EVIDENCE_VIEWS = 4;
/** Most recent scene/resource inspection observations rendered per turn. */
export const MAX_SCENE_EVIDENCE_VIEWS = 4;

/**
 * Contextual `[Reference evidence]` + `[Research evidence]` + `[Scene
 * evidence]` sections, always composed AFTER `[Latest evidence]` and last
 * in the segment list. Content is bounded (4 most recent views/entries
 * each, combined 12 KiB budget with explicit `… [truncated]`); never
 * includes absolute cache paths, and never enters instruction/knowledge
 * sections. All three sections are CONTEXTUAL (not stable): they reach the
 * provider's system prefix — the milestone requires the model to see
 * evidence under data/evidence sections — while the stable fingerprint
 * (stable segments only) is unaffected by their content.
 */
function referenceResearchSegments(options: {
  readonly references?: {
    list: () => readonly Reference[];
    latestEvidence: () => readonly ReferenceEvidenceView[];
  };
  readonly research?: { latestEvidence: () => readonly ResearchEvidence[] };
  readonly scenes?: { latestEvidence: () => readonly GodotSceneEvidenceView[] };
}): Array<{
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
  const encoder = new TextEncoder();
  let referenceContent = "";
  let researchContent = "";
  let sceneContent = "";
  if (options.references !== undefined) {
    const views = options.references.latestEvidence().slice(-MAX_REFERENCE_EVIDENCE_VIEWS);
    referenceContent = views.map((view) => formatReferenceEvidenceLine(view)).join("\n");
  }
  if (options.research !== undefined) {
    const entries = options.research.latestEvidence().slice(-MAX_RESEARCH_EVIDENCE_VIEWS);
    researchContent = entries
      .map((entry) => formatResearchEvidenceView(entry, { maxBytes: 4 * 1024 }))
      .join("\n\n");
  }
  if (options.scenes !== undefined) {
    const views = options.scenes.latestEvidence().slice(-MAX_SCENE_EVIDENCE_VIEWS);
    sceneContent = views
      .map((view) => {
        const revision = view.revision === null ? "<no revision>" : view.revision;
        return `${view.path} @ ${revision} [${view.status}] ${view.summary}`;
      })
      .join("\n");
  }
  const referenceBytes = encoder.encode(referenceContent).length;
  const researchBytes = encoder.encode(researchContent).length;
  const sceneBytes = encoder.encode(sceneContent).length;
  if (referenceBytes + researchBytes + sceneBytes > REFERENCE_RESEARCH_VOLATILE_BUDGET_BYTES) {
    // Deterministic reduction order: research, then scene, then reference.
    let remaining = REFERENCE_RESEARCH_VOLATILE_BUDGET_BYTES - referenceBytes;
    if (remaining > 0) {
      researchContent = truncateText(researchContent, remaining).text;
    } else {
      researchContent = "";
    }
    remaining =
      REFERENCE_RESEARCH_VOLATILE_BUDGET_BYTES -
      referenceBytes -
      encoder.encode(researchContent).length;
    if (remaining > 0) {
      sceneContent = truncateText(sceneContent, remaining).text;
    } else {
      sceneContent = "";
    }
    referenceContent = truncateText(
      referenceContent,
      REFERENCE_RESEARCH_VOLATILE_BUDGET_BYTES,
    ).text;
  }
  if (referenceContent.length > 0) {
    segments.push({
      id: "reference-evidence",
      stability: "contextual",
      title: "Reference evidence",
      content: referenceContent,
    });
  }
  if (researchContent.length > 0) {
    segments.push({
      id: "research-evidence",
      stability: "contextual",
      title: "Research evidence",
      content: researchContent,
    });
  }
  if (sceneContent.length > 0) {
    segments.push({
      id: "scene-evidence",
      stability: "contextual",
      title: "Scene evidence",
      content: sceneContent,
    });
  }
  return segments;
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
  const getCurrentPlan = options.getCurrentPlan ?? (() => null);
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
      ...planSegment(snapshot, getCurrentPlan()),
      ...volatileTaskSegments(snapshot),
      // Reference/research evidence render AFTER [Latest evidence] and last.
      // The ContextProjector sorts segments by stability then id, and
      // current-task-evidence < reference-evidence < research-evidence
      // holds lexically, so the ordering is deterministic.
      ...referenceResearchSegments(options),
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
