import {
  describeInstructionScope,
  formatReferenceAlias,
  type ProjectionService,
  type Reference,
  type ReferenceMaterializerPort,
  type ReferenceRegistry,
  type ReferenceRevision,
  type ReferenceSource,
  type ResearchService,
  type ResearchSourcePort,
  type SolarisSecurity,
  type TaskState,
} from "@solaris/core";
import { sanitizeForDisplay } from "./sanitize.js";

function taskPhaseMark(phase: TaskState["phase"]): string {
  switch (phase) {
    case "prepared":
    case "working":
    case "validating":
    case "reviewing":
      return "\u25CF";
    case "blocked":
      return "\u23F3";
    case "completed":
      return "\u2713";
    case "cancelled":
      return "\u2715";
    case "failed":
      return "\u2717";
  }
}

function describeTaskProgress(state: TaskState["progress"]): string {
  return state.state === "healthy"
    ? "healthy"
    : state.state === "degraded"
      ? `degraded (${state.repeatedActions} repeated actions)`
      : `stalled (${state.repeatedActions} repeated actions)`;
}

/**
 * Host task status projection (Stage 3 milestone 1). The CLI is a
 * read-only client of the authoritative TaskState: it renders a snapshot
 * and the completion-gate evaluation, and never mutates task state.
 */
export function formatTaskStatus(
  task: TaskState,
  completion: { readonly allowed: boolean; readonly missing: readonly string[] },
): string {
  const activeStep = task.steps.find((step) => step.status === "active");
  const pendingSteps = task.steps.filter((step) => step.status !== "completed");
  const criteriaSatisfied = task.acceptance.filter(
    (criterion) => criterion.status === "satisfied",
  ).length;
  const phaseNote =
    task.phase === "blocked" && task.terminalReason !== null
      ? ` \u2014 ${task.terminalReason}`
      : "";
  const stepLines =
    task.steps.length === 0
      ? ["  (no structured steps)"]
      : task.steps.map((step) => {
          const mark =
            step.status === "completed"
              ? "\u2713"
              : step.status === "active"
                ? "\u25B8"
                : step.status === "failed"
                  ? "\u2717"
                  : step.status === "blocked"
                    ? "\u23F3"
                    : "\u00B7";
          return `  ${mark} ${step.id} ${step.status}${step.failedReason !== null ? ` (${step.failedReason})` : ""}`;
        });
  const criterionLines = task.acceptance.map((criterion) => {
    const mark =
      criterion.status === "satisfied"
        ? "\u2713"
        : criterion.status === "failed"
          ? "\u2717"
          : "\u00B7";
    const by = criterion.verifiedBy === null ? "" : ` [${criterion.verifiedBy}]`;
    return `  ${mark} ${criterion.criterionId} ${criterion.status}${by}`;
  });
  const completionLine = completion.allowed
    ? "Completion: allowed"
    : `Completion: NOT allowed (${completion.missing.length} reason${completion.missing.length === 1 ? "" : "s"})`;
  const identityLine =
    task.contractDigest === null
      ? `Identity: contract rev ${task.contractRevision}`
      : `Identity: contract rev ${task.contractRevision} / ${task.contractDigest.slice(0, 8)}\u2026${
          task.plan.planDigest === null
            ? ""
            : ` \u00B7 plan rev ${task.plan.planRevision} / ${task.plan.planDigest.slice(0, 8)}\u2026`
        }`;
  const planLines =
    task.plan.state === "none"
      ? []
      : [
          `Plan: ${task.plan.planId} rev ${task.plan.planRevision} (${task.plan.depth})`,
          `Plan state: ${task.plan.state}${task.plan.staleReason === null ? "" : ` \u2014 ${task.plan.staleReason}`}`,
          `Plan approval: ${task.plan.approval}`,
        ];
  return `Task ${task.taskId} (contract revision ${task.contractRevision})
${identityLine}
${taskPhaseMark(task.phase)} Phase: ${task.phase}${phaseNote}
${planLines.join("\n")}${planLines.length === 0 ? "" : "\n"}Steps: ${task.steps.length - pendingSteps.length}/${task.steps.length} completed${
    activeStep === undefined ? "" : ` \u2014 active: ${activeStep.id}`
  }
${stepLines.join("\n")}
Acceptance: ${criteriaSatisfied}/${task.acceptance.length} satisfied
${criterionLines.join("\n")}
Validation: ${task.validationStatus}
Review: ${task.reviewStatus}
Progress: ${describeTaskProgress(task.progress)} (${task.progress.usefulObservations} useful observations)
${completionLine}
`;
}

