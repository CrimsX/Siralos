import { createHash, randomUUID } from "node:crypto";
import { lstat, readFile, stat, writeFile } from "node:fs/promises";
import { basename, extname } from "node:path";
import path from "node:path";
import { pathToFileURL } from "node:url";
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

/**
 * Node startup flags for the spawned request. They must all precede the
 * script path (the first non-option argument).
 *  - `--no-addons` denies native addon loading (process.dlopen).
 *  - `--disallow-code-generation-from-strings` denies eval and new Function.
 */
const NODE_SELF_CONTAINMENT_FLAGS: readonly string[] = [
  "--no-addons",
  "--disallow-code-generation-from-strings",
];

/** Solaris-owned ESM preload that denies every additional module load. */
export const SOLARIS_ESM_DENIAL_BOOTSTRAP_FILE = "solaris-deny-imports.bootstrap.mjs";

/** Solaris-owned CommonJS preload that denies every additional module load. */
export const SOLARIS_CJS_DENIAL_BOOTSTRAP_FILE = "solaris-deny-requires.bootstrap.cjs";

/**
 * Private environment variable carrying the staged entry path to the
 * Solaris-owned bootstraps. It is added only to the spawned child
 * environment after the allowlisted base is built, and it is never part of
 * the approval digest (it is derived from the approved plan at execution).
 */
const STAGED_ENTRY_ENVIRONMENT_NAME = "SOLARIS_STAGED_ENTRY";

/**
 * CommonJS preload (loaded via `--require`) that mechanically denies every
 * module-loading surface of the spawned process: `require` and direct
 * `Module._load` calls throw for every request except the single staged
 * entry (which Node itself loads as the main module) and Node's own
 * builtin modules (which keep Node's internals functional and never load
 * user code). The `Worker` constructor is replaced with a throwing stub so
 * worker threads cannot load unapproved files; the vm code-generation
 * surfaces and the child-process spawn surfaces are replaced with throwing
 * stubs so unapproved code cannot be compiled or executed in child
 * processes. This file is CommonJS (`.cjs`) because `--require` preloads
 * must be CommonJS. Its bytes are a Solaris-controlled constant.
 */
const SOLARIS_CJS_DENIAL_BOOTSTRAP_CONTENT = `"use strict";
const Module = require("node:module");
const path = require("node:path");
const workerThreads = require("node:worker_threads");
const vm = require("node:vm");
const childProcess = require("node:child_process");
const cluster = require("node:cluster");

const entryPath = path.resolve(process.env.SOLARIS_STAGED_ENTRY);
const originalModuleLoad = Module._load;

function denied(request) {
  const error = new Error(
    "Solaris: additional module loading is denied (requested \\"" +
      String(request) +
      '\\"); the approved node-script executes alone from its private copy and must be self-contained',
  );
  error.code = "SOLARIS_DENIED_MODULE_LOAD";
  throw error;
}

function denySurface(target, property) {
  Object.defineProperty(target, property, {
    configurable: true,
    writable: true,
    value: function deniedSurface() {
      return denied(property);
    },
  });
}

Module._load = function (request, parent, isMain) {
  if (typeof request !== "string") {
    return denied(request);
  }
  if (request.startsWith("node:") || Module.builtinModules.includes(request)) {
    return originalModuleLoad(request, parent, isMain);
  }
  if (isMain === true) {
    try {
      if (path.resolve(Module._resolveFilename(request, parent, isMain)) === entryPath) {
        return originalModuleLoad(request, parent, isMain);
      }
    } catch (error) {
      // a request the resolver rejects falls through to the denial below
    }
  }
  return denied(request);
};

Module.prototype.require = function (request) {
  return denied(request);
};

denySurface(workerThreads, "Worker");
for (const property of [
  "Script",
  "SourceTextModule",
  "SyntheticModule",
  "compileFunction",
  "runInThisContext",
  "runInNewContext",
  "runInContext",
]) {
  denySurface(vm, property);
}
for (const property of [
  "spawn",
  "spawnSync",
  "exec",
  "execSync",
  "execFile",
  "execFileSync",
  "fork",
]) {
  denySurface(childProcess, property);
}
denySurface(cluster, "fork");
`;

