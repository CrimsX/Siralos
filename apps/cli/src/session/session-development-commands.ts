import { createHash } from "node:crypto";
import { closeSync, fstatSync, openSync, readSync, realpathSync } from "node:fs";
import { isAbsolute, join, relative, sep } from "node:path";
import type { DevelopmentTaskFlow, PlanningDecisionInput, TaskRuntime } from "@solaris/core";
import {
  classifyDevelopmentSurface,
  computeExecutorBriefFingerprint,
  computePlanRevisionDigest,
  containsGodotSceneOrResourceReference,
  containsProtectedConfigReference,
  createAdHocTaskContract,
  createDevelopmentTaskFlow,
  createExecutionInputManifest,
  createGuidanceManifest,
  createPlanningFlow,
  createTaskRuntimeSnapshot,
  planTouchpointStaleness,
  type GuidanceManifest,
} from "@solaris/core";
import {
  formatCancelReport,
  formatChangeReviewResult,
  formatDevelopmentResult,
  formatDevelopmentStartPreview,
  formatGodotProbeTerminal,
  formatNoActiveCommand,
  formatPlan,
  formatProviderFailure,
  formatStructuralRead,
  formatTaskStatus,
} from "../output.js";
import { describeGodotFailure } from "./session-godot-commands.js";
import type { SessionControls, SessionIO, SessionInfo } from "./session-types.js";

let activeDevelopmentTaskFlow: DevelopmentTaskFlow | null = null;

export function hasActiveDevelopmentTaskFlow(): boolean {
  return activeDevelopmentTaskFlow !== null;
}

