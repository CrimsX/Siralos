/**
 * tool-loop oracle probe (differential harness, ADR 0033,
 * Stage 3R R7.2).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Composes the REAL TypeScript reference: `createSiralosApplication`,
 * the real Tool Registry, the real Tool Round through the application,
 * the real permission evaluator, the deterministic fake provider for
 * terminal text, and probe-local scripted providers + deterministic
 * stub Tools where the frozen scenario requires controlled outcomes.
 * The probe only canonicalizes the observed events/history; it never
 * reimplements the application state machine.
 */
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createSiralosApplication } from "../../../packages/core/src/application/application.js";
import { createToolRegistry } from "../../../packages/core/src/tools/tool-registry.js";
import { createWorkspaceReadTool } from "../../../packages/adapters/src/tools/workspace/workspace-read-tool.js";
import { createDeterministicFakeProvider } from "../../../packages/adapters/src/providers/deterministic-fake-provider.js";
import { createDefaultPolicy } from "../../../packages/core/src/security/default-policy.js";
import { INSPECT_PROFILE } from "../../../packages/core/src/security/profile.js";

const MAX_INPUT_BYTES = 64 * 1024;

const stdin = readFileSync(0);
if (stdin.length === 0 || stdin.length > MAX_INPUT_BYTES) {
  throw new Error("probe input must be a bounded non-empty JSON document");
}
const input = JSON.parse(stdin.toString("utf8"));

/** Materialize deterministic $repeat markers recursively. */
function materialize(value) {
  if (Array.isArray(value)) {
    return value.map(materialize);
  }
  if (value !== null && typeof value === "object") {
    if (Object.hasOwn(value, "$eventsRepeat")) {
      const repeat = value.$eventsRepeat;
      if (
        !Array.isArray(repeat.events) ||
        !Number.isSafeInteger(repeat.count) ||
        repeat.count < 0 ||
        repeat.count > 4096 ||
        repeat.events.length * repeat.count > 4096
      ) {
        throw new Error("invalid $eventsRepeat marker");
      }
      const unit = repeat.events.map(materialize);
      const out = [];
      for (let index = 0; index < repeat.count; index += 1) {
        out.push(...unit.map((entry) => structuredClone(entry)));
      }
      return out;
    }
    if (Object.hasOwn(value, "$repeat")) {
      const repeat = value.$repeat;
      if (
        typeof repeat.character !== "string" ||
        [...repeat.character].length !== 1 ||
        !Number.isSafeInteger(repeat.count) ||
        repeat.count < 0 ||
        repeat.count > 1_048_576
      ) {
        throw new Error("invalid $repeat marker");
      }
      return repeat.character.repeat(repeat.count);
    }
    const out = {};
    for (const [key, child] of Object.entries(value)) {
      out[key] = materialize(child);
    }
    return out;
  }
  return value;
}

const STUB_DEFINITIONS = Object.freeze({
  "stub.success": {
    name: "stub.success",
    description: "Deterministic success stub tool.",
    inputSchema: { type: "object", additionalProperties: false },
  },
  "stub.invalid_input": {
    name: "stub.invalid_input",
    description: "Deterministic invalid-input stub tool.",
    inputSchema: { type: "object", additionalProperties: false },
  },
  "stub.denied": {
    name: "stub.denied",
    description: "Deterministic denied stub tool.",
    inputSchema: { type: "object", additionalProperties: false },
  },
  "stub.failed": {
    name: "stub.failed",
    description: "Deterministic failed stub tool.",
    inputSchema: { type: "object", additionalProperties: false },
  },
  "stub.cancelled": {
    name: "stub.cancelled",
    description: "Deterministic cancelled stub tool.",
    inputSchema: { type: "object", additionalProperties: false },
  },
  "a.tool": {
    name: "a.tool",
    description: "Deterministic success stub tool.",
    inputSchema: { type: "object", additionalProperties: false },
  },
  "b.tool": {
    name: "b.tool",
    description: "Deterministic success stub tool.",
    inputSchema: { type: "object", additionalProperties: false },
  },
});

function stubResult(name) {
  switch (name) {
    case "stub.success":
      return { status: "success", output: { ok: true }, summary: "stub success" };
    case "stub.invalid_input":
      return { status: "invalid_input", message: "stub invalid input." };
    case "stub.denied":
      return { status: "denied", message: "stub denied." };
    case "stub.failed":
      return { status: "failed", message: "stub failed." };
    case "stub.cancelled":
      return { status: "cancelled", message: "stub cancelled." };
    case "a.tool":
    case "b.tool":
      return { status: "success", output: { ok: true }, summary: "stub success" };
    default:
      throw new Error(`unknown stub tool ${name}`);
  }
}

function createStubTool(name, log) {
  const tool = {
    definition: STUB_DEFINITIONS[name],
    async execute() {
      return stubResult(name);
    },
  };
  return observeTool(tool, log);
}