/**
 * ESM preload (loaded via `--import`) that registers a module loader whose
 * `resolve` hook throws for every specifier except the single staged entry.
 * The hook chain applies process-wide (static and dynamic imports in ESM
 * and in CommonJS `import()`), so no additional module can ever be loaded.
 * The entry href is embedded into the inline loader source, so the loader
 * itself needs no imports. Loader-hook registration on the `node:module`
 * exports is additionally shadowed (best-effort) so the script cannot
 * re-register hooks. Its bytes are a Solaris-controlled constant.
 */
const SOLARIS_ESM_DENIAL_BOOTSTRAP_CONTENT = `import { register } from "node:module";
import { pathToFileURL } from "node:url";

const entryHref = pathToFileURL(process.env.SOLARIS_STAGED_ENTRY).href;
const loaderSource =
  "let entryHref = " + JSON.stringify(entryHref) + ";\\n" +
  "export async function resolve(specifier, context, nextResolve) {\\n" +
  "  if (specifier === entryHref) {\\n" +
  "    return nextResolve(specifier, context);\\n" +
  "  }\\n" +
  "  const e = new Error(\\"Solaris: additional code loading is denied; the approved node-script executes alone and must be self-contained\\");\\n" +
  "  e.code = \\"SOLARIS_DENIED_LOAD\\";\\n" +
  "  throw e;\\n" +
  "}\\n";
const previousNoDeprecation = process.noDeprecation;
try {
  process.noDeprecation = true;
} catch {
  // best-effort suppression of the module.register() deprecation warning
}
register("data:text/javascript," + encodeURIComponent(loaderSource));
try {
  process.noDeprecation = previousNoDeprecation;
} catch {
  // best-effort restore
}

const moduleExports = process.getBuiltinModule("node:module");
for (const property of ["register", "registerHooks"]) {
  try {
    Object.defineProperty(moduleExports, property, {
      configurable: true,
      writable: true,
      value: function deniedHooks() {
        const error = new Error(
          "Solaris: module loader hook registration is denied; the approved node-script executes alone",
        );
        error.code = "SOLARIS_DENIED_HOOKS";
        throw error;
      },
    });
  } catch {
    // shadowing is best-effort; the deny-all resolve hook remains authoritative
  }
}
`;

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

interface StagedRunFiles {
  readonly scriptPath: string;
  readonly esmBootstrapPath: string;
  readonly cjsBootstrapPath: string;
  readonly scriptSha256: string;
  readonly esmBootstrapSha256: string;
  readonly cjsBootstrapSha256: string;
}