export async function runDevelopCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
  args: readonly string[],
  executePrompt: (request: string) => Promise<void>,
): Promise<void> {
  const planningFlags = parseDevelopPlanningFlags(args);
  const request = planningFlags.request;
  if (request.length === 0) {
    io.write("Usage: /develop <request>\n");
    io.write("Example: /develop Add a health component to the player script\n");
    io.write("Planning flags: --plan (force full planning before execution), --plan-light\n");
    return;
  }
  const controller = controls.beginPrompt();
  try {
    io.write("Checking the development-workflow capability\u2026\n");
    const support = await sessionInfo.development.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ?? "The GDScript development workflow is unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Preparing the development workflow\u2026\n");
    const prepared = await sessionInfo.development.prepareStart(request, controller.signal);
    if (prepared.status !== "ready") {
      io.write(formatGodotProbeTerminal(prepared.status, prepared.message));
      return;
    }
    io.write(formatDevelopmentStartPreview(prepared.preview));
    const decision = await sessionInfo.reviewer.review(
      {
        id: "development",
        capability: "godot.development",
        toolName: "/develop",
        summary: `GDScript development workflow (${request})`,
        preview: prepared.preview,
        digest: prepared.digest,
      },
      controller.signal,
    );
    if (decision.type !== "approve_once") {
      io.write(
        decision.type === "cancelled"
          ? "  \u2715 development workflow approval cancelled\n"
          : `  \u2715 development workflow denied: ${decision.reason ?? "not approved"}\n`,
      );
      return;
    }
    io.write("  approval approved\n");
    const started = await sessionInfo.development.start(prepared.workflowId, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    if (started.status !== "ready") {
      io.write(formatGodotProbeTerminal(started.status, started.message));
      return;
    }
    io.write(`Development workflow ${started.session.id} started: investigating the request.\n`);
    // Stage 3 milestone 11: host-owned surface routing. The preliminary
    // classification uses request signals only (no touchpoints exist
    // before inspection); the authoritative surface is re-derived from
    // the actual prepared target paths at change-set preparation time.
    const surfaceDecision = classifyDevelopmentSurface({ request, touchpoints: [] });
    io.write(`Surface: ${surfaceDecision.kind} (${surfaceDecision.rationale})\n`);
    activeDevelopmentTaskFlow = createDevelopmentTaskFlow({
      runtime: sessionInfo.tasks,
      sources: {
        ...sessionInfo.taskSources,
        instructionSetRevision: sessionInfo.instructions.revision(),
        knowledgeStateRevision: sessionInfo.projectKnowledge.revision(),
      },
      surface: surfaceDecision.kind,
      // Executor briefing foundation: the immutable task snapshot records
      // the manifest identity and the initial brief fingerprint.
      snapshotExtras: ({ taskId, contract }) => {
        const brief = sessionInfo.briefing.compileForRequest(taskId, contract.request);
        // Content identity (ADR 0028): the exact guidance selected for the
        // task, digest-backed; read bounded, never the whole repository.
        const guidance = buildGuidanceManifest(
          brief?.documentationSources ?? [],
          sessionInfo.workspaceRoot,
        );
        return {
          milestoneManifest: brief?.milestone ?? null,
          executorBriefFingerprint: brief === null ? null : computeExecutorBriefFingerprint(brief),
          ...(guidance === null ? {} : { guidanceManifestDigest: guidance.aggregateDigest }),
        };
      },
    });
    sessionInfo.development.onEvent = (event) => activeDevelopmentTaskFlow?.handleEvent(event);
    const task = activeDevelopmentTaskFlow.start(request, prepared.preview, prepared.digest);
    io.write(taskStatus(sessionInfo, task.taskId));

    const planningInput: PlanningDecisionInput = {
      request,
      explicitPlanRequest: planningFlags.explicitPlan,
      ...(planningFlags.requestedDepth === undefined
        ? {}
        : { requestedDepth: planningFlags.requestedDepth }),
      inspectionOnly: false,
      expectedMutation: true,
      acceptanceCriterionCount: task.acceptance.length,
      protectedConfigInvolved: containsProtectedConfigReference(request),
      spansMultipleSubsystems: false,
      researchRequired: false,
      capabilityUncertainty: false,
      narrowRepair: false,
      knownTouchpoints: 0,
      involvesGodotSceneOrResource: containsGodotSceneOrResourceReference(request),
      surface: surfaceDecision.kind,
    };
    const handle = sessionInfo.tasks.getTask(task.taskId);
    if (handle === null) {
      throw new Error("The development task was not found after start.");
    }
    const planningFlow = createPlanningFlow({ handle, planner: sessionInfo.planner });
    const planningDecision = planningFlow.route(planningInput);
    io.write(`Planning: ${planningDecision.depth} (${planningDecision.reason})\n`);
    if (planningDecision.depth !== "none") {
      const planningResult = await planningFlow.run(controller.signal);
      if (planningResult.status === "planned") {
        io.write(formatPlan(planningResult.plan, handle.snapshot().plan));
        if (planningResult.plan.depth === "full") {
          const planDecision = await sessionInfo.reviewer.review(
            {
              id: `plan-${planningResult.plan.id}-rev${planningResult.plan.revision}`,
              capability: "plan.approve",
              toolName: "/plan",
              summary: `${planningResult.plan.objective} (${planningResult.plan.steps.length} steps)`,
              planId: planningResult.plan.id,
              planRevision: planningResult.plan.revision,
              taskContractRevision: planningResult.plan.taskContractRevision,
              digest: computePlanRevisionDigest(planningResult.plan),
            },
            controller.signal,
          );
          if (planDecision.type === "approve_once") {
            const approved = planningFlow.approve();
            if (approved.status === "ok") {
              io.write("  plan approved (binds to this exact plan revision only)\n");
            } else {
              io.write(`  \u2715 plan approval refused: ${approved.reason}\n`);
              await cancelActiveDevelopment(io, sessionInfo);
              return;
            }
          } else {
            io.write(
              planDecision.type === "cancelled"
                ? "  \u2715 plan approval cancelled\n"
                : `  \u2715 plan denied: ${planDecision.reason ?? "not approved"}\n`,
            );
            await cancelActiveDevelopment(io, sessionInfo);
            return;
          }
        }
        const stale = planTouchpointStaleness(planningResult.plan, (path) =>
          sessionInfo.revisions.currentRevision(path),
        );
        if (stale.length > 0) {
          io.write(`  \u26A0 plan has stale verified touchpoints: ${stale.join(", ")}\n`);
          handle.invalidatePlan(
            `Verified plan touchpoints changed after inspection: ${stale.join(", ")}.`,
          );
        }
      } else {
        io.write(planningFailureMessage(planningResult));
        await cancelActiveDevelopment(io, sessionInfo);
        return;
      }
    }
    const blocked = planningFlow.mutationExecutionBlocked();
    if (blocked !== null) {
      io.write(`  \u2715 ${blocked}\n`);
      await cancelActiveDevelopment(io, sessionInfo);
      return;
    }
    // Content identity (ADR 0028): record the exact execution-input
    // environment of this iteration in the task activity log.
    const inputHandle = sessionInfo.tasks.getTask(task.taskId);
    const currentPlan = inputHandle?.currentPlan() ?? null;
    const contractDigest = inputHandle?.contract().digest.value ?? null;
    if (inputHandle !== null && contractDigest !== null) {
      const brief = sessionInfo.briefing.compileForRequest(task.taskId, request);
      const guidance = buildGuidanceManifest(
        brief?.documentationSources ?? [],
        sessionInfo.workspaceRoot,
      );
      const manifest = createExecutionInputManifest({
        taskId: task.taskId,
        iteration: 1,
        inputs: [
          { id: "taskContract", revision: inputHandle.contract().revision, digest: contractDigest },
          ...(currentPlan === null
            ? [{ id: "taskPlan", revision: null, digest: null }]
            : [
                {
                  id: "taskPlan",
                  revision: currentPlan.revision,
                  digest: currentPlan.digest.value,
                },
              ]),
          {
            id: "executionContract",
            revision: sessionInfo.taskSources.executionContract?.revision ?? null,
            digest: null,
          },
          ...(brief?.milestone === null || brief?.milestone === undefined
            ? []
            : [
                {
                  id: "milestone",
                  revision: brief.milestone.version,
                  digest: null,
                },
              ]),
          ...(guidance === null
            ? []
            : [{ id: "guidance", revision: null, digest: guidance.aggregateDigest }]),
          { id: "capability", revision: null, digest: null },
        ],
      });
      inputHandle.recordExecutionInputManifest(manifest.digest);
    }
  } catch (error: unknown) {
    io.write(
      controller.signal.aborted
        ? "  \u2715 development workflow cancelled\n"
        : formatProviderFailure(describeGodotFailure(error)),
    );
    return;
  } finally {
    controls.endPrompt();
  }

  await executePrompt(request);
  const status = sessionInfo.development.status();
  if (status.session !== null && status.session.state.kind === "terminal") {
    const result = await sessionInfo.development.cancel();
    if (result.status === "cancelled" && result.result !== null) {
      io.write(formatDevelopmentResult(result.result));
    }
    if (activeDevelopmentTaskFlow !== null) {
      const finalTask = activeDevelopmentTaskFlow.finish(
        status,
        result.status === "cancelled" ? result.result : null,
      );
      if (finalTask !== null) {
        io.write(taskStatus(sessionInfo, finalTask.taskId));
      }
      clearActiveDevelopment(sessionInfo);
    }
  }
}

