import type {
  ConversationItem,
  ModelProvider,
  PlannerOutcome,
  PlannerPort,
  PlannerRequest,
  RegisteredTool,
  TaskPlanContent,
  ToolDefinition,
  ToolExecutionResult,
  ToolProjector,
  ToolRegistry,
} from "@siralos/core";
import { extractPlanCandidateJson, validatePlanCandidate } from "@siralos/core";
import {
  isPreparedCommandTool,
  isPreparedDiagnosticTool,
  isPreparedLSPSessionTool,
  isPreparedMutationTool,
  isPreparedProbeTool,
} from "@siralos/core";
import {
  collectBoundedModelTurn,
  detachBoundedToolResult,
  normalizeBoundedInteger,
  utf8ByteLength,
  type BoundedModelTurnLimits,
} from "../providers/bounded-model-turn.js";

/**
 * Read-only model-based planner executor (Stage 3 milestone 7, ADR 0020).
 *
 * Every planning run uses a FRESH provider context: a new provider
 * instance from the injected factory and a brand-new conversation —
 * planner and executor never share provider-private continuation state,
 * and the executor context later receives only the validated structured
 * plan, never the planner transcript or private reasoning.
 *
 * The planner is strictly read-only, twice enforced: the composition-root
 * registry contains only read-only tools, and `isPlainPlannerTool` refuses
 * every prepared/mutating/process tool class at execution time. The
 * provider-visible tool schema is projected through the host-owned
 * ToolProjector in `planning` mode, which excludes mutation and
 * approval-grant tools even if they were ever registered.
 *
 * Output is a strict JSON contract validated by the core host validation
 * boundary (`validatePlanCandidate`): malformed output is a planning
 * failure retried within a bounded budget (default 2 attempts) — never
 * silently treated as plan prose. Planning has a bounded budget (tool
 * rounds, per-turn calls, output bytes, timeout), and repeated identical
 * no-progress tool calls fail planning cleanly instead of investigating
 * forever; every tool observation feeds the host progress tracker through
 * `onObservation`.
 */

export interface PlannerExecutorOptions {
  /** Must return a FRESH provider instance per attempt (fresh context). */
  readonly providerFactory: () => ModelProvider;
  /** Read-only planner tool registry (composition-root owned). */
  readonly tools: ToolRegistry;
  /**
   * Host-owned tool projection (planning mode). When provided, the
   * provider-visible schema is projected through it; the adapter never
   * makes its own visibility decisions.
   */
  readonly toolProjector?: ToolProjector;
  readonly timeoutMs?: number;
  readonly maxToolRounds?: number;
  /** Bounded planner output attempts (malformed output is retried). */
  readonly maxAttempts?: number;
  /** Host progress feed (wired to the task observer by the caller). */
  readonly onObservation?: (observation: {
    readonly action: string;
    readonly fingerprint: string;
    readonly progress: boolean;
  }) => void;
}

const MAX_ASSISTANT_TEXT_BYTES = 256 * 1024;
const MAX_PROMPT_BYTES = 64 * 1024;
const DEFAULT_TIMEOUT_MS = 120_000;
const DEFAULT_MAX_TOOL_ROUNDS = 6;
const DEFAULT_MAX_ATTEMPTS = 2;
/** Repeated identical (tool, result) calls within this window stall the planner. */
const STALL_REPEAT_THRESHOLD = 3;
const STALL_WINDOW = 5;
const MAX_TOOL_RESULT_BYTES = 2 * 1024 * 1024;
const MAX_ATTEMPT_TOOL_RESULT_BYTES = 8 * 1024 * 1024;

const PLANNER_TURN_LIMITS: BoundedModelTurnLimits = Object.freeze({
  maxTextBytes: MAX_ASSISTANT_TEXT_BYTES,
  maxTextEvents: 4096,
  maxToolCalls: 8,
  maxToolNameBytes: 256,
  maxCallIdBytes: 256,
  maxToolArgumentBytes: 128 * 1024,
  maxTurnBytes: 512 * 1024,
});

