import { createHash, randomUUID } from "node:crypto";
import { lstat, readFile, stat, writeFile } from "node:fs/promises";
import { basename, extname } from "node:path";
import path from "node:path";
import {
  COMMAND_LIMITS,
  createPreparedCommand,
  VALIDATION_OFFLINE_PROFILE,
  type CommandDigestService,
  type CommandExecutionContext,
  type CommandExecutionRequestResult,
  type CommandPreparationContext,
  type CommandPreparationResult,
  type CommandPreview,
  type CommandRunner,
  type PreparedCommand,
} from "@solaris/core";
import { buildCommandEnvironment } from "../../environment/command-environment.js";
import { readParentEnvironment } from "../../environment/child-environment.js";
import {
  parseCommonCommandFields,
  resolveCommandWorkingDirectory,
  type CommandWorkingDirectory,
} from "../command-validation.js";
import { resolveWorkspacePath } from "../../tools/workspace/workspace-path.js";
import { resolveTrustedNode, type TrustedNodeIdentity } from "../trusted-executables.js";

export interface NodeScriptRunnerOptions {
  readonly digest: CommandDigestService;
}

const ALLOWED_SCRIPT_EXTENSIONS: readonly string[] = [".js", ".mjs", ".cjs"];

interface NodeScriptFile {
  readonly workspaceRelativePath: string;
  readonly absolutePath: string;
  readonly sha256: string;
}

interface NodeScriptPlan {
  readonly workspaceRoot: string;
  readonly workingDirectory: CommandWorkingDirectory;
  readonly script: NodeScriptFile;
  readonly arguments: readonly string[];
  readonly timeoutMs: number;
  readonly nodeIdentity: TrustedNodeIdentity;
}

export function createNodeScriptRunner(options: NodeScriptRunnerOptions): CommandRunner {
  const plans = new WeakMap<PreparedCommand, NodeScriptPlan>();

  return {
    definition: {
      id: "node-script",
      description: "Run one JavaScript file through Solaris's trusted Node.js executable.",
    },
    async prepare(
      input: unknown,
      context: CommandPreparationContext,
    ): Promise<CommandPreparationResult> {
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Preparation was cancelled." };
      }
      const parsed = parseCommonCommandFields(input, "node-script", [
        "runner",
        "path",
        "arguments",
        "workingDirectory",
        "timeoutMs",
      ]);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const scriptValue = (input as Record<string, unknown>)["path"];
      if (typeof scriptValue !== "string" || scriptValue.length === 0) {
        return { status: "invalid_input", message: '"path" is required.' };
      }
      const workingDirectory = await resolveCommandWorkingDirectory(
        context.workspaceRoot,
        parsed.value.workingDirectory,
      );
      if (!workingDirectory.ok) {
        return { status: "invalid_input", message: workingDirectory.message };
      }
      const script = await resolveNodeScriptFile(context.workspaceRoot, scriptValue);
      if (!script.ok) {
        return { status: "invalid_input", message: script.message };
      }
      const nodeIdentity = resolveTrustedNode();
      const command = createPreparedCommand();
      const plan: NodeScriptPlan = {
        workspaceRoot: context.workspaceRoot,
        workingDirectory: workingDirectory.value,
        script: script.value,
        arguments: parsed.value.arguments,
        timeoutMs: parsed.value.timeoutMs,
        nodeIdentity,
      };
      plans.set(command, plan);
      const preview = buildNodeScriptPreview(plan);
      const digest = computeNodeScriptDigest(options.digest, plan, preview);
      return {
        status: "ready",
        command,
        preview,
        digest,
        commandId: randomUUID(),
      };
    },

    async toExecutionRequest(
      command: PreparedCommand,
      context: CommandExecutionContext,
    ): Promise<CommandExecutionRequestResult> {
      const plan = plans.get(command);
      if (plan === undefined) {
        return {
          status: "conflict",
          message: "The prepared command is not valid for this runner or has already been used.",
        };
      }
      const revalidated = await revalidateNodeScriptPlan(plan, options.digest);
      if (revalidated.status !== "ready") {
        return revalidated;
      }
      if (revalidated.digest !== context.approvedDigest) {
        return {
          status: "conflict",
          message: "The command plan changed after approval.",
        };
      }
      const stagedScript = await stageApprovedScript(plan, context.runPaths.scriptCache);
      if (stagedScript.status !== "ready") {
        return stagedScript;
      }
      const environment = buildCommandEnvironment(
        readParentEnvironment(),
        {
          home: context.runPaths.home,
          temp: context.runPaths.temp,
          npmCache: context.runPaths.npmCache,
          npmUserConfig: context.runPaths.npmUserConfig,
        },
        { npm: false },
      );
      return {
        status: "ready",
        request: {
          executable: plan.nodeIdentity.executable,
          executableIdentity: `node ${plan.nodeIdentity.version}`,
          executableVersion: plan.nodeIdentity.version,
          arguments: [stagedScript.privatePath, ...plan.arguments],
          workingDirectory: plan.workingDirectory.absolutePath,
          environment,
          digest: revalidated.digest,
        },
      };
    },

    async isAvailable(): Promise<boolean> {
      await Promise.resolve();
      return true;
    },
  };
}

