import type {
  ChangeReviewRequest,
  ChangeReviewResult,
  ChangeReviewer,
  ConversationItem,
  ModelProvider,
  RegisteredTool,
  ToolExecutionResult,
  ToolProjector,
  ToolRegistry,
} from "@solaris/core";
import {
  DEFAULT_MAX_TOOL_ROUNDS,
  QUALITY_LIMITS,
  isPreparedCommandTool,
  isPreparedDiagnosticTool,
  isPreparedLSPSessionTool,
  isPreparedMutationTool,
  isPreparedProbeTool,
  normalizeReviewFindings,
} from "@solaris/core";

/**
 * Model-based independent change reviewer (ADR 0013 §26–§27, §51).
 *
 * Every review uses a FRESH provider context: a new provider instance from
 * the injected factory and a brand-new conversation — the primary
 * implementer's conversational history is never reused or forwarded. The
 * reviewer is strictly read-only: the injected registry contains only
 * read-only tools (no mutation, no process execution, no approval, no
 * checkpoints, no undo), and the request carries only the bounded review
 * input (intent, final diff, evidence summary, repository guidance).
 *
 * Output is a strict JSON contract validated at runtime: findings are
 * bounded (50), every field is length-bounded, paths are normalized to
 * safe workspace-relative form, and malformed output rejects the whole
 * review. Reviewer output is untrusted data; it can never register tools,
 * execute actions, or influence approvals.
 */

export interface ProviderChangeReviewerOptions {
  /** Must return a FRESH provider instance per call (fresh context). */
  readonly providerFactory: () => ModelProvider;
  /** Read-only tool registry for the reviewer (composition-root owned). */
  readonly tools: ToolRegistry;
  /**
   * Host-owned tool projection (review mode). When provided, the reviewer
   * request is projected through it — mutation and process tools are
   * hidden by the mode allowlist even if they were ever registered; the
   * adapter never makes its own visibility decisions.
   */
  readonly toolProjector?: ToolProjector;
  readonly timeoutMs?: number;
  readonly maxToolRounds?: number;
  /** Bounded collected assistant text per review. */
  readonly maxOutputBytes?: number;
}

const MAX_TOOL_CALLS_PER_TURN = 8;
const MAX_ASSISTANT_TEXT_BYTES = 256 * 1024;
const MAX_PROMPT_BYTES = 2 * 1024 * 1024;

