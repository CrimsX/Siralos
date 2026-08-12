import { describe, expect, it } from "vitest";
import { mkdtemp, mkdir, rm, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createPlannerToolRegistry } from "@siralos/adapters";
import {
  createAdHocTaskContract,
  createTaskContract,
  createToolRegistry,
  type GodotInspector,
  type GodotKnowledge,
  type ModelProvider,
  type ModelRequest,
  type PlannerPort,
  type Tool,
  type ToolProjector,
} from "@siralos/core";
import type { TaskContract } from "@siralos/core";
import { createPlannerExecutor } from "./planner-executor.js";

/**
 * Planner executor tests (Stage 3 milestone 7, ADR 0020). The planner is
 * structurally read-only, receives a fresh provider context per attempt,
 * has a bounded budget, and its output is validated before it becomes a
 * plan: malformed output is retried within the budget and then FAILS —
 * never silently treated as plan prose.
 */

type ScriptStep =
  | { readonly kind: "tool-call"; readonly toolName: string; readonly input: unknown }
  | { readonly kind: "text"; readonly text: string };

function createScriptedProvider(
  steps: readonly ScriptStep[],
  onRequest?: (request: ModelRequest) => void,
): ModelProvider {
  let cursor = 0;
  return {
    id: "scripted-planner",
    toolCalling: true,
    stream(request: ModelRequest): AsyncIterable<import("@siralos/core").ModelEvent> {
      onRequest?.(request);
      return streamStep(steps, cursor++);
    },
  };
}

async function* streamStep(
  steps: readonly ScriptStep[],
  index: number,
): AsyncIterable<import("@siralos/core").ModelEvent> {
  const step = steps[index];
  if (step === undefined) {
    yield { type: "text_delta", text: "no further scripted steps" };
    yield { type: "completed" };
    return;
  }
  if (step.kind === "tool-call") {
    yield {
      type: "tool_call",
      callId: `call-${index}`,
      toolName: step.toolName,
      input: step.input,
    };
    await Promise.resolve();
    yield { type: "completed" };
    return;
  }
  // Split text into deterministic chunks like the real fake provider.
  for (let offset = 0; offset < step.text.length; offset += 40) {
    yield { type: "text_delta", text: step.text.slice(offset, offset + 40) };
  }
  yield { type: "completed" };
}

function lightPlanText(objective = "Add health regeneration"): string {
  return JSON.stringify({
    depth: "light",
    objective,
    scope: { inScope: ["player health"], outOfScope: [] },
    nonGoals: [],
    touchpoints: [
      { id: "t1", path: "src/player/player.gd", confidence: "candidate" },
      { id: "t2", path: "tests/player/**", confidence: "candidate" },
    ],
    constraints: [],
    risks: [],
    steps: [
      { id: "step-1", title: "Update health timing", expectedTouchpoints: ["t1"] },
      { id: "step-2", title: "Extend health tests", expectedTouchpoints: ["t2"] },
    ],
    validation: { checks: ["check-only parse"], requirements: ["workspace mutation"] },
  });
}

function makeContract(): TaskContract {
  return createAdHocTaskContract("task-plan-exec", "Add health regeneration");
}

function makePlanner(
  providerFactory: () => ModelProvider,
  options: {
    readonly workspaceRoot?: string;
    readonly tools?: Tool[];
    readonly toolProjector?: ToolProjector;
    readonly maxToolRounds?: number;
    readonly maxAttempts?: number;
  } = {},
): PlannerPort {
  return createPlannerExecutor({
    providerFactory,
    tools:
      options.tools === undefined
        ? createPlannerToolRegistry({
            workspaceRoot: options.workspaceRoot ?? ".",
            godot: {} as GodotInspector,
            knowledge: {} as GodotKnowledge,
          })
        : createToolRegistry(options.tools),
    ...(options.toolProjector === undefined ? {} : { toolProjector: options.toolProjector }),
    ...(options.maxToolRounds === undefined ? {} : { maxToolRounds: options.maxToolRounds }),
    ...(options.maxAttempts === undefined ? {} : { maxAttempts: options.maxAttempts }),
  });
}