/** Read-only projection observability: sizes, pressure, tool ABI. */
export function formatContextStatus(projection: ProjectionService): string {
  const last = projection.lastProjection();
  if (last === null) {
    return "Context projection: not yet computed (send a prompt first)\n";
  }
  const context = last.contextProjection;
  const stableBytes = context.stableSegments.reduce((sum, segment) => sum + segment.bytes, 0);
  const contextualBytes = context.contextualSegments.reduce(
    (sum, segment) => sum + segment.bytes,
    0,
  );
  const volatileBytes = context.volatileSegments.reduce((sum, segment) => sum + segment.bytes, 0);
  const pressure = last.pressure;
  const tool = last.toolProjection;
  return [
    `Context projection (mode ${last.mode})`,
    `  Stable: ${stableBytes} B (fingerprint ${context.stableFingerprint.slice(0, 8)})`,
    `  Contextual: ${contextualBytes} B`,
    `  Volatile: ${volatileBytes} B`,
    `  Estimated: ${last.estimatedTokens} tokens / ${pressure.workingMaximum} working`,
    `  Pressure: ${pressure.state} (${Math.round(pressure.ratio * 100)}%)`,
    `  Tool ABI: ${tool.fingerprint.slice(0, 8)} (${tool.counts.available} available, ${tool.counts.gated} gated, ${tool.counts.hidden} hidden)`,
    "",
  ].join("\n");
}

/** Compact tool projection summary for /tools. */
export function formatToolProjection(projection: ProjectionService): string {
  const last = projection.lastProjection();
  if (last === null) {
    return "Tool projection: not yet computed\n";
  }
  const tool = last.toolProjection;
  return `Tool projection: ${tool.counts.available} available, ${tool.counts.gated} gated, ${tool.counts.hidden} hidden (ABI ${tool.fingerprint.slice(0, 8)})\n`;
}

/** Read-only /instructions listing (never exposes absolute host paths). */
export function formatInstructions(
  instructions: readonly import("@solaris/core").ProjectInstruction[],
  revision: string | null,
): string {
  if (instructions.length === 0) {
    return "Project instructions: none discovered (no AGENTS.md files inside the workspace)\n";
  }
  const lines = [
    `Project instructions (inventory revision ${revision === null ? "none" : revision.slice(0, 12)}):`,
  ];
  for (const instruction of instructions) {
    const scope = describeInstructionScope(instruction);
    const fileRevision =
      instruction.sourceRevision === null ? "" : ` @ ${instruction.sourceRevision}`;
    const firstLine = instruction.content.split("\n")[0] ?? "";
    lines.push(`- ${scope}${fileRevision}: ${firstLine.slice(0, 60)}`);
  }
  lines.push(
    "",
    "Instructions shape how work is performed; they never grant capabilities or override security policy.",
  );
  return `${lines.join("\n")}\n`;
}

/** Read-only /knowledge listing of current facts (ADR 0017 §36). */
export function formatKnowledge(coordinator: import("@solaris/core").KnowledgeCoordinator): string {
  const facts = coordinator.activeFacts();
  const retired = coordinator.retiredSubjects();
  if (facts.length === 0 && retired.length === 0) {
    return "Project knowledge: none recorded\n";
  }
  const lines = ["Project knowledge:"];
  for (const fact of facts) {
    const subject = fact.subjectKey ?? fact.id;
    const pinned = fact.activation === "pinned" ? ", pinned" : "";
    const expiry =
      fact.expiresAtMs === null ? "" : `, expires ${new Date(fact.expiresAtMs).toISOString()}`;
    lines.push(
      `- ${subject}`,
      `    ${fact.content}`,
      `    revision ${fact.revision}, ${fact.confidence} confidence, ${fact.volatility} volatility${pinned}${expiry}`,
    );
  }
  for (const subject of retired) {
    lines.push(`- ${subject} (retired; revisions retained)`);
  }
  lines.push(
    "",
    "Knowledge is factual context only: it never grants permissions, changes policy, or overrides the task contract.",
  );
  return `${lines.join("\n")}\n`;
}