/** Record every real Tool.execute invocation for the canonical record. */
function observeTool(tool, log) {
  const observed = {
    definition: tool.definition,
    async execute(toolInput, context) {
      log.push({ toolName: tool.definition.name, input: toolInput });
      return tool.execute(toolInput, context);
    },
  };
  if (tool.capability !== undefined) {
    observed.capability = tool.capability;
  }
  return observed;
}

/** Probe-local scripted provider; raw fixture events are authoritative. */
function scriptedProvider(events) {
  let position = 0;
  return {
    id: "tool-loop-scripted",
    stream() {
      const turn = [];
      while (position < events.length) {
        const event = events[position];
        position += 1;
        turn.push(event);
        if (event.type === "completed") break;
      }
      return {
        async *[Symbol.asyncIterator]() {
          for (const event of turn) {
            if (event.type === "provider_error") {
              throw new Error(event.message);
            }
            if (event.type === "tool_call" && typeof event.inputJson === "string") {
              yield {
                type: "tool_call",
                callId: event.callId,
                toolName: event.toolName,
                input: JSON.parse(event.inputJson),
              };
              continue;
            }
            yield event;
          }
        },
      };
    },
  };
}

/** Count provider request/turn invocations without changing the stream. */
function countingProvider(inner, counter) {
  return {
    id: inner.id,
    stream(request) {
      counter.count += 1;
      return inner.stream(request);
    },
  };
}

/**
 * Narrow approved-visible-surface seam. The probe uses the production
 * application's projection guard; it does not import R7.3 projection
 * services or reimplement the loop.
 */
function approvedSurfaceProjection(visibleNames) {
  let last = null;
  return {
    projectRequest(projectionInput) {
      const projectedTools = projectionInput.tools.filter((info) =>
        visibleNames.includes(info.definition.name),
      );
      const requestTools = projectedTools.map((info) => info.definition);
      last = {
        mode: projectionInput.mode,
        messages: projectionInput.messages,
        tools: projectedTools,
        system: null,
        pressure: {
          state: "normal",
          estimatedTokens: 0,
          workingMaximum: 1_000_000,
          ratio: 0,
        },
        toolProjection: {
          fingerprint: "tool-loop-approved-surface",
          tools: projectedTools.map((info) => ({
            name: info.definition.name,
            visibility: "available",
            description: info.definition.description,
            inputSchema: info.definition.inputSchema,
          })),
          counts: {
            available: projectedTools.length,
            gated: 0,
            hidden: 0,
          },
          requestTools,
        },
        contextProjection: {
          stableSegments: [],
          contextualSegments: [],
          volatileSegments: [],
          stableFingerprint: "",
          stablePrefixBytes: 0,
          stableBytes: 0,
          totalBytes: 0,
          estimatedTokens: 0,
        },
        estimatedTokens: 0,
        blocked: null,
      };
      return last;
    },
    projectToolResult(projectionInput) {
      return projectionInput.result;
    },
    lastProjection() {
      return last;
    },
    evidenceCacheSize() {
      return 0;
    },
  };
}

function canonicalizeToolResult(result) {
  if (result.status === "success") {
    return { status: "success", output: result.output, summary: result.summary };
  }
  return { status: result.status, message: result.message };
}

function canonicalizeHistory(history) {
  return history.map((item) => {
    switch (item.type) {
      case "user_message":
        return { type: "user_message", content: item.content };
      case "assistant_message":
        return { type: "assistant_message", content: item.content };
      case "assistant_tool_call":
        return item.input === undefined
          ? {
              type: "assistant_tool_call",
              callId: item.callId,
              toolName: item.toolName,
            }
          : {
              type: "assistant_tool_call",
              callId: item.callId,
              toolName: item.toolName,
              input: item.input,
            };
      case "tool_result":
        return {
          type: "tool_result",
          callId: item.callId,
          toolName: item.toolName,
          result: canonicalizeToolResult(item.result),
        };
      default:
        throw new Error(`unknown conversation item type ${item.type}`);
    }
  });
}

function canonicalizeEvent(event) {
  switch (event.type) {
    case "response_started":
    case "response_completed":
    case "response_cancelled":
      return { type: event.type };
    case "response_failed":
      return { type: "response_failed", message: event.message };
    case "text_delta":
      return { type: "text_delta", text: event.text };
    case "tool_started":
      return {
        type: "tool_started",
        callId: event.callId,
        toolName: event.toolName,
        displayInputUtf16: (() => {
          const units = [];
          for (let index = 0; index < event.displayInput.length; index += 1) {
            units.push(event.displayInput.charCodeAt(index));
          }
          return units;
        })(),
      };
    case "tool_completed":
      return {
        type: "tool_completed",
        callId: event.callId,
        toolName: event.toolName,
        summary: event.summary,
      };
    case "tool_failed":
      return {
        type: "tool_failed",
        callId: event.callId,
        toolName: event.toolName,
        message: event.message,
      };
    case "tool_cancelled":
      return {
        type: "tool_cancelled",
        callId: event.callId,
        toolName: event.toolName,
      };
    default:
      throw new Error(`unexpected R7.2 event type ${event.type}`);
  }
}