export async function runPlanCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
  args: readonly string[],
): Promise<void> {
  const request = args.join(" ").trim();
  if (request.length === 0) {
    io.write("Usage: /plan <request>\n");
    io.write("Example: /plan Add health regeneration after 5 seconds without damage\n");
    io.write("Plan-only mode: returns a structured plan and stops; no source is modified.\n");
    return;
  }
  const controller = controls.beginPrompt();
  try {
    const taskId = nextAdHocTaskId(sessionInfo.tasks);
    const brief = sessionInfo.briefing.compileForRequest(taskId, request);
    const handle = sessionInfo.tasks.createTask({
      contract: createAdHocTaskContract(taskId, request),
      snapshot: createTaskRuntimeSnapshot({
        ...sessionInfo.taskSources,
        milestoneManifest: brief?.milestone ?? null,
        executorBriefFingerprint: brief === null ? null : computeExecutorBriefFingerprint(brief),
      }),
      steps: [],
    });
    handle.transitionPhase("working");
    const planningFlow = createPlanningFlow({ handle, planner: sessionInfo.planner });
    const decision = planningFlow.route({
      request,
      explicitPlanRequest: true,
      inspectionOnly: false,
      expectedMutation: true,
      acceptanceCriterionCount: handle.contract().acceptanceCriteria.length,
      protectedConfigInvolved: containsProtectedConfigReference(request),
      spansMultipleSubsystems: false,
      researchRequired: false,
      capabilityUncertainty: false,
      narrowRepair: false,
      knownTouchpoints: 0,
      involvesGodotSceneOrResource: containsGodotSceneOrResourceReference(request),
    });
    io.write(`Planning: ${decision.depth} (${decision.reason})\n`);
    const result = await planningFlow.run(controller.signal);
    if (result.status === "planned") {
      io.write(formatPlan(result.plan, handle.snapshot().plan));
      io.write(
        "Plan-only mode: no source was modified, no mutation approval was requested,\n" +
          "and no execution follows. Use /develop <request> to execute (edits still\n" +
          "require their own exact one-time approval).\n",
      );
      handle.markBlocked(
        "plan-only mode — execution not started; re-run /develop to execute the plan",
      );
    } else {
      io.write(planningFailureMessage(result));
      handle.markBlocked(planOnlyBlockedReason(result.status));
    }
    io.write(formatTaskStatus(handle.snapshot(), handle.evaluateCompletion()));
  } catch (error: unknown) {
    io.write(
      controller.signal.aborted
        ? "  \u2715 planning cancelled\n"
        : formatProviderFailure(describeGodotFailure(error)),
    );
  } finally {
    controls.endPrompt();
  }
}