/**
 * Copies the exact approved script bytes into the private run directory and
 * verifies the private copy's hash. The child process executes only the
 * immutable private copy — never the mutable workspace path — so a script
 * replaced between revalidation and execution can never run.
 */
async function stageApprovedScript(
  plan: NodeScriptPlan,
  scriptCacheDirectory: string,
): Promise<
  | { readonly status: "conflict"; readonly message: string }
  | { readonly status: "ready"; readonly privatePath: string }
> {
  let bytes: Buffer;
  try {
    bytes = await readFile(plan.script.absolutePath);
  } catch (error: unknown) {
    return {
      status: "conflict",
      message: `The script changed after approval: ${describeFsError(error)}`,
    };
  }
  const readHash = createHash("sha256").update(bytes).digest("hex");
  if (readHash !== plan.script.sha256) {
    return { status: "conflict", message: "The script changed after approval." };
  }
  const privatePath = path.join(scriptCacheDirectory, basename(plan.script.workspaceRelativePath));
  try {
    await writeFile(privatePath, bytes, { flag: "wx" });
  } catch (error: unknown) {
    return {
      status: "conflict",
      message: `The approved script could not be staged in the private run directory: ${describeFsError(error)}`,
    };
  }
  let privateBytes: Buffer;
  try {
    privateBytes = await readFile(privatePath);
  } catch (error: unknown) {
    return {
      status: "conflict",
      message: `The private script copy could not be verified: ${describeFsError(error)}`,
    };
  }
  if (
    privateBytes.length !== bytes.length ||
    !privateBytes.equals(bytes) ||
    createHash("sha256").update(privateBytes).digest("hex") !== plan.script.sha256
  ) {
    return {
      status: "conflict",
      message: "The private script copy does not match the approved bytes.",
    };
  }
  return { status: "ready", privatePath };
}

async function revalidateNodeScriptPlan(
  plan: NodeScriptPlan,
  digestService: CommandDigestService,
): Promise<
  | { readonly status: "conflict"; readonly message: string }
  | { readonly status: "unavailable"; readonly message: string }
  | { readonly status: "failed"; readonly message: string }
  | { readonly status: "ready"; readonly digest: string }
> {
  const workingDirectory = await resolveCommandWorkingDirectory(
    plan.workspaceRoot,
    plan.workingDirectory.workspaceRelativePath,
  );
  if (!workingDirectory.ok) {
    return { status: "conflict", message: "The working directory changed after approval." };
  }
  if (workingDirectory.value.absolutePath !== plan.workingDirectory.absolutePath) {
    return { status: "conflict", message: "The working directory changed after approval." };
  }
  const script = await resolveNodeScriptFile(plan.workspaceRoot, plan.script.workspaceRelativePath);
  if (!script.ok) {
    return { status: "conflict", message: `The script changed after approval: ${script.message}` };
  }
  if (script.value.sha256 !== plan.script.sha256) {
    return { status: "conflict", message: "The script changed after approval." };
  }
  const nodeIdentity = resolveTrustedNode();
  if (
    nodeIdentity.executable !== plan.nodeIdentity.executable ||
    nodeIdentity.version !== plan.nodeIdentity.version
  ) {
    return {
      status: "conflict",
      message: "The trusted Node executable changed after approval.",
    };
  }
  const preview = buildNodeScriptPreview(plan);
  return { status: "ready", digest: computeNodeScriptDigest(digestService, plan, preview) };
}

