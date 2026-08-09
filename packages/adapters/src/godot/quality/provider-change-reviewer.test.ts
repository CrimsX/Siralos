import { describe, expect, it } from "vitest";
import {
  createToolRegistry,
  type ModelEvent,
  type ModelProvider,
  type ModelRequest,
  type Tool,
} from "@solaris/core";
import { QUALITY_LIMITS } from "@solaris/core";
import { createProviderChangeReviewer } from "./provider-change-reviewer.js";
import type { ChangeReviewRequest } from "@solaris/core";

/**
 * Reviewer-isolation tests (ADR 0013 §35–§45, §54, §88–§98). The reviewer
 * must use a fresh provider context, never the primary implementer's
 * history; must receive only the bounded review input; must be strictly
 * read-only; and its output is untrusted data validated at runtime.
 */

function firstUserMessageContent(request: ModelRequest): string {
  for (const item of request.messages) {
    if (item.type === "user_message") {
      return item.content;
    }
  }
  return "";
}

function reviewRequest(overrides: Partial<ChangeReviewRequest> = {}): ChangeReviewRequest {
  return {
    developmentId: "dev-1",
    request: "Add a heal(amount) method that clamps health to max_health.",
    engineVersion: "4.7.1-stable",
    changedPaths: ["scripts/player/player.gd"],
    files: [
      {
        path: "scripts/player/player.gd",
        unifiedDiff:
          "@@ -10,3 +10,5 @@\n func _ready():\n+\theal(10)\n+\n func heal(amount):\n+\thealth = clamp(health + amount, 0, max_health)",
      },
    ],
    metrics: {
      filesChanged: 1,
      linesAdded: 4,
      linesRemoved: 0,
      filesCreated: 0,
      filesDeleted: 0,
      functionsTouched: 1,
    },
    evidenceSummary: [
      { kind: "parser", summary: "1/1 changed scripts parsed" },
      { kind: "lsp", summary: "0 diagnostics" },
      { kind: "scope", summary: "workspace integrity verified" },
    ],
    repositoryGuidance: null,
    previousFindingIds: [],
    reviewRound: 1,
    ...overrides,
  };
}

interface ScriptedTurn {
  readonly events: readonly ModelEvent[];
}

function scriptedProvider(options: {
  readonly turns: readonly ScriptedTurn[];
  readonly observe?: (request: ModelRequest) => void;
}): ModelProvider {
  let instanceCounter = 0;
  const stream = async function* (request: ModelRequest): AsyncIterable<ModelEvent> {
    instanceCounter += 1;
    await Promise.resolve();
    options.observe?.(request);
    const turnIndex = Math.min(instanceCounter, options.turns.length) - 1;
    const turn = options.turns[Math.max(0, turnIndex)] as ScriptedTurn;
    for (const event of turn.events) {
      if (request.signal?.aborted) {
        throw new DOMException("aborted", "AbortError");
      }
      yield event;
      await Promise.resolve();
    }
  };
  return { id: "scripted-reviewer", stream };
}

function readOnlyTool(name: string): Tool {
  return {
    definition: { name, description: `read-only ${name}`, inputSchema: {} },
    capability: "workspace.read",
    execute: () => Promise.resolve({ status: "success", output: {}, summary: name }),
  };
}

function writeTool(name: string): Tool {
  return {
    definition: { name, description: `write ${name}`, inputSchema: {} },
    capability: "workspace.write",
    execute: () => Promise.resolve({ status: "success", output: {}, summary: name }),
  };
}

function jsonTurn(text: string): ScriptedTurn {
  return { events: [{ type: "text_delta", text }, { type: "completed" }] };
}

