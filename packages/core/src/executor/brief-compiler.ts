import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import { deepFreeze } from "../domain/deep-freeze.js";
import { sanitizeSecretsOnly } from "../doctor/safe-report.js";
import type { TaskContract } from "../tasks/task-contract.js";
import type { ExecutionContract, ExecutionContractRef } from "./execution-contract.js";
import type { MilestoneManifest, MilestoneManifestRef } from "./milestone-manifest.js";
import type { CapabilityRef, ExecutorContextPack } from "./context-pack.js";

/**
 * ExecutorBriefCompiler (executor briefing foundation).
 *
 * Compiles the bounded, provider-neutral ExecutorBrief for one task from:
 *
 *   TaskContract + TaskPlan (via ExecutorContextPack) + ExecutionContract
 *   + MilestoneManifest
 *
 * The brief contains only the information required for the CURRENT task:
 * task-specific deliverables, verified/candidate touchpoints, milestone
 * invariants, non-goals, acceptance ids, milestone tests, relevant
 * architecture references, and capability limits — while permanent rules
 * are referenced as `Execution Contract rev N` instead of being restated.
 *
 * The compiler is deterministic (identical inputs -> identical brief),
 * imposes explicit output bounds (candidate/background context trims
 * before goal/invariants/acceptance/verified touchpoints), and never
 * grants capability: briefs carry no policy surface and the contract
 * revision is referenced by identity only. Provider adapters never run
 * the compiler; the host owns briefing semantics.
 */

export const EXECUTOR_BRIEF_SCHEMA_VERSION = 2;

export interface ExecutorBrief {
  readonly format: "solaris-executor-brief";
  /** Brief schema version (stable identity, not a content revision). */
  readonly version: number;
  readonly taskId: string;
  readonly contractRevision: number;
  /** Bounded task request text (the executable goal statement). */
  readonly request: string;
  readonly executionContract: ExecutionContractRef;
  readonly milestone: MilestoneManifestRef | null;
  /** Task-specific deliverables (from the milestone manifest). */
  readonly deliverables: readonly string[];
  readonly verifiedTouchpoints: readonly string[];
  readonly candidateTouchpoints: readonly string[];
  /** Task-specific invariants (from the milestone manifest). */
  readonly invariants: readonly string[];
  readonly nonGoals: readonly string[];
  /** Stable acceptance ids that must appear in the task brief. */
  readonly acceptanceIds: readonly string[];
  /** Milestone-specific test requirements (short descriptions). */
  readonly testRequirements: readonly string[];
  /** Relevant architecture references (doc paths). */
  readonly architectureReferences: readonly string[];
  /** Deterministically selected documentation (root/nested/architecture/ADRs). */
  readonly documentationSources: readonly string[];
  /** Current-step working-set files with their inclusion reasons. */
  readonly workingSetFiles: readonly string[];
  /** Verified workspace-scope files (exact/structural references). */
  readonly workspaceVerifiedFiles: readonly string[];
  /** Deterministic review signals (proliferation / unexplained expansion). */
  readonly scopeWarnings: readonly string[];
  /** New production files with their recorded rationales. */
  readonly newFileRationales: readonly string[];
  /**
   * Capability limits relevant to the current task: omitted for
   * capabilities that are available; listed as prohibitions/limits for
   * unavailable/blocked/unsupported ones. Empty when no snapshot exists.
   */
  readonly capabilityLimits: readonly string[];
  readonly plan: {
    readonly id: string;
    readonly revision: number;
    readonly approval: string;
  } | null;
  /** Path-scoped instruction sources included in this task's context. */
  readonly instructionSources: readonly string[];
}

/** Host-owned hard bounds for compiled briefs (deterministic, never raised). */
export const EXECUTOR_BRIEF_LIMITS = Object.freeze({
  maxRequestBytes: 1024,
  maxDeliverables: 8,
  maxDeliverableBytes: 512,
  maxTouchpoints: 12,
  maxInvariants: 12,
  maxInvariantBytes: 512,
  maxNonGoals: 12,
  maxAcceptanceIds: 32,
  maxTestRequirements: 8,
  maxArchitectureReferences: 4,
  maxCapabilityLimits: 8,
  maxInstructionSources: 8,
  maxRenderedBytes: 8 * 1024,
  maxDocumentationSources: 12,
  maxWorkingSetFiles: 8,
  maxWorkspaceVerifiedFiles: 12,
  maxScopeWarnings: 8,
  maxNewFileRationales: 8,
});

