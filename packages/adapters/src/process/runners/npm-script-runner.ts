import { createHash, randomUUID } from "node:crypto";
import { lstat, readFile, stat } from "node:fs/promises";
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
import {
  resolveNpmCli,
  resolveTrustedNode,
  type NpmCliResolution,
  type TrustedNodeIdentity,
} from "../trusted-executables.js";

export interface NpmScriptRunnerOptions {
  readonly digest: CommandDigestService;
  /** Injectable trusted npm CLI resolver; defaults to the trusted resolver. */
  readonly npmResolver?: () => Promise<NpmCliResolution>;
}

interface PackageJsonInfo {
  readonly sha256: string;
  readonly name: string | null;
  readonly scripts: Readonly<Record<string, string>>;
}

interface NpmScriptPlan {
  readonly workspaceRoot: string;
  readonly workingDirectory: CommandWorkingDirectory;
  readonly packageJson: PackageJsonInfo;
  readonly scriptName: string;
  readonly scriptBody: string;
  readonly arguments: readonly string[];
  readonly timeoutMs: number;
  readonly nodeIdentity: TrustedNodeIdentity;
  readonly npmCliPath: string;
  readonly npmVersion: string;
}

export function createNpmScriptRunner(options: NpmScriptRunnerOptions): CommandRunner {
  const plans = new WeakMap<PreparedCommand, NpmScriptPlan>();
  const resolveNpm = options.npmResolver ?? resolveNpmCli;

  return {
    definition: {
      id: "npm-script",
      description: "Run one existing npm package script with structured arguments.",
    },
    async prepare(
      input: unknown,
      context: CommandPreparationContext,
    ): Promise<CommandPreparationResult> {
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Preparation was cancelled." };
      }
      const parsed = parseCommonCommandFields(input, "npm-script", [
        "runner",
        "script",
        "arguments",
        "workingDirectory",
        "timeoutMs",
      ]);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const scriptValue = (input as Record<string, unknown>)["script"];
      if (typeof scriptValue !== "string" || scriptValue.length === 0) {
        return { status: "invalid_input", message: '"script" is required.' };
      }
      if (Buffer.byteLength(scriptValue, "utf8") > COMMAND_LIMITS.maxNpmScriptNameBytes) {
        return {
          status: "invalid_input",
          message: `The script name exceeds the ${COMMAND_LIMITS.maxNpmScriptNameBytes}-byte limit.`,
        };
      }
      const workingDirectory = await resolveCommandWorkingDirectory(
        context.workspaceRoot,
        parsed.value.workingDirectory,
      );
      if (!workingDirectory.ok) {
        return { status: "invalid_input", message: workingDirectory.message };
      }
      const packageJson = await resolvePackageJson(workingDirectory.value.absolutePath);
      if (!packageJson.ok) {
        return { status: "invalid_input", message: packageJson.message };
      }
      const scriptBody = packageJson.value.scripts[scriptValue];
      if (scriptBody === undefined) {
        return {
          status: "invalid_input",
          message: `No npm script named "${scriptValue}" exists in the package.`,
        };
      }
      if (scriptBody.length === 0) {
        return {
          status: "invalid_input",
          message: `The npm script "${scriptValue}" must not be empty.`,
        };
      }
      const npmCli = await resolveNpm();
      if (npmCli.status !== "resolved") {
        return { status: "unavailable", message: npmCli.message };
      }
      const nodeIdentity = resolveTrustedNode();
      const command = createPreparedCommand();
      const plan: NpmScriptPlan = {
        workspaceRoot: context.workspaceRoot,
        workingDirectory: workingDirectory.value,
        packageJson: packageJson.value,
        scriptName: scriptValue,
        scriptBody,
        arguments: parsed.value.arguments,
        timeoutMs: parsed.value.timeoutMs,
        nodeIdentity,
        npmCliPath: npmCli.cliPath,
        npmVersion: npmCli.version,
      };
      plans.set(command, plan);
      const preview = buildNpmScriptPreview(plan);
      const digest = computeNpmScriptDigest(options.digest, plan, preview);
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
      const revalidated = await revalidateNpmScriptPlan(plan, options.digest, resolveNpm);
      if (revalidated.status !== "ready") {
        return revalidated;
      }
      if (revalidated.digest !== context.approvedDigest) {
        return {
          status: "conflict",
          message: "The command plan changed after approval.",
        };
      }
      const environment = buildCommandEnvironment(
        readParentEnvironment(),
        {
          home: context.runPaths.home,
          temp: context.runPaths.temp,
          npmCache: context.runPaths.npmCache,
          npmUserConfig: context.runPaths.npmUserConfig,
        },
        { npm: true },
      );
      return {
        status: "ready",
        request: {
          executable: plan.nodeIdentity.executable,
          executableIdentity: `node ${plan.nodeIdentity.version} + npm ${plan.npmVersion}`,
          executableVersion: plan.nodeIdentity.version,
          arguments: [plan.npmCliPath, "run", plan.scriptName, "--", ...plan.arguments],
          workingDirectory: plan.workingDirectory.absolutePath,
          environment,
          digest: revalidated.digest,
        },
      };
    },

    async isAvailable(): Promise<boolean> {
      return (await resolveNpm()).status === "resolved";
    },
  };
}

