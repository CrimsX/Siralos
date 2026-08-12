import type {
  ConversationItem,
  JsonObject,
  JsonValue,
  ModelEvent,
  ModelProvider,
  ModelRequest,
  ToolDefinition,
  ToolExecutionResult,
} from "@siralos/core";

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
    return `Siralos could not inspect Git: ${result.message}`;
  }
  const record = result.output as JsonObject;
  if (scenario === "git-status") {
    const changes = Array.isArray(record["changes"]) ? record["changes"] : [];
    const untracked = Array.isArray(record["untracked"]) ? record["untracked"] : [];
    return `Siralos found ${changes.length} modified files and ${untracked.length} untracked file${untracked.length === 1 ? "" : "s"}.`;
  }
  const files = Array.isArray(record["files"]) ? record["files"] : [];
  return `Siralos inspected a ${scenario === "diff-working" ? "working" : scenario === "diff-staged" ? "staged" : "HEAD"} diff of ${files.length} file${files.length === 1 ? "" : "s"}.`;
}

type GodotScenario =
  | "inspect-engine"
  | "inspect-project"
  | "probe-project"
  | "api-search"
  | "check-script"
  | "check-project-scripts"
  | "lsp-session"
  | "lsp-hover"
  | "lsp-complete"
  | "lsp-definition"
  | "lsp-diagnostics";

function findGodotScenario(messages: readonly ConversationItem[]): GodotScenario | null {
  const latestUserPrompt = findLatestUserPrompt(messages);
  if (latestUserPrompt === "inspect godot" || latestUserPrompt === "inspect godot engine") {
    return "inspect-engine";
  }
  if (latestUserPrompt === "inspect godot project") {
    return "inspect-project";
  }
  if (latestUserPrompt === "is this project compatible with godot") {
    return "inspect-project";
  }
  if (
    latestUserPrompt === "probe godot project" ||
    latestUserPrompt === "run godot project probe"
  ) {
    return "probe-project";
  }
  if (latestUserPrompt.startsWith("search godot api")) {
    return "api-search";
  }
  if (latestUserPrompt === "check godot script") {
    return "check-script";
  }
  if (latestUserPrompt === "check godot project scripts") {
    return "check-project-scripts";
  }
  if (latestUserPrompt === "start godot language session") {
    return "lsp-session";
  }
  if (latestUserPrompt === "hover godot script") {
    return "lsp-hover";
  }
  if (latestUserPrompt === "complete godot script") {
    return "lsp-complete";
  }
  if (latestUserPrompt === "definition godot script") {
    return "lsp-definition";
  }
  if (latestUserPrompt === "diagnose godot script via lsp") {
    return "lsp-diagnostics";
  }
  return null;
}

function godotScenarioTool(scenario: GodotScenario): string {
  switch (scenario) {
    case "inspect-engine":
      return "godot.inspect_engine";
    case "inspect-project":
      return "godot.inspect_project";
    case "probe-project":
      return "godot.probe_project";
    case "api-search":
      return "godot.api_search";
    case "check-script":
      return "godot.check_script";
    case "check-project-scripts":
      return "godot.check_project_scripts";
    case "lsp-session":
      return "godot.lsp_session";
    case "lsp-hover":
      return "godot.hover";
    case "lsp-complete":
      return "godot.complete";
    case "lsp-definition":
      return "godot.definition";
    case "lsp-diagnostics":
      return "godot.lsp_diagnostics";
  }
}

function godotScenarioInput(scenario: GodotScenario, prompt: string): unknown {
  switch (scenario) {
    case "api-search": {
      const rest = prompt.slice("search godot api".length).trim();
      return { query: rest.length > 0 ? rest : "Node owner" };
    }
    case "check-script":
      return { path: "src/player/player.gd" };
    case "check-project-scripts":
      return {};
    case "lsp-session":
      return {};
    case "lsp-hover":
    case "lsp-complete":
    case "lsp-definition":
      return { path: "src/player/player.gd", line: 10, column: 5 };
    case "lsp-diagnostics":
      return { path: "src/player/player.gd" };
    case "inspect-engine":
    case "inspect-project":
    case "probe-project":
      return {};
  }
}

