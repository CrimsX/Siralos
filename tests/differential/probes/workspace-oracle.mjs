/**
 * Workspace-oracle probe (differential harness, ADR 0033, Stage 3R R4).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes workspace-read / workspace-list / workspace-search /
 * workspace-prepare / git-inspection scenarios against the REAL
 * TypeScript reference implementation (the adapter tools, the
 * revision registry, the mutation tools, and the Git adapter) and
 * prints the canonical R4 observation object as JSON on stdout.
 * This is a thin scenario adapter: it builds fixtures and maps
 * reference results to the canonical record vocabulary; it does not
 * reimplement workspace behavior.
 *
 * Deterministic: revision fingerprints come from the scenario input;
 * fixture content is declared or generated deterministically; no
 * ambient clock, randomness, or environment access enters records.
 */
import {
  mkdtempSync,
  mkdirSync,
  rmSync,
  symlinkSync,
  readFileSync,
  writeFileSync,
  realpathSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createWorkspaceReadTool } from "../../../packages/adapters/src/tools/workspace/workspace-read-tool.js";
import { createWorkspaceListTool } from "../../../packages/adapters/src/tools/workspace/workspace-list-tool.js";
import { createWorkspaceSearchTool } from "../../../packages/adapters/src/tools/workspace/workspace-search-tool.js";
import { createWorkspaceCreateFileTool } from "../../../packages/adapters/src/tools/workspace/mutations/workspace-create-file-tool.js";
import { createWorkspaceEditFileTool } from "../../../packages/adapters/src/tools/workspace/mutations/workspace-edit-file-tool.js";
import { createWorkspaceDeleteFileTool } from "../../../packages/adapters/src/tools/workspace/mutations/workspace-delete-file-tool.js";
import { createMutationLock } from "../../../packages/adapters/src/tools/workspace/mutations/mutation-lock.js";
import { createFilesystemCheckpointStore } from "../../../packages/adapters/src/checkpoints/filesystem/checkpoint-store.js";
import { createWorkspaceRevisionRegistry } from "../../../packages/core/src/workspace/workspace-revision.js";
import { createGitCliAdapter } from "../../../packages/adapters/src/git/cli/git-cli-adapter.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

function abortedSignal() {
  const controller = new AbortController();
  controller.abort();
  return controller.signal;
}

/**
 * Deterministic fixture content generation. Only declared inputs;
 * identical bytes are produced by the Rust candidate runner.
 */
function fixtureBytes(spec) {
  if (spec.content !== undefined) {
    return Buffer.from(spec.content, "utf8");
  }
  if (spec.bytes !== undefined) {
    return Buffer.from(spec.bytes);
  }
  if (spec.kind === "nul-after-probe") {
    return Buffer.concat([Buffer.alloc(9000, 0x61), Buffer.from([0]), Buffer.from("b", "utf8")]);
  }
  if (spec.kind === "crlf") {
    return Buffer.from("a\r\nb\r\n", "utf8");
  }
  if (spec.kind === "unicode") {
    return Buffer.from("héllo wörld\nsnowman ☃\nemoji 😀\n", "utf8");
  }
  if (spec.kind === "many-lines") {
    const lines = [];
    for (let index = 1; index <= 300; index += 1) {
      lines.push(`line ${index}`);
    }
    return Buffer.from(lines.join("\n"), "utf8");
  }
  if (spec.kind === "empty") {
    return Buffer.alloc(0);
  }
  if (spec.kind === "no-trailing-newline") {
    return Buffer.from("hello", "utf8");
  }
  if (spec.size !== undefined) {
    return Buffer.alloc(spec.size, spec.fill.charCodeAt(0));
  }
  throw new Error(`unsupported fixture spec: ${JSON.stringify(spec)}`);
}