/** Deterministic FNV-1a fingerprint over a bounded tool-result view. */
function resultFingerprint(toolName: string, result: ToolExecutionResult): string {
  const view = JSON.stringify({
    status: result.status,
    ...("message" in result ? { message: result.message } : {}),
    ...("output" in result ? { output: result.output } : {}),
  }).slice(0, 2048);
  let hash = 0x811c9dc5;
  for (let index = 0; index < view.length; index += 1) {
    hash ^= view.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return `${toolName}|${hash.toString(16)}`;
}

export function createPlannerExecutor(options: PlannerExecutorOptions): PlannerPort {
  const timeoutMs = normalizeBoundedInteger(options.timeoutMs, DEFAULT_TIMEOUT_MS, 1, 10 * 60_000);
  const maxToolRounds = normalizeBoundedInteger(
    options.maxToolRounds,
    DEFAULT_MAX_TOOL_ROUNDS,
    0,
    32,
  );
  const maxAttempts = normalizeBoundedInteger(options.maxAttempts, DEFAULT_MAX_ATTEMPTS, 1, 3);
  const observe = options.onObservation ?? (() => undefined);

  async function plan(input: PlannerRequest, signal?: AbortSignal): Promise<PlannerOutcome> {
    const controller = new AbortController();
    let timedOut = false;
    const onAbort = (): void => {
      controller.abort();
    };
    signal?.addEventListener("abort", onAbort, { once: true });
    const timer = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, timeoutMs);
    try {
      const registeredTools = options.tools.definitions();
      const projection = options.toolProjector?.project({
        mode: "planning",
        registeredTools,
      });
      const tools =
        projection === undefined
          ? registeredTools.map((info) => info.definition)
          : projection.requestTools;
      // A projected tool may be visible but gated. Planning has no approval
      // protocol, so only tools the host classified as available may execute.
      // Hidden names are refused too, even if a provider fabricates a call.
      const executableToolNames = new Set(
        projection === undefined
          ? tools.map((tool) => tool.name)
          : projection.tools
              .filter((tool) => tool.visibility === "available")
              .map((tool) => tool.name),
      );
      let lastFailure: string | null = null;
      for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
        // Every attempt gets a fresh provider and a fresh conversation:
        // no continuation state is shared with anything else.
        const outcome = await attemptOnce(
          input,
          tools,
          executableToolNames,
          controller.signal,
          maxToolRounds,
          lastFailure,
          () => timedOut,
        );
        if (outcome.kind === "cancelled") {
          return { status: "cancelled" };
        }
        if (outcome.kind === "timed_out") {
          return { status: "timed_out", message: "The planner timed out." };
        }
        if (outcome.kind === "ready") {
          return { status: "ready", content: outcome.content };
        }
        lastFailure = outcome.message;
        observe({
          action: "planning.retry",
          fingerprint: `retry:${attempt}`,
          progress: false,
        });
      }
      return {
        status: "failed",
        message:
          lastFailure === null
            ? "The planner produced no output within its budget."
            : `Planning failed after ${maxAttempts} attempt(s): ${lastFailure}`,
      };
    } finally {
      clearTimeout(timer);
      signal?.removeEventListener("abort", onAbort);
    }
  }

  async function attemptOnce(
    input: PlannerRequest,
    tools: readonly ToolDefinition[],
    executableToolNames: ReadonlySet<string>,
    signal: AbortSignal,
    rounds: number,
    previousFailure: string | null,
    isTimedOut: () => boolean,
  ): Promise<
    | { readonly kind: "ready"; readonly content: TaskPlanContent }
    | { readonly kind: "cancelled" }
    | { readonly kind: "timed_out" }
    | { readonly kind: "failed"; readonly message: string }
  > {
    const prompt = buildPlannerPrompt(input, previousFailure);
    if (utf8ByteLength(prompt) > MAX_PROMPT_BYTES) {
      return {
        kind: "failed",
        message: `The planner prompt exceeded the ${MAX_PROMPT_BYTES}-byte limit; the provider was not invoked.`,
      };
    }
    const provider = options.providerFactory();
    const messages: ConversationItem[] = [{ type: "user_message", content: prompt }];
    const recentResults: string[] = [];
    const seenCallIds = new Set<string>();
    let completedToolRounds = 0;
    let toolResultBytes = 0;
    for (;;) {
      const turn = await collectBoundedModelTurn({
        actor: "The planner",
        provider,
        messages,
        tools,
        signal,
        limits: PLANNER_TURN_LIMITS,
        seenCallIds,
      });
      if (turn.kind === "aborted") {
        return isTimedOut() ? { kind: "timed_out" } : { kind: "cancelled" };
      }
      if (turn.kind === "failed") {
        return { kind: "failed", message: turn.message };
      }
      if (turn.text.length > 0) {
        messages.push({ type: "assistant_message", content: turn.text });
      }
      if (turn.toolCalls.length === 0) {
        const parsed = extractPlanCandidateJson(turn.text);
        const validated =
          parsed === null
            ? null
            : validatePlanCandidate(parsed, { contract: input.contract, depth: input.depth });
        if (validated !== null && validated.ok) {
          return { kind: "ready", content: validated.content };
        }
        return {
          kind: "failed",
          message:
            parsed === null
              ? "The planner did not return a valid JSON plan; malformed output is never treated as plan prose."
              : `The planner returned an invalid plan: ${validated === null ? "invalid candidate" : validated.reasons.join(" ")}`,
        };
      }
      if (completedToolRounds >= rounds) {
        return {
          kind: "failed",
          message: `The planner exceeded the maximum of ${rounds} tool rounds; the additional calls were not executed.`,
        };
      }
      completedToolRounds += 1;
      for (const call of turn.toolCalls) {
        messages.push({
          type: "assistant_tool_call",
          callId: call.callId,
          toolName: call.toolName,
          input: call.input,
        });
        const tool = options.tools.get(call.toolName);
        let result: ToolExecutionResult;
        if (
          tool === undefined ||
          !executableToolNames.has(call.toolName) ||
          !isPlainPlannerTool(tool)
        ) {
          result = {
            status: "failed",
            message:
              tool === undefined
                ? `Unknown tool: ${call.toolName}.`
                : !executableToolNames.has(call.toolName)
                  ? `Tool ${call.toolName} is not available in the host-projected planner tool surface.`
                  : `Tool ${call.toolName} is not a read-only planner tool.`,
          };
        } else {
          try {
            result = await tool.execute(call.input, {
              ...(signal === undefined ? {} : { signal }),
            });
          } catch (error: unknown) {
            result = {
              status: "failed",
              message: error instanceof Error ? error.message : "tool execution failed",
            };
          }
        }
        const detached = detachBoundedToolResult(result, MAX_TOOL_RESULT_BYTES, call.toolName);
        if (!detached.ok) {
          return { kind: "failed", message: detached.message };
        }
        toolResultBytes += detached.byteLength;
        if (toolResultBytes > MAX_ATTEMPT_TOOL_RESULT_BYTES) {
          return {
            kind: "failed",
            message: `The planner exceeded the ${MAX_ATTEMPT_TOOL_RESULT_BYTES}-byte cumulative tool-result limit.`,
          };
        }
        result = detached.result;
        const fingerprint = resultFingerprint(call.toolName, result);
        recentResults.push(fingerprint);
        if (recentResults.length > STALL_WINDOW) {
          recentResults.shift();
        }
        const repeats = recentResults.filter((entry) => entry === fingerprint).length;
        observe({
          action: "planning.tool",
          fingerprint,
          progress: repeats <= 1,
        });
        if (repeats >= STALL_REPEAT_THRESHOLD) {
          return {
            kind: "failed",
            message:
              "The planner stalled: repeated identical reads without progress; planning failed cleanly.",
          };
        }
        messages.push({
          type: "tool_result",
          callId: call.callId,
          toolName: call.toolName,
          result,
        });
      }
    }
  }

  return { plan };
}