function formatGodotFinalText(scenario: GodotScenario, result: ToolExecutionResult): string {
  if (scenario === "probe-project") {
    return formatProbeFinalText(result);
  }
  if (scenario === "lsp-session") {
    return formatLSPSessionFinalText(result);
  }
  if (
    scenario === "lsp-hover" ||
    scenario === "lsp-complete" ||
    scenario === "lsp-definition" ||
    scenario === "lsp-diagnostics"
  ) {
    return formatLSPQueryFinalText(result);
  }
  if (scenario === "check-script" || scenario === "check-project-scripts") {
    return formatCheckFinalText(result);
  }
  if (scenario === "api-search") {
    return formatApiSearchFinalText(result);
  }
  if (result.status !== "success") {
    return `Siralos could not complete the Godot inspection: ${result.message}`;
  }
  const record = result.output as JsonObject;
  if (scenario === "inspect-engine") {
    if (record["selected"] === false) {
      return "No Godot installation is selected, so Siralos could not profile an engine.";
    }
    const version = typeof record["version"] === "string" ? record["version"] : "unknown";
    const edition = typeof record["edition"] === "string" ? record["edition"] : "unknown";
    const support = typeof record["support"] === "string" ? record["support"] : "unknown";
    const verified = Array.isArray(record["verifiedCapabilities"])
      ? (record["verifiedCapabilities"] as readonly unknown[]).length
      : 0;
    return `Siralos inspected the selected Godot installation: ${version} (${edition}, ${support}) with ${verified} operationally verified capabilities.`;
  }
  const detected = record["detected"] === true;
  const name = typeof record["name"] === "string" ? record["name"] : null;
  const statusRecord =
    typeof record["compatibility"] === "object" && record["compatibility"] !== null
      ? (record["compatibility"] as JsonObject)
      : null;
  const status =
    statusRecord !== null && typeof statusRecord["status"] === "string"
      ? statusRecord["status"]
      : "unknown";
  if (!detected) {
    return "Siralos found no Godot project at the workspace root; the static inspection reported nothing to assess.";
  }
  const namePart = name === null ? "an unnamed project" : `the project ${name}`;
  return `Siralos statically inspected ${namePart}: the assessment is ${status}. No project code was executed and no import was performed.`;
}

async function* streamGodotScenario(
  scenario: GodotScenario,
  request: ModelRequest,
  signal: AbortSignal | undefined,
): AsyncIterable<ModelEvent> {
  const toolName = godotScenarioTool(scenario);
  const result = findLatestResult(itemsAfterLastUserMessage(request.messages), toolName);
  if (result === undefined) {
    if (isToolAvailable(request.tools, toolName)) {
      const input = godotScenarioInput(scenario, findLatestUserPrompt(request.messages));
      yield { type: "tool_call", callId: "call-godot", toolName, input };
      await Promise.resolve();
      yield { type: "completed" };
      return;
    }
    if (scenario === "probe-project") {
      yield* streamTextChunks(
        "Siralos cannot probe the Godot project in this profile (godot.probe_project is unavailable).",
        signal,
      );
      return;
    }
    yield* streamTextChunks(
      "Siralos cannot inspect Godot in this profile (Godot inspection tools are unavailable).",
      signal,
    );
    return;
  }
  yield* streamTextChunks(formatGodotFinalText(scenario, result), signal);
}

function formatLSPSessionFinalText(result: ToolExecutionResult): string {
  switch (result.status) {
    case "success": {
      const record = result.output as JsonObject;
      const sessionId = typeof record["sessionId"] === "string" ? record["sessionId"] : "unknown";
      return `Siralos started a bounded Godot GDScript language session (${sessionId}): a headless recovery editor serves the disposable mirror over loopback-only LSP. Source writes and LSP mutations are disabled; the session expires automatically.`;
    }
    case "denied":
      return `The Godot language session was not approved, so Siralos did not start it.`;
    case "conflict":
      return `The project or engine changed after approval, so Siralos did not start the language session. Approve the session again.`;
    case "unavailable":
    case "failed":
    case "cancelled":
    case "timed_out":
    case "invalid_input":
    case "output_limit":
    case "sandbox_denied":
    case "sandbox_unavailable":
    case "workspace_violation":
      return `Siralos could not start the Godot language session: ${result.message}`;
  }
}