async function resolveNodeScriptFile(
  workspaceRoot: string,
  requested: string,
): Promise<
  | { readonly ok: true; readonly value: NodeScriptFile }
  | { readonly ok: false; readonly message: string }
> {
  const extension = extname(requested).toLowerCase();
  if (!ALLOWED_SCRIPT_EXTENSIONS.includes(extension)) {
    return {
      ok: false,
      message: "Only .js, .mjs, and .cjs scripts are supported.",
    };
  }
  const resolved = await resolveWorkspacePath(workspaceRoot, requested);
  if (resolved.status !== "resolved") {
    return { ok: false, message: resolved.message };
  }
  const rawPath = path.resolve(workspaceRoot, requested);
  let rawStats;
  try {
    rawStats = await lstat(rawPath);
  } catch (error: unknown) {
    return { ok: false, message: `The script file is not accessible: ${describeFsError(error)}` };
  }
  if (rawStats.isSymbolicLink()) {
    return { ok: false, message: "The script file must not be a symbolic link." };
  }
  let stats;
  try {
    stats = await stat(resolved.absolutePath);
  } catch (error: unknown) {
    return { ok: false, message: `The script file is not accessible: ${describeFsError(error)}` };
  }
  if (!stats.isFile()) {
    return { ok: false, message: "The script path is not a regular file." };
  }
  if (stats.size > COMMAND_LIMITS.maxNodeScriptBytes) {
    return {
      ok: false,
      message: `The script exceeds the ${COMMAND_LIMITS.maxNodeScriptBytes}-byte limit.`,
    };
  }
  let bytes: Buffer;
  try {
    bytes = await readFile(resolved.absolutePath);
  } catch (error: unknown) {
    return { ok: false, message: `The script file cannot be read: ${describeFsError(error)}` };
  }
  return {
    ok: true,
    value: {
      workspaceRelativePath: resolved.workspaceRelativePath,
      absolutePath: resolved.absolutePath,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    },
  };
}

function buildNodeScriptPreview(plan: NodeScriptPlan): CommandPreview {
  return {
    runnerId: "node-script",
    displayName: `node ${plan.script.workspaceRelativePath}`,
    workingDirectory: plan.workingDirectory.workspaceRelativePath,
    executableIdentity: `node ${plan.nodeIdentity.version}`,
    arguments: [plan.script.workspaceRelativePath, ...plan.arguments],
    timeoutMs: plan.timeoutMs,
    stdoutLimitBytes: COMMAND_LIMITS.stdoutHardLimitBytes,
    stderrLimitBytes: COMMAND_LIMITS.stderrHardLimitBytes,
    workspaceAccess: "read-only",
    networkAccess: "denied",
    environmentPolicy: "minimal",
    stdinPolicy: "closed",
    executionNotice:
      "The script runs from an immutable private copy inside the sandbox run directory; __dirname and import.meta.url refer to that private copy, while process.cwd() stays in the workspace.",
  };
}

function computeNodeScriptDigest(
  digestService: CommandDigestService,
  plan: NodeScriptPlan,
  preview: CommandPreview,
): string {
  return digestService.compute({
    runnerId: "node-script",
    executableIdentity: preview.executableIdentity,
    executableVersion: plan.nodeIdentity.version,
    script: plan.script.workspaceRelativePath,
    fileHash: plan.script.sha256,
    repositoryScript: null,
    arguments: plan.arguments,
    workingDirectory: plan.workingDirectory.workspaceRelativePath,
    profileId: VALIDATION_OFFLINE_PROFILE.id,
    environmentPolicy: "minimal",
    timeoutMs: plan.timeoutMs,
    stdoutLimitBytes: COMMAND_LIMITS.stdoutHardLimitBytes,
    stderrLimitBytes: COMMAND_LIMITS.stderrHardLimitBytes,
    stdinPolicy: "closed",
    networkPolicy: "denied",
  });
}

function describeFsError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message.replace(/,\s*'[^']*'$/, "");
  }
  return "a filesystem error occurred";
}