/** Read-only /knowledge why rendering of the latest retrieval trace. */
export function formatKnowledgeTrace(
  trace: import("@solaris/core").KnowledgeRetrievalTrace | null,
): string {
  if (trace === null) {
    return "Knowledge retrieval: no retrieval has run yet (send a prompt first)\n";
  }
  const query = [
    trace.query.subjectKey === null ? null : `subject=${trace.query.subjectKey}`,
    trace.query.text === null ? null : `text="${trace.query.text.slice(0, 60)}"`,
    trace.query.paths.length === 0 ? null : `paths=${trace.query.paths.join(",")}`,
  ]
    .filter((part): part is string => part !== null)
    .join(" ");
  const lines = [
    `Knowledge retrieval trace (${new Date(trace.atMs).toISOString()})`,
    `  Query: ${query.length === 0 ? "(none)" : query}`,
    `  Considered ${trace.consideredCount} active fact(s); selected ${trace.selected.length}; omitted ${trace.omittedCount}`,
    `  Budget: ${trace.budget.limit} facts / ${trace.budget.maxBytes} bytes (used ${trace.budget.usedBytes})`,
  ];
  for (const selection of trace.selected) {
    lines.push(
      `  - ${selection.subjectKey ?? selection.factId} rev ${selection.revision} (score ${selection.score}, ${selection.confidence}): ${selection.matchReasons.join(", ")}`,
    );
  }
  return `${lines.join("\n")}\n`;
}

/** Render the declared references table (read-only projection). */
export function formatReferences(
  registry: ReferenceRegistry,
  materializer: ReferenceMaterializerPort,
  configError: string | null,
): string {
  const references = registry.list();
  const lines: string[] = [];
  if (configError !== null) {
    lines.push(`References configuration error: ${sanitizeForDisplay(configError)}`);
    lines.push("");
  }
  if (references.length === 0) {
    lines.push(
      configError === null ? "No references are configured." : "No references are available.",
    );
    return `${lines.join("\n")}\n`;
  }
  lines.push(`References (${references.length})`);
  for (const reference of references) {
    lines.push(
      `  ${formatReferenceAlias(reference.alias).padEnd(26)}${reference.kind.padEnd(17)}${materializer.status(reference.id).padEnd(16)}${reference.trust.padEnd(15)}${describeReferenceStatus(reference)}`,
    );
    lines.push(`    source: ${describeReferenceSource(reference.source)}`);
    lines.push(`    revision: ${describeReferenceRevision(registry, reference.id)}`);
    if (reference.description !== null) {
      lines.push(`    description: ${sanitizeForDisplay(reference.description)}`);
    }
  }
  return `${lines.join("\n")}\n`;
}

export function formatReferenceDetail(
  registry: ReferenceRegistry,
  materializer: ReferenceMaterializerPort,
  selector: string,
): string {
  const alias = selector.startsWith("@reference/")
    ? selector.slice("@reference/".length)
    : selector;
  const reference = registry.get(alias as Parameters<ReferenceRegistry["get"]>[0]);
  if (reference === undefined) {
    return `Unknown reference: ${sanitizeForDisplay(selector)}. List configured references with /references.\n`;
  }
  const revision = registry.revision(reference.id);
  const lines = [
    `Reference: ${formatReferenceAlias(reference.alias)}`,
    `Kind: ${reference.kind}`,
    `Description: ${reference.description === null ? "none" : sanitizeForDisplay(reference.description)}`,
    `Source: ${describeReferenceSource(reference.source)}`,
    `Identity: ${describeReferenceIdentity(revision)}`,
    `Materialization: ${materializer.status(reference.id)}`,
    `Trust: ${reference.trust}`,
    `Availability: ${describeReferenceStatus(reference)}`,
    `Resolved revision: ${describeReferenceRevision(registry, reference.id)}`,
  ];
  return `${lines.join("\n")}\n`;
}