export interface CompileExecutorBriefInput {
  readonly contract: TaskContract;
  readonly executionContract: ExecutionContract;
  /** The derived context pack (task, plan, instructions, architecture, capabilities). */
  readonly pack: ExecutorContextPack;
  /** Milestone manifest; null for tasks with no milestone manifest. */
  readonly milestone?: MilestoneManifest | null;
}

const textEncoder = new TextEncoder();

function boundedStrings(values: readonly string[], max: number, maxBytes: number): string[] {
  const result: string[] = [];
  for (const value of values) {
    if (result.length >= max) {
      break;
    }
    const text = value.trim();
    if (text.length === 0) {
      continue;
    }
    result.push(textEncoder.encode(text).length > maxBytes ? `${text.slice(0, 240)}\u2026` : text);
  }
  return result;
}

function capabilityLimitLines(capabilities: CapabilityRef): readonly string[] {
  if (!capabilities.available) {
    return [];
  }
  const lines: string[] = [];
  for (const entry of capabilities.states) {
    if (entry.state === "available" || lines.length >= EXECUTOR_BRIEF_LIMITS.maxCapabilityLimits) {
      continue;
    }
    if (entry.state === "blocked_by_policy") {
      lines.push(`${entry.area}: denied by policy`);
    } else if (entry.state === "unavailable" || entry.state === "unsupported") {
      lines.push(`${entry.area}: unavailable`);
    } else if (entry.state === "degraded") {
      lines.push(`${entry.area}: degraded`);
    } else {
      lines.push(`${entry.area}: ${entry.state}`);
    }
  }
  return lines;
}

/**
 * Compile the brief for one task. Deterministic and bounded: identical
 * inputs produce byte-identical briefs (no timestamps, no volatile state),
 * and trimming follows the priority order — goal/invariants/acceptance/
 * verified touchpoints are kept before candidate/background context.
 */
export function compileExecutorBrief(input: CompileExecutorBriefInput): ExecutorBrief {
  const milestone = input.milestone ?? null;
  const request = input.contract.request.trim();
  const brief: ExecutorBrief = {
    format: "solaris-executor-brief",
    version: EXECUTOR_BRIEF_SCHEMA_VERSION,
    taskId: input.contract.id,
    contractRevision: input.contract.revision,
    request:
      textEncoder.encode(request).length > EXECUTOR_BRIEF_LIMITS.maxRequestBytes
        ? `${request.slice(0, 480)}\u2026`
        : request,
    executionContract: {
      id: input.executionContract.id,
      revision: input.executionContract.revision,
    },
    milestone: milestone === null ? null : { id: milestone.id, version: milestone.version },
    deliverables: boundedStrings(
      milestone?.deliverables.map((deliverable) => deliverable.description) ?? [],
      EXECUTOR_BRIEF_LIMITS.maxDeliverables,
      EXECUTOR_BRIEF_LIMITS.maxDeliverableBytes,
    ),
    verifiedTouchpoints: boundedStrings(
      input.pack.verifiedTouchpoints.map((touchpoint) => touchpoint.path),
      EXECUTOR_BRIEF_LIMITS.maxTouchpoints,
      512,
    ),
    candidateTouchpoints: boundedStrings(
      input.pack.candidateTouchpoints.map((touchpoint) => touchpoint.path),
      EXECUTOR_BRIEF_LIMITS.maxTouchpoints,
      512,
    ),
    invariants: boundedStrings(
      milestone?.invariants.map((invariant) => invariant.description) ?? [],
      EXECUTOR_BRIEF_LIMITS.maxInvariants,
      EXECUTOR_BRIEF_LIMITS.maxInvariantBytes,
    ),
    nonGoals: boundedStrings(
      milestone?.nonGoals ?? [],
      EXECUTOR_BRIEF_LIMITS.maxNonGoals,
      EXECUTOR_BRIEF_LIMITS.maxInvariantBytes,
    ),
    acceptanceIds: boundedStrings(
      milestone?.acceptance.map((requirement) => requirement.id) ?? [],
      EXECUTOR_BRIEF_LIMITS.maxAcceptanceIds,
      128,
    ),
    testRequirements: boundedStrings(
      milestone?.requiredTests.map((test) => test.description) ?? [],
      EXECUTOR_BRIEF_LIMITS.maxTestRequirements,
      EXECUTOR_BRIEF_LIMITS.maxInvariantBytes,
    ),
    architectureReferences: boundedStrings(
      input.pack.architecture.map((entry) => entry.path),
      EXECUTOR_BRIEF_LIMITS.maxArchitectureReferences,
      512,
    ),
    documentationSources: boundedStrings(
      [
        ...(input.pack.documentation?.rootAgents ?? []),
        ...(input.pack.documentation?.nestedAgents ?? []),
        ...(input.pack.documentation?.architectureDocs ?? []),
        ...(input.pack.documentation?.adrs ?? []),
        ...(input.pack.documentation?.developmentDocs ?? []),
      ],
      EXECUTOR_BRIEF_LIMITS.maxDocumentationSources,
      512,
    ),
    workingSetFiles: boundedStrings(
      (input.pack.activeWorkingSet?.files ?? []).map((file) => `${file.path} (${file.reason})`),
      EXECUTOR_BRIEF_LIMITS.maxWorkingSetFiles,
      512,
    ),
    workspaceVerifiedFiles: boundedStrings(
      (input.pack.workspaceScope?.verifiedFiles ?? []).map((file) => file.path),
      EXECUTOR_BRIEF_LIMITS.maxWorkspaceVerifiedFiles,
      512,
    ),
    scopeWarnings: boundedStrings(
      (input.pack.scopeSignals ?? []).map((signal) => `${signal.id}: ${signal.message}`),
      EXECUTOR_BRIEF_LIMITS.maxScopeWarnings,
      512,
    ),
    newFileRationales: boundedStrings(
      (input.pack.newFiles ?? []).map(
        (file) =>
          `${file.path} — ${file.reason} (owners: ${file.existingOwnersInspected.join(", ") || "none"})`,
      ),
      EXECUTOR_BRIEF_LIMITS.maxNewFileRationales,
      512,
    ),
    capabilityLimits: capabilityLimitLines(input.pack.capabilities),
    plan:
      input.pack.plan === undefined
        ? null
        : {
            id: input.pack.plan.id,
            revision: input.pack.plan.revision,
            approval: input.pack.plan.approval,
          },
    instructionSources: boundedStrings(
      input.pack.instructions.map((instruction) => instruction.source),
      EXECUTOR_BRIEF_LIMITS.maxInstructionSources,
      256,
    ),
  };
  return deepFreeze(brief);
}