async function revalidateNpmScriptPlan(
  plan: NpmScriptPlan,
  digestService: CommandDigestService,
  resolveNpm: () => Promise<NpmCliResolution>,
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
  const packageJson = await resolvePackageJson(workingDirectory.value.absolutePath);
  if (!packageJson.ok) {
    return {
      status: "conflict",
      message: `The package changed after approval: ${packageJson.message}`,
    };
  }
  if (packageJson.value.sha256 !== plan.packageJson.sha256) {
    return { status: "conflict", message: "The package.json changed after approval." };
  }
  const scriptBody = packageJson.value.scripts[plan.scriptName];
  if (scriptBody === undefined || scriptBody !== plan.scriptBody) {
    return { status: "conflict", message: "The npm script changed after approval." };
  }
  const npmCli = await resolveNpm();
  if (npmCli.status !== "resolved") {
    return { status: "unavailable", message: npmCli.message };
  }
  if (npmCli.cliPath !== plan.npmCliPath || npmCli.version !== plan.npmVersion) {
    return { status: "conflict", message: "The trusted npm CLI changed after approval." };
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
  const preview = buildNpmScriptPreview(plan);
  return { status: "ready", digest: computeNpmScriptDigest(digestService, plan, preview) };
}

async function resolvePackageJson(
  workingDirectoryAbsolutePath: string,
): Promise<
  | { readonly ok: true; readonly value: PackageJsonInfo }
  | { readonly ok: false; readonly message: string }
> {
  const packageJsonPath = path.join(workingDirectoryAbsolutePath, "package.json");
  let rawStats;
  try {
    rawStats = await lstat(packageJsonPath);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `No package.json exists in the working directory: ${describeFsError(error)}`,
    };
  }
  if (rawStats.isSymbolicLink()) {
    return { ok: false, message: "The package.json must not be a symbolic link." };
  }
  let stats;
  try {
    stats = await stat(packageJsonPath);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The package.json is not accessible: ${describeFsError(error)}`,
    };
  }
  if (!stats.isFile()) {
    return { ok: false, message: "The package.json is not a regular file." };
  }
  if (stats.size > COMMAND_LIMITS.maxPackageJsonBytes) {
    return {
      ok: false,
      message: `The package.json exceeds the ${COMMAND_LIMITS.maxPackageJsonBytes}-byte limit.`,
    };
  }
  let rawBytes: Buffer;
  try {
    rawBytes = await readFile(packageJsonPath);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The package.json cannot be read: ${describeFsError(error)}`,
    };
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(rawBytes.toString("utf8"));
  } catch (error: unknown) {
    return { ok: false, message: `The package.json is not valid JSON: ${describeError(error)}` };
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return { ok: false, message: "The package.json must be a JSON object." };
  }
  const record = parsed as Record<string, unknown>;
  const scripts = record["scripts"];
  if (typeof scripts !== "object" || scripts === null || Array.isArray(scripts)) {
    return { ok: false, message: 'The package.json has no "scripts" object.' };
  }
  const name = typeof record["name"] === "string" ? record["name"] : null;
  if (name !== null && Buffer.byteLength(name, "utf8") > 1024) {
    return { ok: false, message: "The package name is too long." };
  }
  const scriptValues: Record<string, string> = {};
  for (const [scriptName, scriptValue] of Object.entries(scripts)) {
    if (typeof scriptValue !== "string") {
      return {
        ok: false,
        message: `The npm script "${scriptName}" must be a string.`,
      };
    }
    if (Buffer.byteLength(scriptValue, "utf8") > COMMAND_LIMITS.maxNpmScriptBytes) {
      return {
        ok: false,
        message: `The npm script "${scriptName}" exceeds the ${COMMAND_LIMITS.maxNpmScriptBytes}-byte limit.`,
      };
    }
    scriptValues[scriptName] = scriptValue;
  }
  return {
    ok: true,
    value: {
      sha256: createHash("sha256").update(rawBytes).digest("hex"),
      name,
      scripts: scriptValues,
    },
  };
}

function buildNpmScriptPreview(plan: NpmScriptPlan): CommandPreview {
  const hookNames = [plan.scriptName];
  return {
    runnerId: "npm-script",
    displayName: `npm run ${plan.scriptName}`,
    ...(plan.packageJson.name === null ? {} : { packageName: plan.packageJson.name }),
    scriptName: plan.scriptName,
    workingDirectory: plan.workingDirectory.workspaceRelativePath,
    executableIdentity: `node ${plan.nodeIdentity.version} + npm ${plan.npmVersion}`,
    arguments: ["run", plan.scriptName, "--", ...plan.arguments],
    repositoryScript: plan.scriptBody,
    timeoutMs: plan.timeoutMs,
    stdoutLimitBytes: COMMAND_LIMITS.stdoutHardLimitBytes,
    stderrLimitBytes: COMMAND_LIMITS.stderrHardLimitBytes,
    workspaceAccess: "read-only",
    networkAccess: "denied",
    environmentPolicy: "minimal",
    stdinPolicy: "closed",
    scriptShellNotice:
      "npm executes this repository-defined script through its platform script shell.",
    hooksNotice: `Automatically associated ${hookNames
      .flatMap((name) => [`pre${name}`, `post${name}`])
      .join(" and ")} scripts are disabled.`,
  };
}

function computeNpmScriptDigest(
  digestService: CommandDigestService,
  plan: NpmScriptPlan,
  preview: CommandPreview,
): string {
  return digestService.compute({
    runnerId: "npm-script",
    executableIdentity: preview.executableIdentity,
    executableVersion: plan.nodeIdentity.version,
    script: plan.scriptName,
    fileHash: plan.packageJson.sha256,
    repositoryScript: plan.scriptBody,
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

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown error occurred";
}
