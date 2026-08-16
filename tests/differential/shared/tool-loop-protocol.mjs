/**
 * Tool-loop canonical-result validation (Stage 3R R7.2).
 *
 * Kept as a sibling module of protocol.mjs so the large subject-specific
 * validator stays locally inspectable. The runner protocol remains owned
 * by protocol.mjs; this module only validates one completed result shape.
 */

const TOOL_RESULT_FAILURE_STATUSES = new Set([
  "invalid_input",
  "denied",
  "conflict",
  "failed",
  "cancelled",
  "timed_out",
  "output_limit",
  "sandbox_denied",
  "sandbox_unavailable",
  "workspace_violation",
  "unavailable",
]);

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function assertExactKeys(value, expected, label) {
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    throw new Error(`${label} has unknown or missing fields`);
  }
}

function validateToolResultValue(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label} must be an object`);
  }
  if (result.status === "success") {
    assertExactKeys(result, ["status", "output", "summary"], label);
    if (typeof result.summary !== "string") {
      throw new Error(`${label}.summary must be a string`);
    }
    return;
  }
  assertExactKeys(result, ["status", "message"], label);
  if (!TOOL_RESULT_FAILURE_STATUSES.has(result.status)) {
    throw new Error(`${label}.status is not a supported failure status`);
  }
  if (typeof result.message !== "string") {
    throw new Error(`${label}.message must be a string`);
  }
}

function validateHistoryItem(item, label) {
  if (!isObject(item) || typeof item.type !== "string") {
    throw new Error(`${label} history entries must be typed objects`);
  }
  if (item.type === "user_message" || item.type === "assistant_message") {
    assertExactKeys(item, ["type", "content"], label);
    if (typeof item.content !== "string") {
      throw new Error(`${label}.content must be a string`);
    }
    return;
  }
  if (item.type === "assistant_tool_call") {
    if (Object.hasOwn(item, "input")) {
      assertExactKeys(item, ["type", "callId", "toolName", "input"], label);
    } else {
      assertExactKeys(item, ["type", "callId", "toolName"], label);
    }
    if (typeof item.callId !== "string" || typeof item.toolName !== "string") {
      throw new Error(`${label} tool-call identity fields must be strings`);
    }
    return;
  }
  if (item.type === "tool_result") {
    assertExactKeys(item, ["type", "callId", "toolName", "result"], label);
    if (typeof item.callId !== "string" || typeof item.toolName !== "string") {
      throw new Error(`${label} tool-result identity fields must be strings`);
    }
    validateToolResultValue(item.result, `${label}.result`);
    return;
  }
  throw new Error(`${label} has an unknown history item type`);
}

function validateToolLoopEvent(event, label) {
  if (!isObject(event) || typeof event.type !== "string") {
    throw new Error(`${label} events must be typed objects`);
  }
  switch (event.type) {
    case "response_started":
    case "response_completed":
    case "response_cancelled":
      assertExactKeys(event, ["type"], label);
      return;
    case "response_failed":
      assertExactKeys(event, ["type", "message"], label);
      if (typeof event.message !== "string" || event.message.length === 0) {
        throw new Error(`${label}.message must be a non-empty string`);
      }
      return;
    case "text_delta":
      assertExactKeys(event, ["type", "text"], label);
      if (typeof event.text !== "string") {
        throw new Error(`${label}.text must be a string`);
      }
      return;
    case "tool_started":
      assertExactKeys(event, ["type", "callId", "toolName", "displayInputUtf16"], label);
      if (typeof event.callId !== "string" || typeof event.toolName !== "string") {
        throw new Error(`${label} tool identity fields must be strings`);
      }
      if (!Array.isArray(event.displayInputUtf16) || event.displayInputUtf16.length > 4096) {
        throw new Error(`${label}.displayInputUtf16 must be a bounded array`);
      }
      for (const unit of event.displayInputUtf16) {
        if (!Number.isSafeInteger(unit) || unit < 0 || unit > 65_535) {
          throw new Error(`${label}.displayInputUtf16 contains an invalid code unit`);
        }
      }
      return;
    case "tool_completed":
      assertExactKeys(event, ["type", "callId", "toolName", "summary"], label);
      if (typeof event.summary !== "string") {
        throw new Error(`${label}.summary must be a string`);
      }
      return;
    case "tool_failed":
      assertExactKeys(event, ["type", "callId", "toolName", "message"], label);
      if (typeof event.message !== "string" || event.message.length === 0) {
        throw new Error(`${label}.message must be a non-empty string`);
      }
      return;
    case "tool_cancelled":
      assertExactKeys(event, ["type", "callId", "toolName"], label);
      return;
    default:
      throw new Error(`${label} has an unknown event type`);
  }
}

function validateToolLoopCase(caseValue, label) {
  assertExactKeys(
    caseValue,
    [
      "caseIndex",
      "events",
      "terminal",
      "providerTurnCount",
      "history",
      "completedToolRounds",
      "toolCalls",
    ],
    label,
  );
  if (!Number.isSafeInteger(caseValue.caseIndex) || caseValue.caseIndex < 0) {
    throw new Error(`${label}.caseIndex is invalid`);
  }
  if (!Array.isArray(caseValue.events) || caseValue.events.length === 0) {
    throw new Error(`${label}.events must be a non-empty array`);
  }
  for (const [index, event] of caseValue.events.entries()) {
    validateToolLoopEvent(event, `${label}.events[${index}]`);
  }
  if (!isObject(caseValue.terminal)) {
    throw new Error(`${label}.terminal must be an object`);
  }
  if (caseValue.terminal.kind === "completed" || caseValue.terminal.kind === "cancelled") {
    assertExactKeys(caseValue.terminal, ["kind"], `${label}.terminal`);
  } else if (caseValue.terminal.kind === "failed") {
    assertExactKeys(caseValue.terminal, ["kind", "message"], `${label}.terminal`);
    if (typeof caseValue.terminal.message !== "string" || caseValue.terminal.message.length === 0) {
      throw new Error(`${label}.terminal.message must be a non-empty string`);
    }
  } else {
    throw new Error(`${label}.terminal kind is invalid`);
  }
  if (
    !Number.isSafeInteger(caseValue.providerTurnCount) ||
    caseValue.providerTurnCount < 0 ||
    !Number.isSafeInteger(caseValue.completedToolRounds) ||
    caseValue.completedToolRounds < 0
  ) {
    throw new Error(`${label} counters are invalid`);
  }
  if (!Array.isArray(caseValue.history) || caseValue.history.length === 0) {
    throw new Error(`${label}.history must be a non-empty array`);
  }
  for (const [index, item] of caseValue.history.entries()) {
    validateHistoryItem(item, `${label}.history[${index}]`);
  }
  if (!Array.isArray(caseValue.toolCalls)) {
    throw new Error(`${label}.toolCalls must be an array`);
  }
  for (const [index, call] of caseValue.toolCalls.entries()) {
    const callLabel = `${label}.toolCalls[${index}]`;
    if (!isObject(call)) {
      throw new Error(`${callLabel} must be an object`);
    }
    if (call.inputPresent === true) {
      assertExactKeys(
        call,
        ["callId", "toolName", "inputPresent", "input", "executed", "result"],
        callLabel,
      );
    } else if (call.inputPresent === false) {
      assertExactKeys(
        call,
        ["callId", "toolName", "inputPresent", "executed", "result"],
        callLabel,
      );
    } else {
      throw new Error(`${callLabel}.inputPresent must be a boolean`);
    }
    if (
      typeof call.callId !== "string" ||
      typeof call.toolName !== "string" ||
      typeof call.executed !== "boolean"
    ) {
      throw new Error(`${callLabel} identity/execution fields are invalid`);
    }
    validateToolResultValue(call.result, `${callLabel}.result`);
  }
}

/** Validate one completed `tool-loop` scenario result. */
export function validateToolLoopResult(record, label) {
  assertExactKeys(record.result, ["cases"], `${label}.result`);
  if (
    !Array.isArray(record.result.cases) ||
    record.result.cases.length === 0 ||
    record.result.cases.length > 64
  ) {
    throw new Error(`${label}.result.cases must contain 1-64 entries`);
  }
  for (const [index, caseValue] of record.result.cases.entries()) {
    validateToolLoopCase(caseValue, `${label}.result.cases[${index}]`);
    if (caseValue.caseIndex !== index) {
      throw new Error(`${label}.result.cases[${index}].caseIndex must equal its array index`);
    }
  }
}