function terminalFromEvents(events) {
  const last = events.at(-1);
  if (last.type === "response_completed") return { kind: "completed" };
  if (last.type === "response_cancelled") return { kind: "cancelled" };
  if (last.type === "response_failed") {
    return { kind: "failed", message: last.message };
  }
  throw new Error("event sequence has no terminal response outcome");
}

function buildToolCalls(history, executionLog) {
  const results = new Map();
  for (const item of history) {
    if (item.type === "tool_result") {
      results.set(item.callId, item.result);
    }
  }
  let executionIndex = 0;
  const calls = [];
  for (const item of history) {
    if (item.type !== "assistant_tool_call") continue;
    const executed = (() => {
      if (item.input === undefined) return false;
      const next = executionLog[executionIndex];
      if (next !== undefined && next.toolName === item.toolName) {
        executionIndex += 1;
        return true;
      }
      return false;
    })();
    const result = results.get(item.callId);
    if (result === undefined) {
      throw new Error(`missing paired result for ${item.callId}`);
    }
    const call = {
      callId: item.callId,
      toolName: item.toolName,
      inputPresent: item.input !== undefined,
      ...(item.input === undefined ? {} : { input: item.input }),
      executed,
      result: canonicalizeToolResult(result),
    };
    calls.push(call);
  }
  return calls;
}

async function runCase(caseValue, caseIndex) {
  const needsFixture = caseValue.tools.includes("workspace.read");
  const fixtureRoot = needsFixture
    ? mkdtempSync(join(tmpdir(), "siralos-oracle-tool-loop-"))
    : null;
  try {
    if (fixtureRoot !== null) {
      writeFileSync(join(fixtureRoot, "fixture.txt"), "hello\nworld\n");
    }
    const executionLog = [];
    const tools = caseValue.tools.map((name) => {
      if (name === "workspace.read") {
        return observeTool(createWorkspaceReadTool(fixtureRoot), executionLog);
      }
      if (Object.hasOwn(STUB_DEFINITIONS, name)) {
        return createStubTool(name, executionLog);
      }
      throw new Error(`unknown tool registry selection ${name}`);
    });
    const registry = createToolRegistry(tools);
    const baseRules = createDefaultPolicy("inspect").rules;
    const rules = { ...baseRules };
    for (const rule of caseValue.rules ?? []) {
      rules[rule.capability] = rule.decision;
    }
    const policy = { rules };
    const surface =
      caseValue.visibleTools === undefined
        ? undefined
        : approvedSurfaceProjection(caseValue.visibleTools);
    const providerCounter = { count: 0 };
    const innerProvider =
      caseValue.provider.kind === "fake"
        ? createDeterministicFakeProvider()
        : scriptedProvider(caseValue.provider.events);
    const provider = countingProvider(innerProvider, providerCounter);
    const maxToolRounds =
      caseValue.maxToolRounds === "non-finite" ? Number.NaN : caseValue.maxToolRounds;
    const application = createSiralosApplication({
      provider,
      tools: registry,
      policy,
      profile: INSPECT_PROFILE,
      ...(surface === undefined ? {} : { projection: surface }),
      ...(maxToolRounds === undefined ? {} : { maxToolRounds }),
    });
    const controller = new AbortController();
    const events = [];
    let completedCalls = 0;
    for await (const event of application.sendPrompt(caseValue.prompt, controller.signal)) {
      events.push(event);
      if (
        event.type === "tool_completed" &&
        caseValue.cancelAfterCompletedToolCalls !== undefined
      ) {
        completedCalls += 1;
        if (completedCalls === caseValue.cancelAfterCompletedToolCalls) {
          controller.abort();
        }
      }
    }
    return {
      caseIndex,
      events: events.map(canonicalizeEvent),
      terminal: terminalFromEvents(events),
      providerTurnCount: providerCounter.count,
      history: canonicalizeHistory(application.getHistory()),
      completedToolRounds: application.getCompletedToolRounds(),
      toolCalls: buildToolCalls(application.getHistory(), executionLog),
    };
  } finally {
    if (fixtureRoot !== null) {
      rmSync(fixtureRoot, { recursive: true, force: true });
    }
  }
}

const caseValues = input.cases.map(materialize);
const cases = [];
for (const [index, caseValue] of caseValues.entries()) {
  cases.push(await runCase(caseValue, index));
}
process.stdout.write(JSON.stringify({ cases }));