/** Create the fixture workspace tree and return its canonical root. */
function createWorkspace(files, symlinks) {
  const root = mkdtempSync(join(tmpdir(), "siralos-oracle-ws-"));
  for (const spec of files ?? []) {
    if (spec.kind === "bulk") {
      const directory = join(root, spec.path);
      mkdirSync(directory, { recursive: true });
      for (let index = 0; index < spec.count; index += 1) {
        const name = `f${String(index).padStart(3, "0")}.txt`;
        writeFileSync(join(directory, name), spec.content, "utf8");
      }
      continue;
    }
    const target = join(root, spec.path);
    mkdirSync(join(target, ".."), { recursive: true });
    writeFileSync(target, fixtureBytes(spec));
  }
  for (const spec of symlinks ?? []) {
    try {
      // A `../` target points at a real file just outside the workspace,
      // so the containment rejection exercises the canonical escape path.
      const target = spec.target.startsWith("../")
        ? join(root, "..", spec.target.slice(3))
        : join(root, spec.target);
      if (spec.target.startsWith("../")) {
        writeFileSync(target, "outside secret\n", "utf8");
      }
      mkdirSync(join(root, spec.link, ".."), { recursive: true });
      symlinkSync(target, join(root, spec.link));
    } catch {
      // Symlink creation is a host privilege; both runners observe
      // the same per-host outcome and report it identically.
    }
  }
  return realpathSync(root);
}
/** Map a reference rejection/failure message to the stable R4 code. */
function readCodeFromMessage(message, status) {
  if (status === "invalid_input") return "invalid_input";
  if (message.includes("Path is empty.")) return "empty";
  if (message.includes("Path contains a null byte.")) return "null_byte";
  if (message.includes("Path must be relative to the workspace.")) return "absolute";
  if (message.includes("Path is outside the Siralos workspace.")) return "outside_workspace";
  if (message.includes("Path is inside the excluded directory")) return "excluded";
  if (message.includes("Path cannot be resolved")) return "unresolvable";
  if (message.includes("Cannot inspect file")) return "inspect_failed";
  if (message.includes("Target is not a regular file.")) return "not_file";
  if (message.includes("File is too large")) return "too_large";
  if (message.includes("Cannot read file")) return "unreadable";
  if (message.includes("File appears to be binary.")) return "binary";
  if (message.includes("File is not valid UTF-8 text.")) return "not_utf8";
  if (message.includes("beyond the end of the file")) return "start_beyond";
  return "inspect_failed";
}

function listCodeFromMessage(message) {
  if (message.includes("Path is empty.")) return "empty";
  if (message.includes("Path contains a null byte.")) return "null_byte";
  if (message.includes("Path must be relative to the workspace.")) return "absolute";
  if (message.includes("Path is outside the Siralos workspace.")) return "outside_workspace";
  if (message.includes("Path is inside the excluded directory")) return "excluded";
  if (message.includes("Path cannot be resolved")) return "unresolvable";
  if (message.includes("Target is not a directory.")) return "not_directory";
  if (message.includes("Cannot inspect directory")) return "inspect_failed";
  if (message.includes("Cannot list directory")) return "list_failed";
  if (message.includes("Cannot inspect entry")) return "entry_inspect_failed";
  return "list_failed";
}

function searchCodeFromMessage(message) {
  if (message.includes("Tool input must be a JSON object.")) return "not_an_object";
  if (message.includes('"query" is required.')) return "query_required";
  if (message.includes('"query" must be a string.')) return "query_not_string";
  if (message.includes('"path" must be a string.')) return "path_not_string";
  if (message.includes('"maxResults" must be a positive integer.')) return "max_results_invalid";
  if (message.includes("Path is empty.")) return "empty";
  if (message.includes("Path contains a null byte.")) return "null_byte";
  if (message.includes("Path must be relative to the workspace.")) return "absolute";
  if (message.includes("Path is outside the Siralos workspace.")) return "outside_workspace";
  if (message.includes("Path is inside the excluded directory")) return "excluded";
  if (message.includes("Path cannot be resolved")) return "unresolvable";
  return "query_required";
}

async function runReads(root, input) {
  const revisions = createWorkspaceRevisionRegistry({
    workspaceFingerprint: input.fingerprint,
  });
  const tool = createWorkspaceReadTool(root, { revisions });
  const reads = [];
  for (const request of input.reads ?? []) {
    const result = await tool.execute(request, {});
    if (result.status === "cancelled") {
      reads.push({ path: request.path, status: "cancelled" });
      continue;
    }
    if (result.status === "success") {
      const output = result.output;
      if (output.mode !== undefined) {
        reads.push({
          path: request.path,
          status: "success",
          mode: output.mode,
          revision: output.revision,
          supported: output.supported,
          reason: output.reason,
        });
      } else {
        reads.push({
          path: request.path,
          status: "success",
          sha256: output.sha256,
          revision: output.revision,
          content: output.content,
          startLine: output.startLine,
          endLine: output.endLine,
          totalLines: output.totalLines,
          truncated: output.truncated,
        });
      }
      continue;
    }
    if (result.status === "denied" || result.status === "failed") {
      reads.push({
        path: request.path,
        status: result.status,
        code: readCodeFromMessage(result.message, result.status),
      });
      continue;
    }
    reads.push({
      path: typeof request.path === "string" ? request.path : "",
      status: "invalid_input",
      code: "invalid_input",
    });
  }
  for (const request of input.cancelledReads ?? []) {
    const result = await tool.execute(request, { signal: abortedSignal() });
    reads.push({ path: request.path, status: result.status });
  }
  return { reads };
}

