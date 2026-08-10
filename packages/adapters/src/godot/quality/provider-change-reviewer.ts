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
import {
  collectBoundedModelTurn,
  detachBoundedToolResult,
  normalizeBoundedInteger,
  utf8ByteLength,
  type BoundedModelTurnLimits,
} from "../../providers/bounded-model-turn.js";

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

const MAX_ASSISTANT_TEXT_BYTES = 256 * 1024;
const MAX_PROMPT_BYTES = 2 * 1024 * 1024;
const MAX_TOOL_RESULT_BYTES = 2 * 1024 * 1024;
const MAX_REVIEW_TOOL_RESULT_BYTES = 8 * 1024 * 1024;

const REVIEWER_TURN_LIMITS: BoundedModelTurnLimits = Object.freeze({
  maxTextBytes: MAX_ASSISTANT_TEXT_BYTES,
  maxTextEvents: 4096,
  maxToolCalls: 8,
  maxToolNameBytes: 256,
  maxCallIdBytes: 256,
  maxToolArgumentBytes: 128 * 1024,
  maxTurnBytes: 512 * 1024,
});

export function createProviderChangeReviewer(
  options: ProviderChangeReviewerOptions,
): ChangeReviewer {
  const timeoutMs = normalizeBoundedInteger(
    options.timeoutMs,
    QUALITY_LIMITS.reviewTimeoutMs,
    1,
    10 * 60_000,
  );
  const maxToolRounds = normalizeBoundedInteger(
    options.maxToolRounds,
    DEFAULT_MAX_TOOL_ROUNDS,
    0,
    32,
  );
  const maxOutputBytes = normalizeBoundedInteger(
    options.maxOutputBytes,
    MAX_ASSISTANT_TEXT_BYTES,
    1,
    MAX_ASSISTANT_TEXT_BYTES,
  );

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
      let prompt: string;
      try {
        prompt = buildReviewPrompt(request);
      } catch (error: unknown) {
        return {
          status: "failed",
          findings: [],
          message: `The review request could not be serialized: ${error instanceof Error ? error.message : "unknown error"}`,
        };
      }
      if (utf8ByteLength(prompt) > MAX_PROMPT_BYTES) {
        return {
          status: "failed",
          findings: [],
          message: `The review prompt exceeded the ${MAX_PROMPT_BYTES}-byte limit; the provider was not invoked.`,
        };
      }
      const messages: ConversationItem[] = [{ type: "user_message", content: prompt }];
      const provider = options.providerFactory();
      const registeredTools = options.tools.definitions();
      const projection = options.toolProjector?.project({
        mode: "review",
        registeredTools,
      });
      const tools =
        projection === undefined
          ? registeredTools.map((info) => info.definition)
          : projection.requestTools;
      const executableToolNames = new Set(
        projection === undefined
          ? tools.map((tool) => tool.name)
          : projection.tools
              .filter((tool) => tool.visibility === "available")
              .map((tool) => tool.name),
      );
      const seenCallIds = new Set<string>();
      let completedToolRounds = 0;
      let toolResultBytes = 0;
      for (;;) {
        // A stalled provider stream must never block the review: the turn
        // is raced against the abort signal (timeout or caller abort).
        const turn = await collectBoundedModelTurn({
          actor: "The reviewer",
          provider,
          messages,
          tools,
          signal: controller.signal,
          limits: { ...REVIEWER_TURN_LIMITS, maxTextBytes: maxOutputBytes },
          seenCallIds,
        });
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
        if (completedToolRounds >= maxToolRounds) {
          return {
            status: "failed",
            findings: [],
            message: `The review exceeded the maximum of ${maxToolRounds} tool rounds; the additional calls were not executed.`,
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
            !isPlainReviewerTool(tool)
          ) {
            result = {
              status: "failed",
              message:
                tool === undefined
                  ? `Unknown tool: ${call.toolName}.`
                  : !executableToolNames.has(call.toolName)
                    ? `Tool ${call.toolName} is not available in the host-projected reviewer tool surface.`
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
          const detached = detachBoundedToolResult(result, MAX_TOOL_RESULT_BYTES, call.toolName);
          if (!detached.ok) {
            return { status: "failed", findings: [], message: detached.message };
          }
          toolResultBytes += detached.byteLength;
          if (toolResultBytes > MAX_REVIEW_TOOL_RESULT_BYTES) {
            return {
              status: "failed",
              findings: [],
              message: `The review exceeded the ${MAX_REVIEW_TOOL_RESULT_BYTES}-byte cumulative tool-result limit.`,
            };
          }
          result = detached.result;
          messages.push({
            type: "tool_result",
            callId: call.callId,
            toolName: call.toolName,
            result,
          });
        }
      }
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
    return prompt;
  }

  return { review };
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