export function createNodeScriptRunner(options: NodeScriptRunnerOptions): CommandRunner {
  const plans = new WeakMap<PreparedCommand, NodeScriptPlan>();

  return {
    definition: {
      id: "node-script",
      description:
        "Run one self-contained JavaScript file through Solaris's trusted Node.js executable; the file executes alone from a digest-bound private copy and every additional code-loading mechanism is denied at runtime.",
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
      const esmBootstrap = await stageBootstrapFile(
        context.runPaths.scriptCache,
        SOLARIS_ESM_DENIAL_BOOTSTRAP_FILE,
        SOLARIS_ESM_DENIAL_BOOTSTRAP_CONTENT,
      );
      if (esmBootstrap.status !== "ready") {
        return esmBootstrap;
      }
      const cjsBootstrap = await stageBootstrapFile(
        context.runPaths.scriptCache,
        SOLARIS_CJS_DENIAL_BOOTSTRAP_FILE,
        SOLARIS_CJS_DENIAL_BOOTSTRAP_CONTENT,
      );
      if (cjsBootstrap.status !== "ready") {
        return cjsBootstrap;
      }
      const stagedFiles: StagedRunFiles = {
        scriptPath: stagedScript.privatePath,
        esmBootstrapPath: esmBootstrap.privatePath,
        cjsBootstrapPath: cjsBootstrap.privatePath,
        scriptSha256: plan.script.sha256,
        esmBootstrapSha256: sha256OfUtf8(SOLARIS_ESM_DENIAL_BOOTSTRAP_CONTENT),
        cjsBootstrapSha256: sha256OfUtf8(SOLARIS_CJS_DENIAL_BOOTSTRAP_CONTENT),
      };
      const finalVerification = await verifyStagedRunFiles(stagedFiles);
      if (finalVerification !== null) {
        return { status: "conflict", message: finalVerification };
      }
      // The run directory is Solaris-private (0700); the residual window
      // between this final verification and the backend's spawn stays
      // within that trust domain. It is documented, not claimed closed.
      const environment: Record<string, string> = {
        ...buildCommandEnvironment(
          readParentEnvironment(),
          {
            home: context.runPaths.home,
            temp: context.runPaths.temp,
            npmCache: context.runPaths.npmCache,
            npmUserConfig: context.runPaths.npmUserConfig,
          },
          { npm: false },
        ),
      };
      // Belt and braces: the child-environment allowlist already excludes
      // NODE_OPTIONS; never leave a startup-options injection vector open.
      delete environment["NODE_OPTIONS"];
      environment[STAGED_ENTRY_ENVIRONMENT_NAME] = stagedScript.privatePath;
      return {
        status: "ready",
        request: {
          executable: plan.nodeIdentity.executable,
          executableIdentity: `node ${plan.nodeIdentity.version}`,
          executableVersion: plan.nodeIdentity.version,
          arguments: [
            ...NODE_SELF_CONTAINMENT_FLAGS,
            "--import",
            pathToFileURL(esmBootstrap.privatePath).href,
            "--require",
            cjsBootstrap.privatePath,
            stagedScript.privatePath,
            ...plan.arguments,
          ],
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

/**
 * Writes one Solaris-owned bootstrap file into the private run directory
 * with exclusive creation (`wx`) and verifies the written bytes against the
 * expected Solaris-controlled content. A pre-existing or tampered file at
 * the staging path always conflicts; nothing is ever overwritten.
 */
async function stageBootstrapFile(
  scriptCacheDirectory: string,
  fileName: string,
  expectedContent: string,
): Promise<
  | { readonly status: "conflict"; readonly message: string }
  | { readonly status: "ready"; readonly privatePath: string }
> {
  const expectedBytes = Buffer.from(expectedContent, "utf8");
  const expectedSha256 = sha256OfUtf8(expectedContent);
  const privatePath = path.join(scriptCacheDirectory, fileName);
  try {
    await writeFile(privatePath, expectedBytes, { flag: "wx" });
  } catch (error: unknown) {
    return {
      status: "conflict",
      message: `The Solaris loading-denial bootstrap could not be staged in the private run directory: ${describeFsError(error)}`,
    };
  }
  let stagedBytes: Buffer;
  try {
    stagedBytes = await readFile(privatePath);
  } catch (error: unknown) {
    return {
      status: "conflict",
      message: `The Solaris loading-denial bootstrap could not be verified: ${describeFsError(error)}`,
    };
  }
  if (
    stagedBytes.length !== expectedBytes.length ||
    !stagedBytes.equals(expectedBytes) ||
    createHash("sha256").update(stagedBytes).digest("hex") !== expectedSha256
  ) {
    return {
      status: "conflict",
      message: "The Solaris loading-denial bootstrap does not match the Solaris-controlled bytes.",
    };
  }
  return { status: "ready", privatePath };
}

/**
 * Immediate re-verification of every staged run file (the script and both
 * bootstraps) right before the execution request is returned. Any file that
 * no longer carries its expected bytes refuses the execution.
 */
async function verifyStagedRunFiles(files: StagedRunFiles): Promise<string | null> {
  const entries: ReadonlyArray<readonly [string, string, string]> = [
    ["The staged script", files.scriptPath, files.scriptSha256],
    ["The ESM loading-denial bootstrap", files.esmBootstrapPath, files.esmBootstrapSha256],
    ["The CommonJS loading-denial bootstrap", files.cjsBootstrapPath, files.cjsBootstrapSha256],
  ];
  for (const [label, filePath, expectedSha256] of entries) {
    let bytes: Buffer;
    try {
      bytes = await readFile(filePath);
    } catch (error: unknown) {
      return `${label} could not be verified before execution: ${describeFsError(error)}`;
    }
    if (createHash("sha256").update(bytes).digest("hex") !== expectedSha256) {
      return `${label} changed after staging; refusing to execute.`;
    }
  }
  return null;
}

function sha256OfUtf8(content: string): string {
  return createHash("sha256").update(Buffer.from(content, "utf8")).digest("hex");
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
      "The approved single file executes alone: Solaris stages an immutable private copy of it into the sandbox run directory and binds the approval digest to that copy's SHA-256. The script must be self-contained — the runtime mechanically denies every additional code-loading mechanism (static and dynamic imports, require(), worker threads, eval and new Function, and native addons), so any attempt to load other code fails closed. __dirname and import.meta.url refer to the private copy while process.cwd() stays in the workspace; the workspace stays read-only and network access is denied.",
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