export async function runReviewChangeCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
): Promise<void> {
  const controller = controls.beginPrompt();
  try {
    io.write("Running a fresh independent review of the current development change\u2026\n");
    io.write(
      formatChangeReviewResult(
        await sessionInfo.development.runIndependentReview(controller.signal),
      ),
    );
  } catch (error: unknown) {
    io.write(
      controller.signal.aborted
        ? "  \u2715 review cancelled\n"
        : formatProviderFailure(describeGodotFailure(error)),
    );
  } finally {
    controls.endPrompt();
  }
}

export async function runCancelCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
): Promise<void> {
  const status = sessionInfo.development.status();
  if (status.session === null || status.session.state.kind === "terminal") {
    io.write(controls.cancelActivePrompt() ? formatCancelReport() : formatNoActiveCommand());
    return;
  }
  if (controls.cancelActivePrompt()) {
    io.write(formatCancelReport());
  }
  try {
    const outcome = await sessionInfo.development.cancel();
    if (outcome.status === "cancelled" && outcome.result !== null) {
      io.write(formatDevelopmentResult(outcome.result));
    } else {
      io.write("  \u2715 no development workflow was active.\n");
    }
    if (activeDevelopmentTaskFlow !== null) {
      const finalTask = activeDevelopmentTaskFlow.finish(
        sessionInfo.development.status(),
        outcome.status === "cancelled" ? outcome.result : null,
      );
      if (finalTask !== null) {
        io.write(taskStatus(sessionInfo, finalTask.taskId));
      }
      clearActiveDevelopment(sessionInfo);
    }
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

export function runTaskCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  args: readonly string[],
): void {
  const request = args.join(" ").trim();
  if (request.length === 0) {
    io.write("Usage: /task <request>\n");
    io.write(
      "Starts a host-owned ad-hoc task. Completion requires host verification of the\n" +
        "explicit acceptance criterion; use /develop for workflow-integrated tasks.\n",
    );
    io.write(runTaskStatusCommand(sessionInfo));
    return;
  }
  const taskId = nextAdHocTaskId(sessionInfo.tasks);
  const brief = sessionInfo.briefing.compileForRequest(taskId, request);
  const handle = sessionInfo.tasks.createTask({
    contract: createAdHocTaskContract(taskId, request),
    snapshot: createTaskRuntimeSnapshot({
      ...sessionInfo.taskSources,
      milestoneManifest: brief?.milestone ?? null,
      executorBriefFingerprint: brief === null ? null : computeExecutorBriefFingerprint(brief),
    }),
    steps: [],
  });
  handle.transitionPhase("working");
  io.write(formatTaskStatus(handle.snapshot(), handle.evaluateCompletion()));
}

export async function runReadStructureCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  args: readonly string[],
): Promise<void> {
  const path = args.join(" ").trim();
  if (path.length === 0) {
    io.write("Usage: /read-structure <path>\n");
    return;
  }
  const result = await sessionInfo.workspaceRead.execute({ path, mode: "structural" }, {});
  io.write(
    result.status === "success"
      ? formatStructuralRead(result)
      : formatProviderFailure(result.message),
  );
}