export function formatResearchStatus(
  service: ResearchService,
  security: SolarisSecurity,
  sources: readonly ResearchSourcePort[],
): string {
  const decision = security.evaluateCapability("research.fetch");
  const state = decision.decision === "allow" ? "enabled" : "disabled";
  const lines = [`Research: ${state}`];
  if (decision.decision !== "allow") {
    lines.push(`Policy: ${sanitizeForDisplay(decision.reason)}`);
  }
  if (sources.length === 0) {
    lines.push("Sources: none configured");
  } else {
    lines.push(
      `Sources (${sources.length}): ${sources
        .map((source) => `${source.kind} (${sanitizeForDisplay(source.label)})`)
        .join(", ")}`,
    );
  }
  lines.push(`Active requests: ${service.activeRequestCount()}`);
  lines.push(`Recent evidence: ${service.latestEvidence().length}`);
  return `${lines.join("\n")}\n`;
}

function describeReferenceSource(source: ReferenceSource): string {
  if (source.kind === "local-directory") {
    // The path as the user configured it — managed/cache paths are never
    // shown.
    return sanitizeForDisplay(source.path);
  }
  const pin =
    source.ref.kind === "commit"
      ? `commit ${source.ref.commit}`
      : source.ref.kind === "tag"
        ? `tag ${source.ref.tag}`
        : `branch ${source.ref.branch}`;
  return `${sanitizeForDisplay(source.repository)} (${pin})`;
}

function describeReferenceRevision(registry: ReferenceRegistry, id: string): string {
  const revision = registry.revision(id as Parameters<ReferenceRegistry["revision"]>[0]);
  if (revision === null) {
    return "unresolved";
  }
  return revision.identity.kind === "repository"
    ? `commit ${revision.identity.commit}`
    : `fingerprint ${revision.identity.fingerprint}`;
}

function describeReferenceIdentity(revision: ReferenceRevision | null): string {
  if (revision === null) {
    return "unresolved";
  }
  if (revision.identity.kind === "repository") {
    return `${sanitizeForDisplay(revision.identity.origin)} @ commit ${revision.identity.commit}`;
  }
  return `${sanitizeForDisplay(revision.identity.canonicalPath)} (fingerprint ${revision.identity.fingerprint})`;
}

function describeReferenceStatus(reference: Reference): string {
  if (reference.status === "ready") {
    return "ready";
  }
  return `${reference.status}${reference.failureReason === null ? "" : `: ${sanitizeForDisplay(reference.failureReason)}`}`;
}

/** Render a structural read tool result (read-only projection). */
export function formatStructuralRead(result: import("@solaris/core").ToolExecutionResult): string {
  if (result.status !== "success") {
    return "";
  }
  const output = result.output;
  if (typeof output !== "object" || output === null || Array.isArray(output)) {
    return "(no structural data)\n";
  }
  const record = output as Record<string, unknown>;
  const revision = typeof record["revision"] === "string" ? record["revision"] : "none";
  if (record["supported"] === false) {
    return `${record["path"] as string} @ ${revision}: unsupported (${String(record["reason"])})\n`;
  }
  const structure = record["structure"];
  if (typeof structure !== "object" || structure === null) {
    return "(no structure)\n";
  }
  const s = structure as Record<string, unknown>;
  const functions = Array.isArray(s["functions"])
    ? (s["functions"] as Array<Record<string, unknown>>)
    : [];
  const properties = Array.isArray(s["properties"])
    ? (s["properties"] as Array<Record<string, unknown>>)
    : [];
  const signals = Array.isArray(s["signals"])
    ? (s["signals"] as Array<Record<string, unknown>>)
    : [];
  const status = s["status"] === "partial" ? " (partial)" : "";
  const errors = Array.isArray(s["parserErrors"])
    ? (s["parserErrors"] as Array<Record<string, unknown>>)
    : [];
  const lines = [
    `${String(record["path"])} @ ${revision}${status}`,
    `  extends: ${typeof s["extendsType"] === "string" ? s["extendsType"] : "-"}`,
    `  class_name: ${typeof s["className"] === "string" ? s["className"] : "-"}`,
    `  ${signals.length} signals, ${properties.length} properties, ${functions.length} functions`,
    ...(functions.length === 0
      ? []
      : [`  functions: ${functions.map((fn) => String(fn["name"])).join(", ")}`]),
    ...(errors.length === 0
      ? []
      : [
          `  parser errors: ${errors.map((error) => `${String(error["line"])}:${String(error["message"])}`).join(" | ")}`,
        ]),
    ...(s["truncated"] === true ? ["  (declaration cap reached; output truncated)"] : []),
    "",
  ];
  return lines.join("\n");
}

// --- Stage 3 milestone 6: capability doctor + self-reference rendering ---
