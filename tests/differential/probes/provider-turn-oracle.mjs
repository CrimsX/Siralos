/**
 * provider-turn oracle probe (differential harness, ADR 0033,
 * Stage 3R R7.1).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Runs each provider-turn case against the REAL TypeScript reference:
 * the deterministic fake provider, the application turn collector
 * (collectProviderTurn), and the bounded tool-result detach boundary.
 * Adversarial cases use a probe-local scripted provider whose raw
 * events flow through the production collector validation; the probe
 * only composes reference code and canonicalizes its result.
 *
 * Deterministic: no ambient clock, randomness, or environment access
 * enters records.
 */
import { readFileSync } from "node:fs";
import { collectProviderTurn } from "../../../packages/core/src/application/provider-turn.js";
import { createToolRegistry } from "../../../packages/core/src/tools/tool-registry.js";
import { createDeterministicFakeProvider } from "../../../packages/adapters/src/providers/deterministic-fake-provider.js";
import { detachBoundedToolResult } from "../../../packages/adapters/src/providers/bounded-model-turn.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

/** Materialize deterministic $repeat markers into strings. */
function materialize(value) {
  if (Array.isArray(value)) {
    return value.map(materialize);
  }
  if (value !== null && typeof value === "object") {
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

function parseConversationItems(items) {
  return items.map((item) => {
    switch (item.type) {
      case "user_message":
        return { type: "user_message", content: item.content };
      case "assistant_message":
        return { type: "assistant_message", content: item.content };
      case "assistant_tool_call":
        return {
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
          result: item.result,
        };
      default:
        throw new Error("unknown conversation item type");
    }
  });
}

function parseToolDefinitions(tools) {
  return tools.map((tool) => ({
    name: tool.name,
    description: tool.description,
    inputSchema: tool.inputSchema,
  }));
}

/**
 * Probe-local scripted provider: yields the raw fixture events; the
 * production collector validates them through its trust boundary.
 */
function scriptedProvider(events) {
  return {
    id: "scripted-provider",
    stream() {
      return {
        async *[Symbol.asyncIterator]() {
          for (const event of events) {
            yield event;
            await Promise.resolve();
          }
        },
      };
    },
  };
}

/**
 * Host-scripted cancellation wrapper: aborts after exactly N events
 * have been emitted (0 = before the first event).
 */
function cancellationWrapper(inner, controller, cancelAfter) {
  return {
    async *[Symbol.asyncIterator]() {
      let emitted = 0;
      for await (const event of inner) {
        if (emitted === cancelAfter) {
          // Host-scripted cancellation point: stop without yielding the
          // next event; the collector's post-loop cancellation check
          // decides the cancelled outcome.
          controller.abort();
          return;
        }
        emitted += 1;
        yield event;
      }
    },
  };
}

/** Stable failure category derived from the frozen reference message. */
function failureCode(message) {
  if (
    message ===
    "The provider exceeded the assistant-text byte limit limit; the response was rejected."
  ) {
    return "LIMIT_ASSISTANT_TEXT_BYTES";
  }
  if (message === "The provider exceeded the text-event count limit; the response was rejected.") {
    return "LIMIT_TEXT_EVENT_COUNT";
  }
  if (message === "The provider exceeded the tool-call count limit; the response was rejected.") {
    return "LIMIT_TOOL_CALL_COUNT";
  }
  if (
    message ===
    "The provider exceeded the tool-call id byte limit limit; the response was rejected."
  ) {
    return "LIMIT_CALL_ID_BYTES";
  }
  if (
    message === "The provider exceeded the tool-name byte limit limit; the response was rejected."
  ) {
    return "LIMIT_TOOL_NAME_BYTES";
  }
  if (
    message ===
    "The provider exceeded the tool-argument byte limit limit; the response was rejected."
  ) {
    return "LIMIT_TOOL_ARGUMENT_BYTES";
  }
  if (
    message ===
    "The provider exceeded the aggregate turn byte limit limit; the response was rejected."
  ) {
    return "LIMIT_AGGREGATE_TURN_BYTES";
  }
  if (
    message ===
    "The provider exceeded the tool-argument JSON validity limit; the response was rejected."
  ) {
    return "INVALID_TOOL_ARGUMENT_JSON";
  }
  if (
    message === "The provider exceeded an event after completion limit; the response was rejected."
  ) {
    return "EVENT_AFTER_COMPLETION";
  }
  if (
    message === "The provider stream ended without a completion event; the response was rejected."
  ) {
    return "EOF_WITHOUT_COMPLETION";
  }
  if (message === "The provider emitted an unknown event type; the response was rejected.") {
    return "UNKNOWN_EVENT_TYPE";
  }
  if (message === "The provider emitted a malformed event; the response was rejected.") {
    return "MALFORMED_EVENT";
  }
  if (
    message ===
    "The provider emitted a text event without a string payload; the response was rejected."
  ) {
    return "MALFORMED_TEXT_EVENT";
  }
  if (
    message ===
    "The provider emitted a tool call with a non-string id or name; the response was rejected."
  ) {
    return "MALFORMED_TOOL_CALL";
  }
  if (
    message.startsWith(
      "The conversation transcript is structurally invalid; the provider request was blocked: ",
    )
  ) {
    return "INVALID_TRANSCRIPT";
  }
  return "PROVIDER_FAILED";
}

function toolCallValue(call) {
  if (call.kind === "execute") {
    return {
      kind: "execute",
      callId: call.callId,
      toolName: call.toolName,
      input: call.input,
    };
  }
  return {
    kind: "invalid",
    callId: call.callId,
    toolName: call.toolName,
    message: call.message,
  };
}

async function runTurnCase(turnCase) {
  const caseValue = materialize(turnCase);
  const providerSpec = caseValue.provider;
  const messages = parseConversationItems(caseValue.messages);
  const toolDefinitions = parseToolDefinitions(caseValue.tools);
  const cancelAfter = caseValue.cancelAfterEvents;
  const controller = new AbortController();
  let innerProvider;
  if (providerSpec.kind === "fake") {
    innerProvider = createDeterministicFakeProvider();
  } else {
    innerProvider = scriptedProvider(providerSpec.events);
  }
  const provider = {
    id: innerProvider.id,
    stream(request) {
      const inner = innerProvider.stream(request);
      if (cancelAfter === undefined) {
        return inner;
      }
      return cancellationWrapper(inner, controller, cancelAfter);
    },
  };
  const context = {
    provider,
    tools: createToolRegistry([]),
    toolDefinitions,
    history: messages,
  };
  const signal = cancelAfter === undefined ? undefined : controller.signal;
  const deltas = [];
  const generator = collectProviderTurn(context, signal);
  let outcome;
  for (;;) {
    const step = await generator.next();
    if (step.done === true) {
      outcome = step.value;
      break;
    }
    if (step.value.type === "text_delta") {
      deltas.push(step.value.text);
    }
  }
  if (outcome.kind === "cancelled") {
    return { kind: "cancelled" };
  }
  if (outcome.kind === "failed") {
    return {
      kind: "failed",
      failure: failureCode(outcome.message),
      message: outcome.message,
    };
  }
  return {
    kind: "turn",
    assistantText: outcome.assistantText,
    textDeltas: deltas,
    toolCalls: outcome.toolCalls.map(toolCallValue),
  };
}

function runDetachCase(detachCase) {
  const caseValue = materialize(detachCase);
  const outcome = detachBoundedToolResult(caseValue.value, caseValue.maxBytes, caseValue.actor);
  if (outcome.ok) {
    return {
      ok: true,
      result: outcome.result,
      byteLength: outcome.byteLength,
    };
  }
  return { ok: false, message: outcome.message };
}

const input = readStdinBounded();
const cases = [];
for (const entry of input.cases) {
  if (Object.hasOwn(entry, "turn")) {
    cases.push({ turn: await runTurnCase(entry.turn) });
  } else {
    cases.push({ detach: runDetachCase(entry.detach) });
  }
}
process.stdout.write(JSON.stringify({ cases }));
