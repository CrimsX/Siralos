import type {
  ConversationItem,
  JsonObject,
  JsonValue,
  ModelEvent,
  ModelProvider,
  ModelRequest,
  ToolDefinition,
  ToolExecutionResult,
} from "@solaris/core";

export const DETERMINISTIC_FAKE_PROVIDER_ID = "deterministic-fake";

const CHUNK_SIZE = 16;

type Scenario =
  | {
      readonly kind: "list";
      readonly toolName: "workspace.list";
      readonly input: { readonly path: string };
    }
  | {
      readonly kind: "read";
      readonly toolName: "workspace.read";
      readonly input: { readonly path: string };
    }
  | {
      readonly kind: "search";
      readonly toolName: "workspace.search";
      readonly input: { readonly query: string; readonly path: string };
    };

export function createDeterministicFakeProvider(): ModelProvider {
  return {
    id: DETERMINISTIC_FAKE_PROVIDER_ID,
    stream,
  };
}

type GitScenario = "git-status" | "diff-working" | "diff-staged" | "diff-head";

function findGitScenario(messages: readonly ConversationItem[]): GitScenario | null {
  const latestUserPrompt = findLatestUserPrompt(messages);
  if (latestUserPrompt === "git status") {
    return "git-status";
  }
  if (latestUserPrompt === "show working diff") {
    return "diff-working";
  }
  if (latestUserPrompt === "show staged diff") {
    return "diff-staged";
  }
  if (latestUserPrompt === "show head diff") {
    return "diff-head";
  }
  return null;
}

function gitScenarioTool(scenario: GitScenario): string {
  return scenario === "git-status" ? "git.status" : "git.diff";
}

function gitScenarioInput(scenario: GitScenario): unknown {
  if (scenario === "git-status") {
    return {};
  }
  return {
    scope: scenario === "diff-working" ? "working" : scenario === "diff-staged" ? "staged" : "head",
  };
}

function formatGitFinalText(scenario: GitScenario, result: ToolExecutionResult): string {
  if (result.status !== "success") {
    return `Solaris could not inspect Git: ${result.message}`;
  }
  const record = result.output as JsonObject;
  if (scenario === "git-status") {
    const changes = Array.isArray(record["changes"]) ? record["changes"] : [];
    const untracked = Array.isArray(record["untracked"]) ? record["untracked"] : [];
    return `Solaris found ${changes.length} modified files and ${untracked.length} untracked file${untracked.length === 1 ? "" : "s"}.`;
  }
  const files = Array.isArray(record["files"]) ? record["files"] : [];
  return `Solaris inspected a ${scenario === "diff-working" ? "working" : scenario === "diff-staged" ? "staged" : "HEAD"} diff of ${files.length} file${files.length === 1 ? "" : "s"}.`;
}

async function* stream(request: ModelRequest): AsyncIterable<ModelEvent> {
  const signal = request.signal;
  if (signal?.aborted) {
    throw createAbortError();
  }
  const gitScenario = findGitScenario(request.messages);
  if (gitScenario !== null && isToolAvailable(request.tools, gitScenarioTool(gitScenario))) {
    const toolName = gitScenarioTool(gitScenario);
    const result = findLatestResult(itemsAfterLastUserMessage(request.messages), toolName);
    if (result === undefined) {
      yield {
        type: "tool_call",
        callId: "call-git",
        toolName,
        input: gitScenarioInput(gitScenario),
      };
      await Promise.resolve();
      return;
    }
    yield* streamTextChunks(formatGitFinalText(gitScenario, result), signal);
    return;
  }
  const writeScenario = findWriteScenario(request.messages);
  if (writeScenario !== null && writeScenarioToolsAvailable(writeScenario, request.tools)) {
    const turn = buildWriteScenarioTurn(writeScenario, request.messages);
    if (turn.kind === "call") {
      yield turn.event;
      await Promise.resolve();
      return;
    }
    yield* streamTextChunks(turn.text, signal);
    return;
  }
  const commandScenario = findCommandScenario(request.messages);
  if (commandScenario !== null) {
    if (isToolAvailable(request.tools, "process.run")) {
      const result = findLatestResult(itemsAfterLastUserMessage(request.messages), "process.run");
      if (result === undefined) {
        yield {
          type: "tool_call",
          callId: "call-command",
          toolName: "process.run",
          input: commandScenarioInput(commandScenario),
        };
        await Promise.resolve();
        return;
      }
      yield* streamTextChunks(formatCommandFinalText(commandScenario, result), signal);
      return;
    }
    yield* streamTextChunks(
      `Solaris cannot run development commands in this profile (process.run is unavailable).`,
      signal,
    );
    return;
  }
  const scenario = findScenario(request.messages);
  if (scenario !== null) {
    const result = findResultForCall(request.messages, "call-1");
    if (result === undefined) {
      if (isToolAvailable(request.tools, scenario.toolName)) {
        yield {
          type: "tool_call",
          callId: "call-1",
          toolName: scenario.toolName,
          input: scenario.input,
        };
        await Promise.resolve();
        return;
      }
    } else {
      const responseText = formatScenarioResponse(scenario, result);
      yield* streamTextChunks(responseText, signal);
      return;
    }
  }
  const responseText = formatResponse(request.messages);
  yield* streamTextChunks(responseText, signal);
}