async function runLists(root, input) {
  const tool = createWorkspaceListTool(root);
  const lists = [];
  for (const request of input.lists ?? []) {
    const result = await tool.execute(request, {});
    if (result.status === "success") {
      const output = result.output;
      lists.push({
        path: request.path ?? ".",
        status: "success",
        resolvedPath: output.path,
        entries: output.entries.map((entry) => ({
          name: entry.name,
          path: entry.path,
          type: entry.type,
          ...(entry.size === undefined ? {} : { size: entry.size }),
        })),
        truncated: output.truncated,
      });
      continue;
    }
    if (result.status === "denied" || result.status === "failed") {
      lists.push({
        path: request.path ?? ".",
        status: result.status,
        code: listCodeFromMessage(result.message),
      });
      continue;
    }
    lists.push({
      path: typeof request.path === "string" ? request.path : ".",
      status: "invalid_input",
      code: "invalid_input",
    });
  }
  return { lists };
}

async function runSearches(root, input) {
  const tool = createWorkspaceSearchTool(root);
  const searches = [];
  for (const request of input.searches ?? []) {
    const result = await tool.execute(request, {});
    if (result.status === "success") {
      const output = result.output;
      searches.push({
        query: request.query,
        status: "success",
        path: output.path,
        matches: output.matches.map((match) => ({
          path: match.path,
          line: match.line,
          column: match.column,
          text: match.text,
        })),
        scannedFiles: output.scannedFiles,
        skippedFiles: output.skippedFiles,
        truncated: output.truncated,
        truncationReason: output.truncationReason,
      });
      continue;
    }
    if (
      result.status === "denied" ||
      result.status === "failed" ||
      result.status === "invalid_input"
    ) {
      searches.push({
        query: typeof request.query === "string" ? request.query : "",
        status: result.status,
        code: searchCodeFromMessage(result.message),
      });
      continue;
    }
    searches.push({
      query: typeof request.query === "string" ? request.query : "",
      status: result.status,
      code: "query_required",
    });
  }
  return { searches };
}

async function runPrepares(root, input) {
  const lock = createMutationLock();
  const checkpointRoot = mkdtempSync(join(tmpdir(), "siralos-oracle-cp-"));
  const store = await createFilesystemCheckpointStore({
    workspaceRoot: root,
    rootDirectory: checkpointRoot,
  });
  const tools = {
    "workspace.create_file": createWorkspaceCreateFileTool(root, lock, store),
    "workspace.edit_file": createWorkspaceEditFileTool(root, lock, store),
    "workspace.delete_file": createWorkspaceDeleteFileTool(root, lock, store),
  };
  const prepares = [];
  for (const request of input.prepares ?? []) {
    const tool = tools[request.tool];
    if (tool === undefined) {
      throw new Error(`unknown mutation tool ${request.tool}`);
    }
    const result = await tool.prepare(request.input, {
      ...(request.cancelled === true ? { signal: abortedSignal() } : {}),
    });
    if (result.status === "cancelled") {
      prepares.push({ tool: request.tool, status: "cancelled" });
    } else {
      prepares.push({
        tool: request.tool,
        status: result.status,
        ...(result.status === "unavailable" ? { code: "mutation_unavailable" } : {}),
      });
    }
  }
  const readTool = createWorkspaceReadTool(root);
  const verify = await readTool.execute({ path: input.verifyPath }, {});
  const workspaceSha256 = verify.status === "success" ? verify.output.sha256 : "missing";
  const checkpointCount = (await store.list()).length;
  return { prepares, workspaceSha256, checkpointCount };
}

async function runGit(root, input) {
  const backend = {
    async inspect() {
      return input.backend;
    },
    async execute() {
      throw new Error("Git must never execute when inspection is unavailable");
    },
    async close() {
      return undefined;
    },
  };
  const runDirectories = {
    async create() {
      return { ok: false, reason: "unavailable", message: "probe stub" };
    },
    async remove() {
      return { ok: false, reason: "unavailable", message: "probe stub" };
    },
  };
  const git = createGitCliAdapter({ workspaceRoot: root, backend, runDirectories });
  try {
    await git.getStatus({});
    return { disposition: "available", code: null };
  } catch (error) {
    return {
      disposition: "unavailable",
      code: error instanceof Error && "code" in error ? error.code : "git_unavailable",
    };
  }
}

const SUBJECTS = new Set([
  "workspace-read",
  "workspace-list",
  "workspace-search",
  "workspace-prepare",
  "git-inspection",
]);

async function main() {
  const input = readStdinBounded();
  if (typeof input.subject !== "string" || !SUBJECTS.has(input.subject)) {
    throw new Error(`unsupported subject ${input.subject}`);
  }
  const root = createWorkspace(input.files, input.symlinks);
  try {
    let result;
    switch (input.subject) {
      case "workspace-read":
        result = await runReads(root, input);
        break;
      case "workspace-list":
        result = await runLists(root, input);
        break;
      case "workspace-search":
        result = await runSearches(root, input);
        break;
      case "workspace-prepare":
        result = await runPrepares(root, input);
        break;
      case "git-inspection":
        result = await runGit(root, input);
        break;
      default:
        throw new Error(`unsupported subject ${input.subject}`);
    }
    process.stdout.write(`${JSON.stringify(result)}\n`);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

await main();