/** Deterministic fingerprint over the brief's canonical form. */
export function computeExecutorBriefFingerprint(brief: ExecutorBrief): string {
  return sha256Hex(canonicalizeJson(brief));
}

/**
 * Rendered text representation of the brief: the concise executor input.
 * Under budget pressure the renderer drops whole sections in priority
 * order — capability limits, tests, architecture references, non-goals,
 * candidates, then deliverables — before ever touching task/goal/
 * invariants/acceptance/verified touchpoints (executor briefing §20), and
 * finally truncates the tail with an explicit marker.
 */
export function renderExecutorBrief(
  brief: ExecutorBrief,
  maxBytes: number = EXECUTOR_BRIEF_LIMITS.maxRenderedBytes,
): string {
  // Drop priority: 0 = never dropped by section trimming.
  const sections: Array<{
    readonly title: string;
    readonly content: string;
    readonly priority: number;
  }> = [
    { title: "TASK", content: brief.request, priority: 0 },
    {
      title: "EXECUTION CONTRACT",
      content: `Execution Contract: ${brief.executionContract.id} rev ${brief.executionContract.revision}`,
      priority: 0,
    },
  ];
  if (brief.milestone !== null) {
    sections.push({
      title: "MILESTONE",
      content: `Milestone Manifest: ${brief.milestone.id} rev ${brief.milestone.version}`,
      priority: 0,
    });
  }
  if (brief.plan !== null) {
    sections.push({
      title: "PLAN",
      content: `Plan: ${brief.plan.id} rev ${brief.plan.revision} (${brief.plan.approval})`,
      priority: 0,
    });
  }
  if (brief.invariants.length > 0) {
    sections.push({
      title: "TASK-SPECIFIC INVARIANTS",
      content: bullet(brief.invariants),
      priority: 0,
    });
  }
  if (brief.acceptanceIds.length > 0) {
    sections.push({ title: "ACCEPTANCE", content: brief.acceptanceIds.join(", "), priority: 0 });
  }
  if (brief.verifiedTouchpoints.length > 0) {
    sections.push({
      title: "VERIFIED TOUCHPOINTS",
      content: bullet(brief.verifiedTouchpoints),
      priority: 1,
    });
  }
  if (brief.workspaceVerifiedFiles.length > 0) {
    sections.push({
      title: "VERIFIED WORKSPACE FILES",
      content: bullet(brief.workspaceVerifiedFiles),
      priority: 1,
    });
  }
  if (brief.workingSetFiles.length > 0) {
    sections.push({
      title: "WORKING SET (CURRENT STEP)",
      content: bullet(brief.workingSetFiles),
      priority: 3,
    });
  }
  if (brief.deliverables.length > 0) {
    sections.push({ title: "DELIVERABLES", content: bullet(brief.deliverables), priority: 2 });
  }
  if (brief.candidateTouchpoints.length > 0) {
    sections.push({
      title: "CANDIDATE TOUCHPOINTS",
      content: bullet(brief.candidateTouchpoints),
      priority: 3,
    });
  }
  if (brief.nonGoals.length > 0) {
    sections.push({ title: "NON-GOALS", content: bullet(brief.nonGoals), priority: 4 });
  }
  if (brief.architectureReferences.length > 0) {
    sections.push({
      title: "ARCHITECTURE REFERENCES",
      content: bullet(brief.architectureReferences),
      priority: 5,
    });
  }
  if (brief.documentationSources.length > 0) {
    sections.push({
      title: "DOCUMENTATION",
      content: bullet(brief.documentationSources),
      priority: 5,
    });
  }
  if (brief.newFileRationales.length > 0) {
    sections.push({
      title: "NEW FILES (RATIONALE)",
      content: bullet(brief.newFileRationales),
      priority: 6,
    });
  }
  if (brief.testRequirements.length > 0) {
    sections.push({
      title: "MILESTONE-SPECIFIC TESTS",
      content: bullet(brief.testRequirements),
      priority: 6,
    });
  }
  if (brief.capabilityLimits.length > 0) {
    sections.push({
      title: "CAPABILITY LIMITS",
      content: bullet(brief.capabilityLimits),
      priority: 7,
    });
  }
  if (brief.scopeWarnings.length > 0) {
    sections.push({
      title: "SCOPE WARNINGS",
      content: bullet(brief.scopeWarnings),
      priority: 7,
    });
  }
  const joined = sections.map(({ title, content }) => `${title}\n${content}`).join("\n\n");
  // The rendered brief is the provider/terminal boundary: known
  // credential-shaped tokens are redacted before projection (secrets
  // never enter brief/docs projection; the structured brief stays
  // host-internal).
  const sanitized = sanitizeSecretsOnly(joined);
  if (textEncoder.encode(sanitized).length <= maxBytes) {
    return sanitized;
  }
  // Drop whole low-priority sections from the end first, then truncate.
  let kept = sections.filter((section) => section.priority === 0);
  for (let priority = 1; priority <= 7 && renderedBytes(kept) > maxBytes; priority += 1) {
    const next = [...kept, ...sections.filter((section) => section.priority === priority)];
    kept = next;
    if (renderedBytes(kept) <= maxBytes) {
      break;
    }
    while (kept.length > 0 && kept[kept.length - 1]!.priority === priority) {
      kept.pop();
    }
  }
  const rendered = kept.map(({ title, content }) => `${title}\n${content}`).join("\n\n");
  return sanitizeSecretsOnly(trimRendered(rendered, maxBytes));
}