type WriteScenario = "create" | "edit" | "delete";

const WRITE_TEST_FILE = "solaris-write-test.txt";
const WRITE_TEST_CONTENT = "Created by the deterministic Solaris test provider.\n";

function findWriteScenario(messages: readonly ConversationItem[]): WriteScenario | null {
  const latestUserPrompt = findLatestUserPrompt(messages);
  if (latestUserPrompt === "create solaris-write-test") {
    return "create";
  }
  if (latestUserPrompt === "edit solaris-write-test") {
    return "edit";
  }
  if (latestUserPrompt === "delete solaris-write-test") {
    return "delete";
  }
  return null;
}

function writeScenarioToolsAvailable(
  scenario: WriteScenario,
  tools: readonly ToolDefinition[],
): boolean {
  switch (scenario) {
    case "create":
      return isToolAvailable(tools, "workspace.create_file");
    case "edit":
      return (
        isToolAvailable(tools, "workspace.read") && isToolAvailable(tools, "workspace.edit_file")
      );
    case "delete":
      return (
        isToolAvailable(tools, "workspace.read") && isToolAvailable(tools, "workspace.delete_file")
      );
  }
}

type WriteScenarioTurn =
  | { readonly kind: "call"; readonly event: ModelEvent }
  | { readonly kind: "text"; readonly text: string };

function buildWriteScenarioTurn(
  scenario: WriteScenario,
  messages: readonly ConversationItem[],
): WriteScenarioTurn {
  const stepItems = itemsAfterLastUserMessage(messages);
  switch (scenario) {
    case "create": {
      const result = findLatestResult(stepItems, "workspace.create_file");
      if (result === undefined) {
        return {
          kind: "call",
          event: {
            type: "tool_call",
            callId: "call-create",
            toolName: "workspace.create_file",
            input: { path: WRITE_TEST_FILE, content: WRITE_TEST_CONTENT },
          },
        };
      }
      return { kind: "text", text: formatWriteFinalText("create", result) };
    }
    case "edit": {
      const editResult = findLatestResult(stepItems, "workspace.edit_file");
      if (editResult === undefined) {
        const readResult = findLatestResult(stepItems, "workspace.read");
        if (readResult === undefined) {
          return {
            kind: "call",
            event: {
              type: "tool_call",
              callId: "call-read",
              toolName: "workspace.read",
              input: { path: WRITE_TEST_FILE },
            },
          };
        }
        const hash = readResultSha256(readResult);
        if (hash === null) {
          return {
            kind: "text",
            text: `Solaris could not read ${WRITE_TEST_FILE}, so it did not modify it.`,
          };
        }
        return {
          kind: "call",
          event: {
            type: "tool_call",
            callId: "call-edit",
            toolName: "workspace.edit_file",
            input: {
              path: WRITE_TEST_FILE,
              expectedSha256: hash,
              replacements: [{ oldText: "Created", newText: "Updated" }],
            },
          },
        };
      }
      return { kind: "text", text: formatWriteFinalText("edit", editResult) };
    }
    case "delete": {
      const deleteResult = findLatestResult(stepItems, "workspace.delete_file");
      if (deleteResult === undefined) {
        const readResult = findLatestResult(stepItems, "workspace.read");
        if (readResult === undefined) {
          return {
            kind: "call",
            event: {
              type: "tool_call",
              callId: "call-read",
              toolName: "workspace.read",
              input: { path: WRITE_TEST_FILE },
            },
          };
        }
        const hash = readResultSha256(readResult);
        if (hash === null) {
          return {
            kind: "text",
            text: `Solaris could not read ${WRITE_TEST_FILE}, so it did not delete it.`,
          };
        }
        return {
          kind: "call",
          event: {
            type: "tool_call",
            callId: "call-delete",
            toolName: "workspace.delete_file",
            input: { path: WRITE_TEST_FILE, expectedSha256: hash },
          },
        };
      }
      return { kind: "text", text: formatWriteFinalText("delete", deleteResult) };
    }
  }
}