function formatLSPQueryFinalText(result: ToolExecutionResult): string {
  if (result.status !== "success") {
    if (
      result.status === "failed" &&
      typeof result.message === "string" &&
      result.message.includes("No Godot language session is active")
    ) {
      return `No Godot language session is active; start and approve one with godot.lsp_session first.`;
    }
    return `Siralos could not complete the language query: ${result.message}`;
  }
  const record = result.output as JsonObject;
  if (Array.isArray(record["diagnostics"])) {
    return `Siralos received ${(record["diagnostics"] as readonly unknown[]).length} normalized diagnostics from the language session.`;
  }
  if (Array.isArray(record["items"])) {
    return `Siralos received ${(record["items"] as readonly unknown[]).length} bounded completion candidates (never applied).`;
  }
  if (Array.isArray(record["locations"])) {
    return `Siralos resolved ${(record["locations"] as readonly unknown[]).length} definition location(s).`;
  }
  if (typeof record["contents"] === "object" && record["contents"] !== null) {
    return `Siralos returned bounded hover information from the language session.`;
  }
  return `Siralos completed the language query.`;
}

function formatApiSearchFinalText(result: ToolExecutionResult): string {
  if (result.status !== "success") {
    if (result.status === "unavailable") {
      return `Siralos cannot search the Godot API right now: ${result.message}`;
    }
    return `Siralos could not search the Godot API: ${result.message}`;
  }
  const record = result.output as JsonObject;
  const results = Array.isArray(record["results"]) ? (record["results"] as readonly unknown[]) : [];
  const version = typeof record["engineVersion"] === "string" ? record["engineVersion"] : "unknown";
  const truncated = record["truncated"] === true;
  const names = results
    .slice(0, 3)
    .map((entry) => {
      const item = entry as JsonObject;
      const owner = typeof item["owner"] === "string" ? item["owner"] : null;
      const name = typeof item["name"] === "string" ? item["name"] : "?";
      return owner === null ? name : `${owner}.${name}`;
    })
    .join(", ");
  const suffix = truncated ? " (results truncated)" : "";
  return `Siralos found ${results.length} API result${results.length === 1 ? "" : "s"} for the selected Godot ${version}: ${names}.${suffix}`;
}

function formatCheckFinalText(result: ToolExecutionResult): string {
  switch (result.status) {
    case "success": {
      const record = result.output as JsonObject;
      const valid = record["valid"] === true;
      const invalidCount = typeof record["invalidCount"] === "number" ? record["invalidCount"] : 0;
      const diagnostics = Array.isArray(record["diagnostics"])
        ? (record["diagnostics"] as readonly unknown[])
        : [];
      if (valid) {
        return "Siralos checked the GDScript with the selected engine's parser (--check-only): it is valid. No game code was executed.";
      }
      return `Siralos checked the GDScript with the selected engine's parser (--check-only): it has ${invalidCount} invalid script${invalidCount === 1 ? "" : "s"} and ${diagnostics.length} normalized diagnostic${diagnostics.length === 1 ? "" : "s"}. No game code was executed.`;
    }
    case "denied":
      return `The GDScript check was not approved, so Siralos did not run it.`;
    case "conflict":
      return `The project, engine, or script changed after approval, so Siralos did not run the check. Approve the check again.`;
    case "cancelled":
      return `The GDScript check was cancelled before completion.`;
    case "timed_out":
      return `The GDScript check timed out and the engine process tree was terminated.`;
    case "unavailable":
    case "sandbox_denied":
    case "sandbox_unavailable":
      return `Siralos could not run the GDScript check: ${result.message}`;
    case "invalid_input":
    case "failed":
    case "workspace_violation":
    case "output_limit":
      return `Siralos could not complete the GDScript check: ${result.message}`;
  }
}