function buildPlannerPrompt(input: PlannerRequest, previousFailure: string | null): string {
  const contract = input.contract;
  const criteria = contract.acceptanceCriteria
    .map(
      (criterion) => `- ${criterion.id} (${criterion.verificationKind}): ${criterion.description}`,
    )
    .join("\n");
  const constraints = contract.constraints
    .map((constraint) => `- ${constraint.description}`)
    .join("\n");
  const depthInstructions =
    input.depth === "light"
      ? [
          "This is a LIGHT plan: keep it compact.",
          "Include: objective, at most 6 ordered steps, expected primary",
          "touchpoints (verified only when actually inspected), and primary",
          "validation. Do not add risk essays or exhaustive exploration.",
        ].join("\n")
      : [
          "This is a FULL plan: include objective, scope (inScope/outOfScope),",
          "non-goals, verified and candidate touchpoints, architecture",
          "constraints, risks (severity low/medium/high), acceptance-criteria",
          "linkage on steps, implementation steps, validation strategy, and",
          "rollback/recovery considerations. Keep it bounded — no essays.",
        ].join("\n");
  const prompt = [
    "You are the Siralos planner. You produce a structured plan ONLY; you",
    "never modify files, never request approval, and never claim capability",
    "authority. Plans are descriptive; capability, sandbox, and approval",
    "policy are outside your reach.",
    "",
    "You may inspect the repository with the provided read-only tools",
    "(workspace reads, Godot inspection/API knowledge, references, research",
    "where available, self-reference). A touchpoint is VERIFIED only when",
    "you actually inspected the exact file state; then record its exact",
    "workspace revision handle (rev_ + 32 hex) from the read result.",
    "Otherwise mark it candidate.",
    "",
    depthInstructions,
    "",
    "Task request:",
    contract.request,
    "",
    "Acceptance criteria:",
    criteria,
    "",
    constraints.length === 0 ? "" : `Constraints:\n${constraints}`,
    previousFailure === null
      ? ""
      : `Note: a previous attempt was rejected: ${previousFailure}. Return a corrected plan.`,
    "",
    'Respond with exactly one JSON object and nothing else, matching the host "depth":',
    `{"depth":"${input.depth}","objective":"...","scope":{"inScope":[...],"outOfScope":[...]},"nonGoals":[...],"touchpoints":[{"id":"t1","path":"workspace/relative/path","confidence":"verified|candidate","revision":"rev_... (required when verified)","evidence":"read:path|api:Symbol|reference:alias@rev|knowledge:subject"}],"constraints":[{"id":"c1","description":"..."}],"risks":[{"id":"r1","severity":"low|medium|high","description":"..."}],"steps":[{"id":"step-1","title":"...","expectedTouchpoints":["t1"],"verification":["criterion-id"]}],"validation":{"checks":["..."],"requirements":["..."]},"rollback":{"description":"..."}}`,
    "",
    "Rules:",
    "- Steps reference existing acceptance-criterion ids from the list above.",
    "- Verified touchpoints REQUIRE the exact inspected revision handle.",
    "- Paths are workspace-relative with forward slashes; no absolute paths,",
    "  no '..', no '@reference/' paths.",
    "- Never include secrets or credential-shaped values.",
    "- Never include capability/policy claims (no enabling/disabling sandbox,",
    "  network, approvals, or execution).",
    "- Keep every field bounded; at most 12 steps (6 for light plans).",
  ].join("\n");
  return prompt;
}