function formatWriteFinalText(
  operation: "create" | "edit" | "delete",
  result: ToolExecutionResult,
): string {
  const verb = operation === "create" ? "create" : operation === "edit" ? "modify" : "delete";
  switch (result.status) {
    case "success":
      return `Solaris ${operation === "create" ? "created" : operation === "edit" ? "updated" : "deleted"} ${WRITE_TEST_FILE}.`;
    case "denied":
      return `The workspace change was denied, so Solaris did not ${verb} ${WRITE_TEST_FILE}.`;
    case "conflict":
      return `The file changed, so Solaris did not ${verb} ${WRITE_TEST_FILE}. Reread the file to continue.`;
    case "cancelled":
      return `The workspace change was cancelled, so Solaris did not ${verb} ${WRITE_TEST_FILE}.`;
    case "invalid_input":
    case "failed":
    case "unavailable":
    case "timed_out":
    case "output_limit":
    case "sandbox_denied":
    case "sandbox_unavailable":
    case "workspace_violation":
      return `Solaris could not ${verb} ${WRITE_TEST_FILE}: ${result.message}`;
  }
}

type CommandScenario =
  | {
      readonly kind: "npm-check";
      readonly display: string;
    }
  | {
      readonly kind: "npm-test";
      readonly display: string;
    }
  | {
      readonly kind: "node-fixture";
      readonly display: string;
    };

const NODE_FIXTURE_PATH = "scripts/process-validation-fixture.mjs";

function findCommandScenario(messages: readonly ConversationItem[]): CommandScenario | null {
  const latestUserPrompt = findLatestUserPrompt(messages);
  if (latestUserPrompt === "run npm check") {
    return { kind: "npm-check", display: "npm run check" };
  }
  if (latestUserPrompt === "run npm test") {
    return { kind: "npm-test", display: "npm run test" };
  }
  if (latestUserPrompt === "run node validation fixture") {
    return { kind: "node-fixture", display: `node ${NODE_FIXTURE_PATH}` };
  }
  return null;
}

function commandScenarioInput(scenario: CommandScenario): JsonValue {
  switch (scenario.kind) {
    case "npm-check":
      return {
        runner: "npm-script",
        script: "check",
        arguments: [],
        workingDirectory: ".",
      };
    case "npm-test":
      return {
        runner: "npm-script",
        script: "test",
        arguments: [],
        workingDirectory: ".",
      };
    case "node-fixture":
      return {
        runner: "node-script",
        path: NODE_FIXTURE_PATH,
        arguments: [],
        workingDirectory: ".",
      };
  }
}

function formatCommandFinalText(scenario: CommandScenario, result: ToolExecutionResult): string {
  const display = scenario.display;
  switch (result.status) {
    case "success": {
      const exitCode =
        typeof result.output === "object" &&
        result.output !== null &&
        !Array.isArray(result.output) &&
        typeof (result.output as JsonObject)["exitCode"] === "number"
          ? (result.output as JsonObject)["exitCode"]
          : null;
      if (exitCode !== null && exitCode !== 0) {
        return `Solaris ran \`${display}\`, but it exited with code ${exitCode}.`;
      }
      return `Solaris ran \`${display}\` and it exited with code 0.`;
    }
    case "denied":
      return `The command was not approved, so Solaris did not run it.`;
    case "conflict":
      return `The command plan changed, so Solaris did not run it. Request the command again.`;
    case "cancelled":
      return `The command was cancelled before it completed.`;
    case "timed_out":
      return `The command timed out and its process tree was terminated.`;
    case "sandbox_denied":
      return `The sandbox denied part of the command: ${result.message}`;
    case "sandbox_unavailable":
      return `The sandbox is unavailable, so the command did not run.`;
    case "workspace_violation":
      return `Solaris detected unexpected workspace changes; command execution is disabled for this session.`;
    case "output_limit":
      return `The command exceeded its output limit and was terminated.`;
    case "unavailable":
    case "invalid_input":
    case "failed":
      return `Solaris could not run the command: ${result.message}`;
  }
}