function formatProbeFinalText(result: ToolExecutionResult): string {
  switch (result.status) {
    case "success": {
      const record = result.output as JsonObject;
      const status = typeof record["status"] === "string" ? record["status"] : "unknown";
      const diagnostics =
        typeof record["diagnostics"] === "object" && record["diagnostics"] !== null
          ? (record["diagnostics"] as JsonObject)
          : null;
      const errors =
        diagnostics !== null && Array.isArray(diagnostics["errors"])
          ? (diagnostics["errors"] as readonly unknown[]).length
          : 0;
      const warnings =
        diagnostics !== null && Array.isArray(diagnostics["warnings"])
          ? (diagnostics["warnings"] as readonly unknown[]).length
          : 0;
      const integrity =
        typeof record["workspaceIntegrity"] === "object" && record["workspaceIntegrity"] !== null
          ? (record["workspaceIntegrity"] as JsonObject)
          : null;
      const unchanged = integrity?.["unchanged"] === true;
      const cleanup =
        typeof record["cleanup"] === "object" && record["cleanup"] !== null
          ? (record["cleanup"] as JsonObject)
          : null;
      const cleaned = cleanup?.["completed"] === true;
      const engine =
        typeof record["engine"] === "object" && record["engine"] !== null
          ? (record["engine"] as JsonObject)
          : null;
      const version = typeof engine?.["version"] === "string" ? engine["version"] : "unknown";
      return `Siralos ran a recovery-mode Godot project probe with ${version}: ${status} with ${errors} error${errors === 1 ? "" : "s"} and ${warnings} warning${warnings === 1 ? "" : "s"}. Recovery mode was used, the source workspace was not loaded${unchanged ? " and was unchanged" : ""}, and the disposable mirror was ${cleaned ? "removed" : "not removed"}.`;
    }
    case "denied":
      return `The Godot project probe was not approved, so Siralos did not run it.`;
    case "conflict":
      return `The project or engine changed after approval, so Siralos did not run the probe. Approve the probe again.`;
    case "cancelled":
      return `The Godot project probe was cancelled before completion.`;
    case "timed_out":
      return `The Godot project probe timed out and the engine process tree was terminated.`;
    case "sandbox_denied":
    case "sandbox_unavailable":
      return `The sandbox could not enforce the probe boundaries, so the probe did not run.`;
    case "workspace_violation":
      return `Siralos detected unexpected source workspace changes during the probe; nothing was reverted.`;
    case "output_limit":
      return `The Godot project probe exceeded its output limit and was terminated.`;
    case "invalid_input":
    case "unavailable":
    case "failed":
      return `Siralos could not probe the Godot project: ${result.message}`;
  }
}