/**
 * Defense in depth: the planner may only execute plain tools whose
 * capability is inside the fixed read-only set. Prepared tools
 * (mutations, commands, probes, checks, sessions) are never executable by
 * the planner even if one were registered by mistake. Tools without a
 * declared capability execute (the planner registry contains only
 * read-only tools by construction); a future plain tool whose capability
 * is outside the read-only set is refused here rather than executed
 * outside the permission machinery.
 */
const READ_ONLY_PLANNER_CAPABILITIES = new Set([
  "workspace.read",
  "git.inspect",
  "godot.inspect",
  "godot.api",
  "reference.inspect",
  "research.fetch",
  "self.inspect",
]);

function isPlainPlannerTool(tool: RegisteredTool): tool is {
  readonly definition: ToolDefinition;
  readonly capability?: import("@siralos/core").Capability;
  readonly execute: (
    input: unknown,
    context: import("@siralos/core").ToolExecutionContext,
  ) => Promise<ToolExecutionResult>;
} {
  if (
    isPreparedCommandTool(tool) ||
    isPreparedMutationTool(tool) ||
    isPreparedProbeTool(tool) ||
    isPreparedDiagnosticTool(tool) ||
    isPreparedLSPSessionTool(tool)
  ) {
    return false;
  }
  if (typeof (tool as { execute?: unknown }).execute !== "function") {
    return false;
  }
  return tool.capability === undefined || READ_ONLY_PLANNER_CAPABILITIES.has(tool.capability);
}