function itemsAfterLastUserMessage(
  messages: readonly ConversationItem[],
): readonly ConversationItem[] {
  const start = findLatestUserMessageIndex(messages) + 1;
  return messages.slice(start);
}

function findLatestResult(
  items: readonly ConversationItem[],
  toolName: string,
): ToolExecutionResult | undefined {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item && item.type === "tool_result" && item.toolName === toolName) {
      return item.result;
    }
  }
  return undefined;
}

function readResultSha256(result: ToolExecutionResult): string | null {
  if (result.status !== "success") {
    return null;
  }
  if (typeof result.output !== "object" || result.output === null || Array.isArray(result.output)) {
    return null;
  }
  const record = result.output as JsonObject;
  const sha256 = record["sha256"];
  return typeof sha256 === "string" && sha256.length === 64 ? sha256 : null;
}

async function* streamTextChunks(
  text: string,
  signal: AbortSignal | undefined,
): AsyncIterable<ModelEvent> {
  for (const chunk of chunkText(text, CHUNK_SIZE)) {
    if (signal?.aborted) {
      throw createAbortError();
    }
    yield { type: "text_delta", text: chunk };
    await Promise.resolve();
  }
  if (signal?.aborted) {
    throw createAbortError();
  }
  yield { type: "completed" };
}

function findScenario(messages: readonly ConversationItem[]): Scenario | null {
  const latestUserPrompt = findLatestUserPrompt(messages);
  if (latestUserPrompt === "list files") {
    return { kind: "list", toolName: "workspace.list", input: { path: "." } };
  }
  if (latestUserPrompt === "read README.md") {
    return { kind: "read", toolName: "workspace.read", input: { path: "README.md" } };
  }
  if (latestUserPrompt.startsWith("search ")) {
    const query = latestUserPrompt.slice("search ".length).trim();
    if (query.length > 0) {
      return {
        kind: "search",
        toolName: "workspace.search",
        input: { query, path: "." },
      };
    }
  }
  return null;
}

function isToolAvailable(tools: readonly ToolDefinition[], toolName: string): boolean {
  return tools.some((tool) => tool.name === toolName);
}

function findResultForCall(
  messages: readonly ConversationItem[],
  callId: string,
): ToolExecutionResult | undefined {
  const firstItemOfCurrentTurn = findLatestUserMessageIndex(messages) + 1;
  for (let index = messages.length - 1; index >= firstItemOfCurrentTurn; index -= 1) {
    const item = messages[index];
    if (item && item.type === "tool_result" && item.callId === callId) {
      return item.result;
    }
  }
  return undefined;
}

function findLatestUserMessageIndex(messages: readonly ConversationItem[]): number {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const item = messages[index];
    if (item && item.type === "user_message") {
      return index;
    }
  }
  return -1;
}

function formatScenarioResponse(
  scenario: Scenario,
  result: ToolExecutionResult | undefined,
): string {
  if (result === undefined) {
    return "Solaris has no tool result available.";
  }
  if (result.status !== "success") {
    return `Solaris could not complete the workspace operation: ${result.message}`;
  }
  switch (scenario.kind) {
    case "list": {
      const count = countArrayField(result.output, "entries");
      return count === null
        ? "Solaris inspected the workspace entries."
        : `Solaris inspected ${count} workspace entries.`;
    }
    case "read":
      return `Solaris read ${scenario.input.path}.`;
    case "search": {
      const count = countArrayField(result.output, "matches");
      return count === null
        ? "Solaris searched the workspace."
        : `Solaris found ${count} matching lines.`;
    }
  }
}

function countArrayField(output: JsonValue, key: string): number | null {
  if (typeof output !== "object" || output === null || Array.isArray(output)) {
    return null;
  }
  const record = output as JsonObject;
  const value = record[key];
  return Array.isArray(value) ? value.length : null;
}

function formatResponse(messages: readonly ConversationItem[]): string {
  return `Solaris received: ${findLatestUserPrompt(messages)}`;
}

function findLatestUserPrompt(messages: readonly ConversationItem[]): string {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const item = messages[index];
    if (item && item.type === "user_message") {
      return item.content;
    }
  }
  return "";
}

function chunkText(text: string, size: number): readonly string[] {
  const chunks: string[] = [];
  for (let index = 0; index < text.length; index += size) {
    chunks.push(text.slice(index, index + size));
  }
  return chunks;
}

function createAbortError(): Error {
  return new DOMException("The fake provider was aborted.", "AbortError");
}