export function createProviderChangeReviewer(
  options: ProviderChangeReviewerOptions,
): ChangeReviewer {
  const timeoutMs = options.timeoutMs ?? QUALITY_LIMITS.reviewTimeoutMs;
  const maxToolRounds = options.maxToolRounds ?? DEFAULT_MAX_TOOL_ROUNDS;
  const maxOutputBytes = options.maxOutputBytes ?? MAX_ASSISTANT_TEXT_BYTES;

  async function review(
    request: ChangeReviewRequest,
    signal?: AbortSignal,
  ): Promise<ChangeReviewResult> {
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
      const prompt = buildReviewPrompt(request);
      const messages: ConversationItem[] = [{ type: "user_message", content: prompt }];
      const provider = options.providerFactory();
      const tools =
        options.toolProjector === undefined
          ? options.tools.definitions().map((info) => info.definition)
          : options.toolProjector.project({
              mode: "review",
              registeredTools: options.tools.definitions(),
            }).requestTools;
      for (let round = 0; round < maxToolRounds; round += 1) {
        // A stalled provider stream must never block the review: the turn
        // is raced against the abort signal (timeout or caller abort).
        const turn = await raceAbort(
          collectTurn(provider, messages, tools, controller.signal, maxOutputBytes),
          controller.signal,
        );
        if (turn === "aborted") {
          return {
            status: timedOut ? "failed" : "cancelled",
            findings: [],
            message: timedOut ? "The review timed out." : "The review was cancelled.",
          };
        }
        if (turn.kind === "aborted") {
          return {
            status: timedOut ? "failed" : "cancelled",
            findings: [],
            message: timedOut ? "The review timed out." : "The review was cancelled.",
          };
        }
        if (turn.kind === "failed") {
          return { status: "failed", findings: [], message: turn.message };
        }
        if (turn.text.length > 0) {
          messages.push({ type: "assistant_message", content: turn.text });
        }
        if (turn.toolCalls.length === 0) {
          return parseReviewOutput(turn.text);
        }
        for (const call of turn.toolCalls) {
          messages.push({
            type: "assistant_tool_call",
            callId: call.callId,
            toolName: call.toolName,
            input: call.input,
          });
          const tool = options.tools.get(call.toolName);
          let result: ToolExecutionResult;
          if (tool === undefined || !isPlainReviewerTool(tool)) {
            result = {
              status: "failed",
              message:
                tool === undefined
                  ? `Unknown tool: ${call.toolName}.`
                  : `Tool ${call.toolName} is not a read-only reviewer tool.`,
            };
          } else {
            try {
              result = await tool.execute(call.input, {
                ...(controller.signal === undefined ? {} : { signal: controller.signal }),
              });
            } catch (error: unknown) {
              result = {
                status: "failed",
                message: error instanceof Error ? error.message : "tool execution failed",
              };
            }
          }
          messages.push({
            type: "tool_result",
            callId: call.callId,
            toolName: call.toolName,
            result,
          });
        }
      }
      return {
        status: "failed",
        findings: [],
        message: `The review exceeded the maximum of ${maxToolRounds} tool rounds.`,
      };
    } finally {
      clearTimeout(timer);
      signal?.removeEventListener("abort", onAbort);
    }
  }

  function buildReviewPrompt(request: ChangeReviewRequest): string {
    const guidance =
      request.repositoryGuidance === null
        ? "No additional repository guidance was provided."
        : `Repository guidance:\n${request.repositoryGuidance}`;
    const payload = JSON.stringify({
      developmentId: request.developmentId,
      intent: request.request,
      engineVersion: request.engineVersion,
      changedPaths: request.changedPaths,
      files: request.files.map((file) => ({ path: file.path, diff: file.unifiedDiff })),
      metrics: request.metrics,
      evidenceSummary: request.evidenceSummary,
      previousFindingIds: request.previousFindingIds,
      reviewRound: request.reviewRound,
    });
    const prompt = [
      "Review the supplied change against its stated intent and repository constraints.",
      "",
      "Focus on concrete defects, regressions, Godot API misuse, security,",
      "architecture, and missing validation.",
      "",
      "Report only evidence-backed findings.",
      "",
      "Do not modify files.",
      "Do not propose unrelated refactors.",
      "Do not penalize stylistic differences unless they materially conflict",
      "with project conventions.",
      "",
      "You may use the provided read-only tools to inspect the repository;",
      "you cannot modify anything.",
      "",
      guidance,
      "",
      "Respond with exactly one JSON object and nothing else:",
      '{"findings":[{"severity":"critical|high|medium|low","category":"correctness|regression|godot-api|architecture|security|maintainability|testing|documentation|style","title":"short title","path":"workspace-relative path or null","line":123,"evidence":"concrete evidence","impact":"impact","recommendation":"narrow remediation","confidence":"high|medium|low"}]}',
      "",
      "Constraints:",
      "- At most 50 findings.",
      "- title at most 256 characters; evidence, impact, and recommendation each at most 4096 characters.",
      "- path must be workspace-relative with forward slashes; absolute paths are rejected.",
      "- An empty findings array is a valid response.",
      "",
      "Change under review (JSON):",
      payload,
    ].join("\n");
    return prompt.slice(0, MAX_PROMPT_BYTES);
  }

  return { review };
}

type TurnOutcome =
  | {
      readonly kind: "turn";
      readonly text: string;
      readonly toolCalls: readonly {
        readonly callId: string;
        readonly toolName: string;
        readonly input: unknown;
      }[];
    }
  | { readonly kind: "aborted" }
  | { readonly kind: "failed"; readonly message: string };

/**
 * Races the turn collection against the abort signal so a provider that
 * never yields (or ignores the signal) still terminates the review within
 * the timeout. The abandoned generator is never awaited again.
 */
async function raceAbort(
  pending: Promise<TurnOutcome>,
  signal: AbortSignal,
): Promise<TurnOutcome | "aborted"> {
  if (signal.aborted) {
    return "aborted";
  }
  return new Promise<TurnOutcome | "aborted">((resolve) => {
    const onAbort = (): void => {
      resolve("aborted");
    };
    signal.addEventListener("abort", onAbort, { once: true });
    pending.then(
      (outcome) => {
        signal.removeEventListener("abort", onAbort);
        resolve(outcome);
      },
      (error: unknown) => {
        signal.removeEventListener("abort", onAbort);
        resolve({
          kind: "failed",
          message: `The reviewer provider failed: ${error instanceof Error ? error.message : "unknown error"}`,
        });
      },
    );
  });
}