describe("createPlannerExecutor", () => {
  it("returns ready with validated content for valid structured output (fixture 10)", async () => {
    const planner = makePlanner(() =>
      createScriptedProvider([{ kind: "text", text: lightPlanText() }]),
    );
    const outcome = await planner.plan({ request: "x", contract: makeContract(), depth: "light" });
    expect(outcome.status).toBe("ready");
    if (outcome.status === "ready") {
      expect(outcome.content.objective).toContain("health regeneration");
      expect(outcome.content.touchpoints[0]?.confidence).toBe("candidate");
    }
  });

  it("rejects malformed output after the bounded retry budget (fixture 11)", async () => {
    let calls = 0;
    const planner = makePlanner(() => {
      calls += 1;
      return createScriptedProvider([
        { kind: "text", text: "I will write a plan in prose, not JSON." },
      ]);
    });
    const outcome = await planner.plan({ request: "x", contract: makeContract(), depth: "light" });
    expect(outcome.status).toBe("failed");
    if (outcome.status === "failed") {
      expect(outcome.message).toContain("valid JSON plan");
      expect(outcome.message).toContain("2 attempt");
    }
    expect(calls).toBe(2);
  });

  it("retries once and accepts corrected output within the budget", async () => {
    let calls = 0;
    const planner = makePlanner(() => {
      calls += 1;
      return createScriptedProvider([
        calls === 1
          ? { kind: "text", text: "not json at all" }
          : { kind: "text", text: lightPlanText("retried objective") },
      ]);
    });
    const outcome = await planner.plan({ request: "x", contract: makeContract(), depth: "light" });
    expect(outcome.status).toBe("ready");
    if (outcome.status === "ready") {
      expect(outcome.content.objective).toBe("retried objective");
    }
    expect(calls).toBe(2);
  });

  it("gives the planner a fresh provider context per attempt (fixture 33/18 separation)", async () => {
    const instances: string[] = [];
    const planner = makePlanner(() => {
      const id = `instance-${instances.length}`;
      instances.push(id);
      return createScriptedProvider([{ kind: "text", text: lightPlanText() }]);
    });
    await planner.plan({ request: "x", contract: makeContract(), depth: "light" });
    expect(instances.length).toBe(1);
  });

  it("projects NO mutation, process, or approval tools to the provider (fixtures 6/7/48)", async () => {
    const requests: ModelRequest[] = [];
    const planner = makePlanner(() =>
      createScriptedProvider(
        [{ kind: "text", text: lightPlanText() }],
        requests.length === 0 ? (request) => requests.push(request) : undefined,
      ),
    );
    await planner.plan({ request: "x", contract: makeContract(), depth: "light" });
    expect(requests.length).toBe(1);
    const toolNames = (requests[0]?.tools ?? []).map((tool) => tool.name);
    expect(toolNames).toContain("workspace.read");
    for (const forbidden of [
      "workspace.create_file",
      "workspace.edit_file",
      "workspace.delete_file",
      "workspace.apply_text_changeset",
      "process.run",
      "siralos.undo",
      "godot.probe_project",
      "godot.check_script",
      "godot.lsp_session",
      "approve",
    ]) {
      expect(toolNames).not.toContain(forbidden);
    }
  });

  it("refuses a mutating tool at the runtime boundary and never writes (fixture 9)", async () => {
    const parent = await mkdtemp(join(tmpdir(), "siralos-planner-exec-"));
    const workspace = join(parent, "workspace");
    await mkdir(workspace, { recursive: true });
    await writeFile(join(workspace, "target.gd"), "extends Node\n");
    const fakeWriteTool: Tool = {
      definition: {
        name: "evil.write",
        description: "a mutating tool that must never run inside the planner",
        inputSchema: { type: "object", properties: { path: { type: "string" } } },
      },
      capability: "workspace.write",
      async execute(input) {
        const candidate = (input as { path?: unknown })["path"];
        const target = typeof candidate === "string" ? candidate : "pwned.gd";
        await writeFile(join(workspace, target), "pwned\n");
        return { status: "success", output: {}, summary: "wrote" };
      },
    };
    try {
      const planner = makePlanner(
        () =>
          createScriptedProvider([
            { kind: "tool-call", toolName: "evil.write", input: { path: "pwned.gd" } },
            { kind: "tool-call", toolName: "evil.write", input: { path: "pwned.gd" } },
          ]),
        { tools: [fakeWriteTool] },
      );
      const outcome = await planner.plan({
        request: "x",
        contract: makeContract(),
        depth: "light",
      });
      // The mutating tool is refused at the runtime boundary; the attempt
      // fails cleanly (the fake provider has nothing else to say).
      expect(outcome.status).toBe("failed");
      await expect(readFile(join(workspace, "pwned.gd"), "utf8")).rejects.toThrow();
      expect(await readFile(join(workspace, "target.gd"), "utf8")).toBe("extends Node\n");
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });

  it("allows a final answer after exactly the configured tool-round budget", async () => {
    let executions = 0;
    const requests: ModelRequest[] = [];
    const readTool: Tool = {
      definition: { name: "workspace.read", description: "read", inputSchema: {} },
      capability: "workspace.read",
      execute: () => {
        executions += 1;
        return Promise.resolve({ status: "success", output: {}, summary: "read" });
      },
    };
    const planner = makePlanner(
      () =>
        createScriptedProvider(
          [
            { kind: "tool-call", toolName: "workspace.read", input: { path: "player.gd" } },
            { kind: "text", text: lightPlanText() },
          ],
          (request) => requests.push(request),
        ),
      { tools: [readTool], maxToolRounds: 1 },
    );

    const outcome = await planner.plan({ request: "x", contract: makeContract(), depth: "light" });

    expect(outcome.status).toBe("ready");
    expect(executions).toBe(1);
    expect(requests).toHaveLength(2);
  });

  it("never executes a gated projected tool even when the provider calls its visible name", async () => {
    let executions = 0;
    const requests: ModelRequest[] = [];
    const readTool: Tool = {
      definition: { name: "workspace.read", description: "read", inputSchema: {} },
      capability: "workspace.read",
      execute: () => {
        executions += 1;
        return Promise.resolve({ status: "success", output: {}, summary: "read" });
      },
    };
    const toolProjector: ToolProjector = {
      project() {
        return {
          fingerprint: "gated",
          tools: [
            {
              name: "workspace.read",
              visibility: "gated",
              description: "read",
              inputSchema: {},
            },
          ],
          counts: { available: 0, gated: 1, hidden: 0 },
          requestTools: [readTool.definition],
        };
      },
    };
    const planner = makePlanner(
      () =>
        createScriptedProvider(
          [
            { kind: "tool-call", toolName: "workspace.read", input: { path: "player.gd" } },
            { kind: "text", text: lightPlanText() },
          ],
          (request) => requests.push(request),
        ),
      { tools: [readTool], toolProjector, maxToolRounds: 1 },
    );

    const outcome = await planner.plan({ request: "x", contract: makeContract(), depth: "light" });

    expect(outcome.status).toBe("ready");
    expect(executions).toBe(0);
    expect(requests[0]?.tools.map((tool) => tool.name)).toEqual(["workspace.read"]);
    const result = requests[1]?.messages.find((item) => item.type === "tool_result");
    expect(result?.type).toBe("tool_result");
    if (result?.type === "tool_result" && result.result.status !== "success") {
      expect(result.result.message).toContain("host-projected planner tool surface");
    } else {
      throw new Error("expected the gated planner call to be refused");
    }
  });

  it("rejects an oversized UTF-8 prompt before creating a provider", async () => {
    let providerCreations = 0;
    const contract = createTaskContract({
      id: "task-large-planner-prompt",
      request: "Plan the change",
      acceptanceCriteria: Array.from({ length: 32 }, (_, index) => ({
        id: `criterion-${index}`,
        description: "界".repeat(1200),
        verificationKind: "deterministic" as const,
      })),
    });
    const planner = makePlanner(
      () => {
        providerCreations += 1;
        return createScriptedProvider([{ kind: "text", text: lightPlanText() }]);
      },
      { tools: [], maxAttempts: 1 },
    );

    const outcome = await planner.plan({ request: contract.request, contract, depth: "light" });

    expect(outcome.status).toBe("failed");
    if (outcome.status === "failed") {
      expect(outcome.message).toContain("prompt exceeded");
    }
    expect(providerCreations).toBe(0);
  });

  it("fails cleanly when the planner repeats identical reads without progress (fixture 29)", async () => {
    const parent = await mkdtemp(join(tmpdir(), "siralos-planner-stall-"));
    const workspace = join(parent, "workspace");
    await mkdir(join(workspace, "src", "player"), { recursive: true });
    await writeFile(join(workspace, "src", "player", "player.gd"), "extends Node\n");
    try {
      const readCall = {
        kind: "tool-call" as const,
        toolName: "workspace.read",
        input: { path: "src/player/player.gd", mode: "exact" },
      };
      const planner = makePlanner(() => createScriptedProvider([readCall, readCall, readCall]), {
        workspaceRoot: workspace,
      });
      const outcome = await planner.plan({
        request: "x",
        contract: makeContract(),
        depth: "light",
      });
      expect(outcome.status).toBe("failed");
      if (outcome.status === "failed") {
        expect(outcome.message).toContain("stalled");
      }
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });

  it("keeps planner private reasoning out of the stored content (fixture 35)", async () => {
    // The planner smuggles a private-reasoning field into its JSON; the
    // host validation boundary drops unknown fields, so the stored plan
    // content can never carry private reasoning.
    const text = JSON.stringify({
      ...JSON.parse(lightPlanText()),
      privateReasoning: "chain-of-thought: my private reasoning must never be stored",
    });
    const planner = makePlanner(() => createScriptedProvider([{ kind: "text", text }]));
    const outcome = await planner.plan({ request: "x", contract: makeContract(), depth: "light" });
    expect(outcome.status).toBe("ready");
    if (outcome.status === "ready") {
      expect(JSON.stringify(outcome.content)).not.toContain("chain-of-thought");
      expect(JSON.stringify(outcome.content)).not.toContain("privateReasoning");
    }
  });
});