function renderedBytes(sections: readonly { title: string; content: string }[]): number {
  return textEncoder.encode(
    sections.map(({ title, content }) => `${title}\n${content}`).join("\n\n"),
  ).length;
}

function bullet(values: readonly string[]): string {
  return values.map((value) => `- ${value}`).join("\n");
}

function trimRendered(rendered: string, maxBytes: number): string {
  const marker = "\n\u2026 [brief truncated]";
  const markerBytes = textEncoder.encode(marker).length;
  if (maxBytes <= markerBytes) {
    return marker.slice(0, Math.max(1, maxBytes));
  }
  // Render is deterministic; truncating whole rendered lines keeps
  // sections scannable.
  const lines = rendered.split("\n");
  let kept = "";
  for (const line of lines) {
    const candidate = kept.length === 0 ? line : `${kept}\n${line}`;
    if (textEncoder.encode(`${candidate}${marker}`).length > maxBytes) {
      break;
    }
    kept = candidate;
  }
  return `${kept}${marker}`;
}

/** Compact deterministic description of a brief's shape (for reports). */
export function summarizeExecutorBrief(brief: ExecutorBrief): string {
  return [
    `${brief.executionContract.id} rev ${brief.executionContract.revision}`,
    brief.milestone === null
      ? "no milestone"
      : `${brief.milestone.id} rev ${brief.milestone.version}`,
    `${brief.verifiedTouchpoints.length} verified / ${brief.candidateTouchpoints.length} candidate touchpoints`,
    `${brief.acceptanceIds.length} acceptance ids`,
  ].join(", ");
}