async function collectTurn(
  provider: ModelProvider,
  messages: readonly ConversationItem[],
  tools: readonly import("@solaris/core").ToolDefinition[],
  signal: AbortSignal,
  maxOutputBytes: number,
): Promise<TurnOutcome> {
  const request = {
    messages: [...messages],
    tools,
    signal,
  };
  let text = "";
  let textBytes = 0;
  let completionSeen = false;
  const toolCalls: { callId: string; toolName: string; input: unknown }[] = [];
  const seenCallIds = new Set<string>();
  try {
    for await (const event of provider.stream(request)) {
      if (signal.aborted) {
        return { kind: "aborted" };
      }
      if (completionSeen) {
        return {
          kind: "failed",
          message: "The reviewer stream emitted an event after completion.",
        };
      }
      if (event.type === "completed") {
        completionSeen = true;
        continue;
      }
      if (event.type === "text_delta") {
        textBytes += new TextEncoder().encode(event.text).length;
        if (textBytes > maxOutputBytes) {
          return { kind: "failed", message: "The review output exceeded its byte limit." };
        }
        text += event.text;
        continue;
      }
      if (event.callId.length === 0 || event.toolName.length === 0) {
        return {
          kind: "failed",
          message: "The reviewer emitted a tool call with an empty id or name.",
        };
      }
      if (seenCallIds.has(event.callId)) {
        return {
          kind: "failed",
          message: `The reviewer emitted duplicate tool call id ${event.callId}.`,
        };
      }
      if (toolCalls.length >= MAX_TOOL_CALLS_PER_TURN) {
        return { kind: "failed", message: "The reviewer exceeded the per-turn tool-call limit." };
      }
      seenCallIds.add(event.callId);
      toolCalls.push({ callId: event.callId, toolName: event.toolName, input: event.input });
    }
  } catch (error: unknown) {
    if (signal.aborted) {
      return { kind: "aborted" };
    }
    return {
      kind: "failed",
      message: `The reviewer provider failed: ${error instanceof Error ? error.message : "unknown error"}`,
    };
  }
  if (signal.aborted) {
    return { kind: "aborted" };
  }
  if (!completionSeen) {
    return { kind: "failed", message: "The reviewer stream ended without a completion event." };
  }
  return { kind: "turn", text, toolCalls };
}

/**
 * Defense in depth: the reviewer may only execute plain read-only tools
 * whose capability is in the fixed read-only set. Prepared tools
 * (mutations, commands, probes, checks, sessions) are never executable by
 * the reviewer even if one were registered by mistake, and a future plain
 * tool with side effects would be refused here rather than executed
 * outside the permission machinery. The composition-root-owned registry
 * remains the only source of reviewer tools.
 */
const READ_ONLY_REVIEWER_CAPABILITIES = new Set([
  "workspace.read",
  "git.inspect",
  "godot.inspect",
  "godot.api",
  "godot.lsp",
]);

function isPlainReviewerTool(tool: RegisteredTool): tool is {
  readonly definition: import("@solaris/core").ToolDefinition;
  readonly capability?: import("@solaris/core").Capability;
  readonly execute: (
    input: unknown,
    context: import("@solaris/core").ToolExecutionContext,
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
  return tool.capability === undefined || READ_ONLY_REVIEWER_CAPABILITIES.has(tool.capability);
}

function parseReviewOutput(text: string): ChangeReviewResult {
  const parsedJson = extractJson(text);
  if (parsedJson === null) {
    return {
      status: "failed",
      findings: [],
      message: "The reviewer did not return a valid JSON result.",
    };
  }
  const normalized = normalizeReviewFindings(parsedJson);
  if (!normalized.ok) {
    return { status: "failed", findings: [], message: normalized.message };
  }
  return { status: "completed", findings: normalized.findings, message: null };
}

/** Extracts a JSON object from the assistant text (optional code fence). */
function extractJson(text: string): unknown {
  const trimmed = text.trim();
  const fenced = /^```(?:json)?\s*([\s\S]*?)\s*```$/.exec(trimmed);
  const candidate = fenced === null ? trimmed : (fenced[1] ?? trimmed);
  try {
    return JSON.parse(candidate);
  } catch {
    return null;
  }
}