async function* stream(request: ModelRequest): AsyncIterable<ModelEvent> {
  const signal = request.signal;
  if (signal?.aborted) {
    throw createAbortError();
  }
  const planningScenario = findPlanningPrompt(request.messages);
  if (planningScenario) {
    yield* streamPlanningScenario(request, signal);
    return;
  }
  const developScenario = findDevelopScenario(request.messages);
  if (developScenario !== null) {
    yield* streamDevelopScenario(developScenario, request, signal);
    return;
  }
  const godotScenario = findGodotScenario(request.messages);
  if (godotScenario !== null) {
    yield* streamGodotScenario(godotScenario, request, signal);
    return;
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
      yield { type: "completed" };
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
      yield { type: "completed" };
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
        yield { type: "completed" };
        return;
      }
      yield* streamTextChunks(formatCommandFinalText(commandScenario, result), signal);
      return;
    }
    yield* streamTextChunks(
      `Siralos cannot run development commands in this profile (process.run is unavailable).`,
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
        yield { type: "completed" };
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

async function* streamPlanningScenario(
  request: ModelRequest,
  signal: AbortSignal | undefined,
): AsyncIterable<ModelEvent> {
  const result = findLatestResult(itemsAfterLastUserMessage(request.messages), "workspace.list");
  if (result === undefined && isToolAvailable(request.tools, "workspace.list")) {
    yield {
      type: "tool_call",
      callId: "call-plan-1",
      toolName: "workspace.list",
      input: { path: "." },
    };
    await Promise.resolve();
    yield { type: "completed" };
    return;
  }
  const prompt = findLatestUserPrompt(request.messages);
  const depth = /This is a LIGHT plan/.test(prompt) ? "light" : "full";
  const objective = extractPlanningRequest(prompt);
  const plan = {
    depth,
    objective: `Structured plan for: ${objective}`,
    scope: { inScope: ["the requested workspace change"], outOfScope: [] },
    nonGoals: [],
    touchpoints: [
      { id: "t1", path: "src/player/player.gd", confidence: "candidate" },
      { id: "t2", path: "tests/player/**", confidence: "candidate" },
    ],
    constraints: [],
    risks:
      depth === "full"
        ? [{ id: "r1", severity: "low", description: "Bounded change; risk is minimal." }]
        : [],
    steps: [
      {
        id: "step-1",
        title: "Inspect the relevant files",
        expectedTouchpoints: ["t1"],
      },
      { id: "step-2", title: "Implement the bounded change", expectedTouchpoints: ["t1", "t2"] },
      {
        id: "step-3",
        title: "Validate with check-only parsing and existing tests",
        expectedTouchpoints: [],
      },
    ],
    validation: {
      checks: ["check-only parse", "existing project tests"],
      requirements: ["workspace mutation"],
    },
    ...(depth === "full" ? { rollback: { description: "Revert the prepared change set." } } : {}),
  };
  yield* streamTextChunks(JSON.stringify(plan, null, 2), signal);
}

/** Detects the host-authored planner prompt (fresh planner context). */
function findPlanningPrompt(messages: readonly ConversationItem[]): boolean {
  return messages.some(
    (message) =>
      message.type === "user_message" && message.content.startsWith("You are the Siralos planner."),
  );
}

function extractPlanningRequest(prompt: string): string {
  const marker = "Task request:";
  const index = prompt.indexOf(marker);
  if (index < 0) {
    return "the requested task";
  }
  const rest = prompt.slice(index + marker.length).trim();
  const nextSection = rest.indexOf("\nAcceptance criteria:");
  return (nextSection < 0 ? rest : rest.slice(0, nextSection)).trim().slice(0, 120);
}

type WriteScenario = "create" | "edit" | "delete";

const WRITE_TEST_FILE = "siralos-write-test.txt";
const WRITE_TEST_CONTENT = "Created by the deterministic Siralos test provider.\n";

function findWriteScenario(messages: readonly ConversationItem[]): WriteScenario | null {
  const latestUserPrompt = findLatestUserPrompt(messages);
  if (latestUserPrompt === "create siralos-write-test") {
    return "create";
  }
  if (latestUserPrompt === "edit siralos-write-test") {
    return "edit";
  }
  if (latestUserPrompt === "delete siralos-write-test") {
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
            text: `Siralos could not read ${WRITE_TEST_FILE}, so it did not modify it.`,
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
            text: `Siralos could not read ${WRITE_TEST_FILE}, so it did not delete it.`,
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
      return `Siralos ${operation === "create" ? "created" : operation === "edit" ? "updated" : "deleted"} ${WRITE_TEST_FILE}.`;
    case "denied":
      return `The workspace change was denied, so Siralos did not ${verb} ${WRITE_TEST_FILE}.`;
    case "conflict":
      return `The file changed, so Siralos did not ${verb} ${WRITE_TEST_FILE}. Reread the file to continue.`;
    case "cancelled":
      return `The workspace change was cancelled, so Siralos did not ${verb} ${WRITE_TEST_FILE}.`;
    case "invalid_input":
    case "failed":
    case "unavailable":
    case "timed_out":
    case "output_limit":
    case "sandbox_denied":
    case "sandbox_unavailable":
    case "workspace_violation":
      return `Siralos could not ${verb} ${WRITE_TEST_FILE}: ${result.message}`;
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
      const exitCodeValue = readExitCode(result.output);
      if (exitCodeValue !== null && exitCodeValue !== 0) {
        return `Siralos ran \`${display}\`, but it exited with code ${exitCodeValue}.`;
      }
      return `Siralos ran \`${display}\` and it exited with code 0.`;
    }
    case "denied":
      return `The command was not approved, so Siralos did not run it.`;
    case "conflict":
      return `The command plan changed, so Siralos did not run it. Request the command again.`;
    case "cancelled":
      return `The command was cancelled before it completed.`;
    case "timed_out":
      return `The command timed out and its process tree was terminated.`;
    case "sandbox_denied":
      return `The sandbox denied part of the command: ${result.message}`;
    case "sandbox_unavailable":
      return `The sandbox is unavailable, so the command did not run.`;
    case "workspace_violation":
      return `Siralos detected unexpected workspace changes; command execution is disabled for this session.`;
    case "output_limit":
      return `The command exceeded its output limit and was terminated.`;
    case "unavailable":
    case "invalid_input":
    case "failed":
      return `Siralos could not run the command: ${result.message}`;
  }
}