export function runTaskStatusCommand(sessionInfo: SessionInfo): string {
  const handle = sessionInfo.tasks.latestTask();
  return handle === null
    ? "No task is tracked yet. Start one with /task <request> or /develop <request>.\n"
    : formatTaskStatus(handle.snapshot(), handle.evaluateCompletion());
}

interface DevelopPlanningFlags {
  readonly request: string;
  readonly explicitPlan: boolean;
  readonly requestedDepth: "light" | "full" | undefined;
}

function parseDevelopPlanningFlags(args: readonly string[]): DevelopPlanningFlags {
  let explicitPlan = false;
  let requestedDepth: "light" | "full" | undefined;
  const rest: string[] = [];
  for (const arg of args) {
    if (arg === "--plan" && !explicitPlan) {
      explicitPlan = true;
      requestedDepth = "full";
    } else if (arg === "--plan-light" && !explicitPlan) {
      explicitPlan = true;
      requestedDepth = "light";
    } else {
      rest.push(arg);
    }
  }
  return { request: rest.join(" ").trim(), explicitPlan, requestedDepth };
}

async function cancelActiveDevelopment(io: SessionIO, sessionInfo: SessionInfo): Promise<void> {
  const status = sessionInfo.development.status();
  if (status.session === null || status.session.state.kind === "terminal") {
    return;
  }
  const result = await sessionInfo.development.cancel();
  if (result.status === "cancelled" && result.result !== null) {
    io.write(formatDevelopmentResult(result.result));
  }
  if (activeDevelopmentTaskFlow !== null) {
    const finalTask = activeDevelopmentTaskFlow.finish(
      status,
      result.status === "cancelled" ? result.result : null,
    );
    if (finalTask !== null) {
      io.write(taskStatus(sessionInfo, finalTask.taskId));
    }
    clearActiveDevelopment(sessionInfo);
  }
}

function clearActiveDevelopment(sessionInfo: SessionInfo): void {
  activeDevelopmentTaskFlow = null;
  sessionInfo.development.onEvent = undefined;
}

function nextAdHocTaskId(runtime: TaskRuntime): string {
  let sequence = runtime.listTasks().length + 1;
  while (runtime.getTask(`task-${sequence}`) !== null) {
    sequence += 1;
  }
  return `task-${sequence}`;
}

function taskStatus(sessionInfo: SessionInfo, taskId: string): string {
  const handle = sessionInfo.tasks.getTask(taskId);
  return handle === null
    ? "The task is no longer available.\n"
    : formatTaskStatus(handle.snapshot(), handle.evaluateCompletion());
}

function planningFailureMessage(result: {
  readonly status: string;
  readonly message?: string;
}): string {
  switch (result.status) {
    case "cancelled":
      return "  \u2715 planning cancelled\n";
    case "timed_out":
      return `  \u2715 planning timed out: ${result.message ?? "timeout"}\n`;
    case "routed":
      return "  \u2715 planning was routed but produced no plan\n";
    default:
      return `  \u2715 planning failed: ${result.message ?? result.status}\n`;
  }
}