describe("provider change reviewer isolation", () => {
  it("uses a fresh provider context per review (new provider instance, brand-new conversation)", async () => {
    const observed: ModelRequest[] = [];
    const providerFactory = (): ModelProvider =>
      scriptedProvider({
        turns: [jsonTurn('{"findings":[]}')],
        observe: (request) => observed.push(request),
      });
    const reviewer = createProviderChangeReviewer({
      providerFactory,
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    const first = await reviewer.review(reviewRequest());
    const second = await reviewer.review(reviewRequest());
    expect(first.status).toBe("completed");
    expect(second.status).toBe("completed");
    expect(observed).toHaveLength(2);
    // Every request starts with exactly one user message: no primary
    // implementer history, no hidden reasoning, no provider continuation.
    for (const request of observed) {
      expect(request.messages).toHaveLength(1);
      expect(request.messages[0]?.type).toBe("user_message");
      const content = firstUserMessageContent(request);
      expect(content).toContain("Add a heal(amount) method");
      expect(content).toContain("heal(10)");
      expect(content).not.toContain("chain-of-thought");
    }
  });

  it("delivers the final diff, the original request, and the validation evidence", async () => {
    const observed: ModelRequest[] = [];
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [jsonTurn('{"findings":[]}')],
          observe: (request) => observed.push(request),
        }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    await reviewer.review(reviewRequest());
    const content = firstUserMessageContent(observed[0] as ModelRequest);
    expect(content).toContain(
      '"intent":"Add a heal(amount) method that clamps health to max_health."',
    );
    expect(content).toContain('"engineVersion":"4.7.1-stable"');
    expect(content).toContain("workspace integrity verified");
    expect(content).toContain("scripts/player/player.gd");
  });

  it("never includes primary hidden reasoning or credentials in the request", async () => {
    const observed: ModelRequest[] = [];
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [jsonTurn('{"findings":[]}')],
          observe: (request) => observed.push(request),
        }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    await reviewer.review(reviewRequest());
    const content = firstUserMessageContent(observed[0] as ModelRequest);
    expect(content).not.toContain("chain-of-thought");
    expect(content).not.toContain("apiKey");
    expect(content).not.toContain("authorization");
    expect(content).not.toContain("~/.solaris");
    expect(content).not.toContain("C:\\");
    expect(content).not.toContain("approval");
  });

  it("runs reviewer tool calls against the read-only registry only", async () => {
    const observed: ModelRequest[] = [];
    const registry = createToolRegistry([readOnlyTool("workspace.read")]);
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [
            {
              events: [
                {
                  type: "tool_call",
                  callId: "call-1",
                  toolName: "workspace.read",
                  input: { path: "scripts/player/player.gd" },
                },
                { type: "completed" },
              ],
            },
            jsonTurn('{"findings":[]}'),
          ],
          observe: (request) => observed.push(request),
        }),
      tools: registry,
      timeoutMs: 1000,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("completed");
    // The tool result was fed back to the fresh context.
    expect(observed).toHaveLength(2);
    const secondMessages = observed[1]?.messages ?? [];
    expect(
      secondMessages.some(
        (item) => item.type === "tool_result" && item.toolName === "workspace.read",
      ),
    ).toBe(true);
  });

  it("refuses to execute prepared/write tools even if one is registered", async () => {
    const registry = createToolRegistry([writeTool("workspace.create_file")]);
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [
            {
              events: [
                {
                  type: "tool_call",
                  callId: "call-1",
                  toolName: "workspace.create_file",
                  input: { path: "evil.gd", content: "x" },
                },
                { type: "completed" },
              ],
            },
            jsonTurn('{"findings":[]}'),
          ],
        }),
      tools: registry,
      timeoutMs: 1000,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("completed");
  });

  it("refuses a plain tool whose capability is outside the read-only set", async () => {
    const sideEffectTool: Tool = {
      definition: { name: "workspace.side_effect", description: "would write", inputSchema: {} },
      capability: "workspace.write",
      execute: () => Promise.resolve({ status: "success", output: {}, summary: "ran" }),
    };
    const observed: ModelRequest[] = [];
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [
            {
              events: [
                {
                  type: "tool_call",
                  callId: "call-1",
                  toolName: "workspace.side_effect",
                  input: {},
                },
                { type: "completed" },
              ],
            },
            jsonTurn('{"findings":[]}'),
          ],
          observe: (request) => observed.push(request),
        }),
      tools: createToolRegistry([sideEffectTool]),
      timeoutMs: 1000,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("completed");
    // The tool result fed back must be a failure: the tool never ran.
    const secondMessages = observed[1]?.messages ?? [];
    const toolResult = secondMessages.find(
      (item) => item.type === "tool_result" && item.toolName === "workspace.side_effect",
    );
    expect(toolResult?.type).toBe("tool_result");
    if (toolResult?.type === "tool_result" && toolResult.result.status !== "success") {
      expect(toolResult.result.status).toBe("failed");
      expect(toolResult.result.message).toContain("read-only reviewer tool");
    } else {
      throw new Error("expected a failed tool result for the refused tool");
    }
  });

  it("treats unknown tools as failed tool results, never as actions", async () => {
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [
            {
              events: [
                {
                  type: "tool_call",
                  callId: "call-1",
                  toolName: "process.run",
                  input: { runner: "npm-script", script: "check" },
                },
                { type: "completed" },
              ],
            },
            jsonTurn('{"findings":[]}'),
          ],
        }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("completed");
  });

  it("rejects malformed output as a failed review (never partial findings)", async () => {
    const reviewer = createProviderChangeReviewer({
      providerFactory: () => scriptedProvider({ turns: [jsonTurn("not json at all")] }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("failed");
    expect(result.findings).toHaveLength(0);
  });

  it("rejects findings exceeding the immutable bound", async () => {
    const findings = Array.from({ length: QUALITY_LIMITS.maxReviewFindings + 1 }, (_, index) => ({
      severity: "low",
      category: "style",
      title: `f${index}`,
      evidence: "e",
      impact: "i",
      recommendation: "r",
      confidence: "high",
    }));
    const reviewer = createProviderChangeReviewer({
      providerFactory: () => scriptedProvider({ turns: [jsonTurn(JSON.stringify({ findings }))] }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("failed");
  });

  it("accepts a fenced JSON payload", async () => {
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [jsonTurn('```json\n{"findings":[]}\n```')],
        }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("completed");
    expect(result.findings).toHaveLength(0);
  });

  it("normalizes findings into deterministic structured output", async () => {
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [
            jsonTurn(
              JSON.stringify({
                findings: [
                  {
                    severity: "high",
                    category: "correctness",
                    title: "health can exceed max_health",
                    path: "scripts/player/player.gd",
                    line: 12,
                    evidence: "heal() adds without clamping",
                    impact: "health can exceed the maximum",
                    recommendation: "clamp the result",
                    confidence: "high",
                  },
                ],
              }),
            ),
          ],
        }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("completed");
    expect(result.findings).toHaveLength(1);
    expect(result.findings[0]?.id).toMatch(/^[0-9a-f]{24}$/);
    expect(result.findings[0]?.path).toBe("scripts/player/player.gd");
  });

  it("times out when the provider never completes", async () => {
    const reviewer = createProviderChangeReviewer({
      providerFactory: () => ({
        id: "never-completing",
        stream: async function* () {
          await new Promise<never>(() => undefined);
          yield { type: "completed" };
        },
      }),
      tools: createToolRegistry([]),
      timeoutMs: 20,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("failed");
    expect(result.message).toContain("timed out");
  });

  it("cancels cleanly when the signal aborts", async () => {
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [{ events: [{ type: "text_delta", text: "slow" }] }],
        }),
      tools: createToolRegistry([]),
      timeoutMs: 10_000,
    });
    const controller = new AbortController();
    const promise = reviewer.review(reviewRequest(), controller.signal);
    controller.abort();
    const result = await promise;
    expect(result.status).toBe("cancelled");
  });

  it("fails when the provider stream throws", async () => {
    const reviewer = createProviderChangeReviewer({
      providerFactory: () => ({
        id: "throwing",
        stream: async function* () {
          await Promise.resolve();
          yield { type: "text_delta", text: "" };
          throw new Error("provider exploded");
        },
      }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("failed");
    expect(result.message).toContain("provider exploded");
  });

  it("bounds the reviewer output and tool-round budget", async () => {
    const turnWithCalls: readonly ModelEvent[] = Array.from({ length: 8 }, (_, index) => ({
      type: "tool_call" as const,
      callId: `call-${index}`,
      toolName: "workspace.read",
      input: {},
    }));
    const turns: readonly ScriptedTurn[] = Array.from({ length: 10 }, () => ({
      events: [...turnWithCalls, { type: "completed" as const }],
    }));
    const reviewer = createProviderChangeReviewer({
      providerFactory: () => scriptedProvider({ turns }),
      tools: createToolRegistry([readOnlyTool("workspace.read")]),
      timeoutMs: 5000,
      maxToolRounds: 4,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("failed");
    expect(result.message).toContain("tool rounds");
  });

  it("keeps reviewer provider credentials isolated from the request", async () => {
    let captured: ModelRequest | null = null;
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [jsonTurn('{"findings":[]}')],
          observe: (request) => {
            captured = request;
          },
        }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    await reviewer.review(reviewRequest());
    const payload = JSON.stringify(captured);
    expect(payload).not.toContain("secret");
    expect(payload).not.toContain("sk-");
    expect(payload).not.toContain("credential");
  });

  it("never lets reviewer output register tools or execute actions", async () => {
    const turns: ModelRequest[] = [];
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [
            jsonTurn(
              JSON.stringify({
                findings: [],
                tools: [{ name: "process.run" }],
                execute: ["rm -rf /"],
              }),
            ),
          ],
          observe: (request) => turns.push(request),
        }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    const result = await reviewer.review(reviewRequest());
    expect(result.status).toBe("completed");
    // The extra fields were ignored; the next request (if any) still has
    // only the injected read-only tools.
    expect(turns).toHaveLength(1);
    expect(turns[0]?.tools.map((tool) => tool.name)).not.toContain("process.run");
  });
});

describe("provider change reviewer payload shape", () => {
  it("sends the review instruction set without repeating the whole architecture", async () => {
    const observed: ModelRequest[] = [];
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [jsonTurn('{"findings":[]}')],
          observe: (request) => observed.push(request),
        }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    void reviewer.review(reviewRequest()).then(() => undefined);
    await new Promise<void>((resolve) => setImmediate(resolve));
    const content = firstUserMessageContent(observed[0] as ModelRequest);
    expect(content).toContain("Do not modify files.");
    expect(content).toContain("Do not propose unrelated refactors.");
    expect(content).toContain("evidence-backed findings");
    expect(content).toContain('"findings"');
    expect(content.length).toBeLessThan(64 * 1024);
  });

  it("injects repository guidance when provided", async () => {
    const observed: ModelRequest[] = [];
    const reviewer = createProviderChangeReviewer({
      providerFactory: () =>
        scriptedProvider({
          turns: [jsonTurn('{"findings":[]}')],
          observe: (request) => observed.push(request),
        }),
      tools: createToolRegistry([]),
      timeoutMs: 1000,
    });
    void reviewer
      .review(reviewRequest({ repositoryGuidance: "The project prefers explicit types." }))
      .then(() => undefined);
    await new Promise<void>((resolve) => setImmediate(resolve));
    const content = firstUserMessageContent(observed[0] as ModelRequest);
    expect(content).toContain("The project prefers explicit types.");
  });
});