function readExitCode(output: JsonValue): number | null {
  if (typeof output !== "object" || output === null || Array.isArray(output)) {
    return null;
  }
  const value = (output as JsonObject)["exitCode"];
  return typeof value === "number" ? value : null;
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
    return "Siralos has no tool result available.";
  }
  if (result.status !== "success") {
    return `Siralos could not complete the workspace operation: ${result.message}`;
  }
  switch (scenario.kind) {
    case "list": {
      const count = countArrayField(result.output, "entries");
      return count === null
        ? "Siralos inspected the workspace entries."
        : `Siralos inspected ${count} workspace entries.`;
    }
    case "read":
      return `Siralos read ${scenario.input.path}.`;
    case "search": {
      const count = countArrayField(result.output, "matches");
      return count === null
        ? "Siralos searched the workspace."
        : `Siralos found ${count} matching lines.`;
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

type DevelopScenario = "fixture" | "fixture-repair" | "fixture-review" | "fixture-review-exhaust";

const DEV_FIXTURE_PATH = "scripts/player/player.gd";
const DEV_FIXTURE_OLD = "move_and_slide()";
const DEV_FIXTURE_NEW = "move_and_slide(Vector2.UP)";
const DEV_FIXTURE_BROKEN = "move_and_slide())";
const DEV_FIXTURE_REVIEW_REPAIR_1 = "move_and_slide(Vector2.UP) # review-repair";
const DEV_FIXTURE_REVIEW_REPAIR_2 = "move_and_slide(Vector2.UP) # review-repair-2";

function findDevelopScenario(messages: readonly ConversationItem[]): DevelopScenario | null {
  const latestUserPrompt = findLatestUserPrompt(messages);
  if (latestUserPrompt === "develop fixture") {
    return "fixture";
  }
  if (latestUserPrompt === "develop fixture with repair") {
    return "fixture-repair";
  }
  if (latestUserPrompt === "develop fixture with review repair") {
    return "fixture-review";
  }
  if (latestUserPrompt === "develop fixture with review repair exhaust") {
    return "fixture-review-exhaust";
  }
  return null;
}

async function* streamDevelopScenario(
  scenario: DevelopScenario,
  request: ModelRequest,
  signal: AbortSignal | undefined,
): AsyncIterable<ModelEvent> {
  const stepItems = itemsAfterLastUserMessage(request.messages);
  const changesetTool = "workspace.apply_text_changeset";
  const firstResult = findLatestResult(stepItems, changesetTool);
  const repairResult = findResultForCall(request.messages, "call-dev-repair");
  const secondRepairResult = findResultForCall(request.messages, "call-dev-repair-2");
  const readResult = findLatestResult(stepItems, "workspace.read");
  if (!isToolAvailable(request.tools, changesetTool)) {
    yield* streamTextChunks(
      "Siralos cannot propose source changes in this profile (workspace.apply_text_changeset is unavailable).",
      signal,
    );
    return;
  }
  if (scenario === "fixture") {
    if (firstResult === undefined) {
      if (readResult === undefined) {
        yield {
          type: "tool_call",
          callId: "call-dev-read",
          toolName: "workspace.read",
          input: { path: DEV_FIXTURE_PATH },
        };
        await Promise.resolve();
        yield { type: "completed" };
        return;
      }
      const hash = readResultSha256(readResult);
      if (hash === null) {
        yield* streamTextChunks(
          `Siralos could not read ${DEV_FIXTURE_PATH}, so it did not propose a change.`,
          signal,
        );
        return;
      }
      yield {
        type: "tool_call",
        callId: "call-dev-change",
        toolName: changesetTool,
        input: {
          changes: [
            {
              operation: "edit",
              path: DEV_FIXTURE_PATH,
              expectedSha256: hash,
              replacements: [{ oldText: DEV_FIXTURE_OLD, newText: DEV_FIXTURE_NEW }],
            },
          ],
        },
      };
      await Promise.resolve();
      yield { type: "completed" };
      return;
    }
    yield* streamTextChunks(formatDevelopmentFinalText(firstResult), signal);
    return;
  }
  if (scenario === "fixture-repair") {
    // the first edit is deliberately broken; the repair fixes it after
    // the parser diagnostic arrives.
    if (repairResult === undefined) {
      if (firstResult === undefined) {
        if (readResult === undefined) {
          yield {
            type: "tool_call",
            callId: "call-dev-read",
            toolName: "workspace.read",
            input: { path: DEV_FIXTURE_PATH },
          };
          await Promise.resolve();
          yield { type: "completed" };
          return;
        }
        const hash = readResultSha256(readResult);
        if (hash === null) {
          yield* streamTextChunks(
            `Siralos could not read ${DEV_FIXTURE_PATH}, so it did not propose a change.`,
            signal,
          );
          return;
        }
        yield {
          type: "tool_call",
          callId: "call-dev-change",
          toolName: changesetTool,
          input: {
            changes: [
              {
                operation: "edit",
                path: DEV_FIXTURE_PATH,
                expectedSha256: hash,
                replacements: [{ oldText: DEV_FIXTURE_OLD, newText: DEV_FIXTURE_BROKEN }],
              },
            ],
          },
        };
        await Promise.resolve();
        yield { type: "completed" };
        return;
      }
      const currentHash = firstResultSha256(firstResult);
      if (currentHash === null) {
        yield* streamTextChunks(
          "Siralos could not determine the applied fixture content for the repair, so it did not propose one.",
          signal,
        );
        return;
      }
      yield {
        type: "tool_call",
        callId: "call-dev-repair",
        toolName: changesetTool,
        input: {
          changes: [
            {
              operation: "edit",
              path: DEV_FIXTURE_PATH,
              expectedSha256: currentHash,
              replacements: [{ oldText: DEV_FIXTURE_BROKEN, newText: DEV_FIXTURE_NEW }],
            },
          ],
        },
      };
      await Promise.resolve();
      yield { type: "completed" };
      return;
    }
    yield* streamTextChunks(formatDevelopmentFinalText(repairResult), signal);
    return;
  }
  // Review-repair scenarios: the first apply triggers blocking review
  // findings; the provider proposes a focused repair, and the workflow
  // revalidates and re-reviews after it.
  const reviewBlocking = (result: ToolExecutionResult): boolean => {
    if (result.status !== "success") {
      return false;
    }
    if (
      typeof result.output !== "object" ||
      result.output === null ||
      Array.isArray(result.output)
    ) {
      return false;
    }
    const quality = (result.output as JsonObject)["quality"];
    if (typeof quality !== "object" || quality === null || Array.isArray(quality)) {
      return false;
    }
    return (quality as JsonObject)["status"] === "blocking_findings";
  };
  if (firstResult === undefined) {
    if (readResult === undefined) {
      yield {
        type: "tool_call",
        callId: "call-dev-read",
        toolName: "workspace.read",
        input: { path: DEV_FIXTURE_PATH },
      };
      await Promise.resolve();
      yield { type: "completed" };
      return;
    }
    const hash = readResultSha256(readResult);
    if (hash === null) {
      yield* streamTextChunks(
        `Siralos could not read ${DEV_FIXTURE_PATH}, so it did not propose a change.`,
        signal,
      );
      return;
    }
    yield {
      type: "tool_call",
      callId: "call-dev-change",
      toolName: changesetTool,
      input: {
        changes: [
          {
            operation: "edit",
            path: DEV_FIXTURE_PATH,
            expectedSha256: hash,
            replacements: [{ oldText: DEV_FIXTURE_OLD, newText: DEV_FIXTURE_NEW }],
          },
        ],
      },
    };
    await Promise.resolve();
    yield { type: "completed" };
    return;
  }
  if (repairResult === undefined) {
    if (!reviewBlocking(firstResult)) {
      yield* streamTextChunks(formatDevelopmentFinalText(firstResult), signal);
      return;
    }
    const currentHash = firstResultSha256(firstResult);
    if (currentHash === null) {
      yield* streamTextChunks(
        "Siralos could not determine the applied fixture content for the review repair, so it did not propose one.",
        signal,
      );
      return;
    }
    yield {
      type: "tool_call",
      callId: "call-dev-repair",
      toolName: changesetTool,
      input: {
        changes: [
          {
            operation: "edit",
            path: DEV_FIXTURE_PATH,
            expectedSha256: currentHash,
            replacements: [{ oldText: DEV_FIXTURE_NEW, newText: DEV_FIXTURE_REVIEW_REPAIR_1 }],
          },
        ],
      },
    };
    await Promise.resolve();
    yield { type: "completed" };
    return;
  }
  if (scenario === "fixture-review-exhaust" && secondRepairResult === undefined) {
    if (!reviewBlocking(repairResult)) {
      yield* streamTextChunks(formatDevelopmentFinalText(repairResult), signal);
      return;
    }
    const currentHash = firstResultSha256(repairResult);
    if (currentHash === null) {
      yield* streamTextChunks(
        "Siralos could not determine the repaired fixture content for the second review repair, so it did not propose one.",
        signal,
      );
      return;
    }
    yield {
      type: "tool_call",
      callId: "call-dev-repair-2",
      toolName: changesetTool,
      input: {
        changes: [
          {
            operation: "edit",
            path: DEV_FIXTURE_PATH,
            expectedSha256: currentHash,
            replacements: [
              { oldText: DEV_FIXTURE_REVIEW_REPAIR_1, newText: DEV_FIXTURE_REVIEW_REPAIR_2 },
            ],
          },
        ],
      },
    };
    await Promise.resolve();
    yield { type: "completed" };
    return;
  }
  yield* streamTextChunks(formatDevelopmentFinalText(secondRepairResult ?? repairResult), signal);
}

function firstResultSha256(result: ToolExecutionResult): string | null {
  if (result.status !== "success") {
    return null;
  }
  if (typeof result.output !== "object" || result.output === null || Array.isArray(result.output)) {
    return null;
  }
  const record = result.output as JsonObject;
  const changedFiles = record["changedFiles"];
  if (!Array.isArray(changedFiles) || changedFiles.length === 0) {
    return null;
  }
  // The change records are cumulative across the workflow; the latest
  // record describes the most recent change-set application.
  const file = changedFiles[changedFiles.length - 1] as JsonObject;
  const sha = file["afterSha256"];
  return typeof sha === "string" && sha.length === 64 ? sha : null;
}

function formatDevelopmentFinalText(result: ToolExecutionResult): string {
  switch (result.status) {
    case "success": {
      const record = result.output as JsonObject;
      const changed = Array.isArray(record["changedFiles"])
        ? (record["changedFiles"] as readonly unknown[])
        : [];
      const diagnostics =
        typeof record["diagnostics"] === "object" && record["diagnostics"] !== null
          ? (record["diagnostics"] as JsonObject)
          : null;
      const errors = typeof diagnostics?.["errors"] === "number" ? diagnostics["errors"] : 0;
      const warnings = typeof diagnostics?.["warnings"] === "number" ? diagnostics["warnings"] : 0;
      const files = changed.map((entry) => (entry as JsonObject)["path"] as string).join(", ");
      const validation =
        typeof record["validation"] === "object" && record["validation"] !== null
          ? (record["validation"] as JsonObject)
          : null;
      const parser = validation?.["parser"] === true;
      const lsp = validation?.["lsp"] === true;
      const quality =
        typeof record["quality"] === "object" && record["quality"] !== null
          ? (record["quality"] as JsonObject)
          : null;
      const qualityStatus = typeof quality?.["status"] === "string" ? quality["status"] : null;
      const qualityPart =
        qualityStatus === null
          ? ""
          : ` Quality: ${qualityStatus}${qualityStatus === "blocking_findings" ? " (blocking findings remain; a focused repair may be proposed)" : qualityStatus === "validation_incomplete" ? " (validation incomplete; approved changes remain)" : ""}.`;
      return `Siralos applied the approved change set to ${files}: parser ${parser ? "passed" : "failed"}, fresh language session ${lsp ? "started" : "failed"}, ${errors} error(s), ${warnings} warning(s).${qualityPart}`;
    }
    case "denied":
      return "The source change was not approved, so Siralos did not apply it.";
    case "conflict":
      return "The workspace changed, so Siralos did not apply the change set. Reread the files to continue.";
    case "cancelled":
      return "The development workflow was cancelled; approved changes (if any) remain.";
    case "unavailable":
      return `Siralos cannot apply source changes yet: ${result.message}`;
    case "invalid_input":
    case "failed":
      return `Siralos could not apply the change set: ${result.message}`;
    case "timed_out":
    case "output_limit":
    case "sandbox_denied":
    case "sandbox_unavailable":
    case "workspace_violation":
      return `Siralos could not complete the development change: ${result.message}`;
  }
}

function formatResponse(messages: readonly ConversationItem[]): string {
  return `Siralos received: ${findLatestUserPrompt(messages)}`;
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
  let current = "";
  let count = 0;
  for (const character of text) {
    current += character;
    count += 1;
    if (count >= size) {
      chunks.push(current);
      current = "";
      count = 0;
    }
  }
  if (current.length > 0) {
    chunks.push(current);
  }
  return chunks;
}

function createAbortError(): Error {
  return new DOMException("The fake provider was aborted.", "AbortError");
}