function planOnlyBlockedReason(status: string): string {
  switch (status) {
    case "cancelled":
      return "plan-only mode — planning cancelled";
    case "timed_out":
      return "plan-only mode — planning timed out";
    case "routed":
      return "plan-only mode — planning produced no plan";
    default:
      return "plan-only mode — planning failed";
  }
}

/**
 * Digest-backed guidance manifest (ADR 0028): the exact documentation
 * selected for the task, each with its content digest. Reads are bounded
 * per document (stat before read, 512 KiB cap) and containment-checked
 * (realpath must stay under the workspace root; symlinks cannot pull
 * external files into the manifest). A document that cannot be read is
 * omitted (the manifest covers only exactly-representable guidance).
 */
function buildGuidanceManifest(
  documentationSources: readonly string[],
  workspaceRoot: string,
): GuidanceManifest | null {
  if (documentationSources.length === 0) {
    return null;
  }
  const entries: {
    id: string;
    kind: "root-agents" | "nested-agents" | "architecture" | "adr" | "development";
    path: string;
    digest: string;
  }[] = [];
  let workspaceReal: string | null = null;
  try {
    workspaceReal = realpathSync(workspaceRoot);
  } catch {
    return null;
  }
  const maxGuidanceBytes = 512 * 1024;
  for (const source of documentationSources.slice(0, 16)) {
    const relativePath = source.replace(/^\.\//, "");
    if (
      relativePath.includes("..") ||
      relativePath.startsWith("/") ||
      /^[A-Za-z]:/.test(relativePath)
    ) {
      continue;
    }
    try {
      const absolute = join(workspaceRoot, ...relativePath.split("/"));
      // Containment: the real path must stay under the workspace root
      // (portable: path.relative rejects .. and absolute results on both
      // POSIX and Windows separator conventions).
      const real = realpathSync(absolute);
      const containment = relative(workspaceReal, real);
      if (containment === ".." || containment.startsWith(`..${sep}`) || isAbsolute(containment)) {
        continue;
      }
      // Bound the read at the FILE HANDLE level: fstat on the opened fd
      // sizes the exact file that will be read, so a same-user swap
      // between stat and read cannot pull unbounded bytes into memory.
      let fd: number | null = null;
      try {
        fd = openSync(real, "r");
        const handleStats = fstatSync(fd);
        if (!handleStats.isFile() || handleStats.size > maxGuidanceBytes) {
          continue;
        }
        // Read exactly the fstat-sized bytes into a bounded buffer: a
        // same-user append between fstat and read cannot grow the read
        // beyond the cap.
        const buffer = Buffer.alloc(handleStats.size);
        let readTotal = 0;
        while (readTotal < handleStats.size) {
          const readNow = readSync(fd, buffer, readTotal, handleStats.size - readTotal, readTotal);
          if (readNow <= 0) {
            break;
          }
          readTotal += readNow;
        }
        const bytes = buffer.subarray(0, readTotal);
        const kind = relativePath.endsWith("AGENTS.md")
          ? relativePath === "AGENTS.md"
            ? "root-agents"
            : "nested-agents"
          : relativePath.startsWith("docs/adr/")
            ? "adr"
            : relativePath.startsWith("docs/architecture/")
              ? "architecture"
              : "development";
        entries.push({
          id: kind === "adr" ? `adr:${relativePath.slice(9, 13)}` : `doc:${relativePath}`,
          kind,
          path: relativePath,
          digest: createHash("sha256").update(bytes).digest("hex"),
        });
      } finally {
        if (fd !== null) {
          closeSync(fd);
        }
      }
    } catch {
      // Unreadable guidance is omitted; the manifest stays exact.
    }
  }
  return entries.length === 0 ? null : createGuidanceManifest(entries);
}
