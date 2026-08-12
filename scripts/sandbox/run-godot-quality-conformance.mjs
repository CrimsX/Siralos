import { mkdtemp, mkdir, rm, readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import {
  createEngineProfileCache,
  createFailClosedChangeSetFilePrimitives,
  createFilesystemCheckpointStore,
  createGDScriptDevelopmentService,
  createGDScriptLanguageService,
  createGodotDiagnosticsService,
  createGodotProbeRunner,
  createMutationLock,
  createRunDirectoryProvider,
  DEFAULT_CHECKPOINT_ROOT,
  DEFAULT_USER_CONFIG,
  validateExecutable,
} from "@siralos/adapters";
import { createAnthropicSandboxRuntimeBackend } from "@siralos/adapters";

const godotPath = process.env["SIRALOS_TEST_GODOT"];
const passed = [];
const failed = [];
const skipped = [];

function record(probeId, ok, detail) {
  const mark = ok ? "PASS" : "FAIL";
  if (!ok) {
    failed.push({ probeId, detail });
  } else {
    passed.push({ probeId, detail });
  }
  console.log(`[${mark}] ${probeId}: ${detail}`);
}

function skip(probeId, detail) {
  skipped.push({ probeId, detail });
  console.log(`[SKIP] ${probeId}: ${detail}`);
}

async function writeFixtureFiles(root) {
  const { mkdir, writeFile } = await import("node:fs/promises");
  const files = {
    "project.godot": [
      "[application]",
      'config/name="Quality Fixture"',
      'config/features=PackedStringArray("4.7")',
      "",
    ].join("\n"),
    "src/player/player.gd":
      "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n",
    "package.json": JSON.stringify(
      {
        name: "quality-fixture",
        private: true,
        scripts: { check: "node --check src/player/player.gd" },
      },
      null,
      2,
    ),
  };
  for (const [relativePath, content] of Object.entries(files)) {
    const target = join(root, relativePath);
    await mkdir(join(root, relativePath.split("/").slice(0, -1).join("/")), { recursive: true });
    await writeFile(target, content);
  }
}

async function main() {
  if (!godotPath || godotPath.trim().length === 0) {
    skip(
      "setup",
      "SIRALOS_TEST_GODOT is not set; live quality-stage conformance requires an explicit Godot editor executable.",
    );
    return;
  }
  if (!isAbsolute(godotPath.trim())) {
    skip("setup", "SIRALOS_TEST_GODOT must be an absolute path.");
    return;
  }
  const workRoot = await mkdtemp(join(tmpdir(), "siralos-quality-conformance-"));
  const fixtureWorkspace = join(workRoot, "fixture");
  const runsRoot = join(workRoot, "runs");
  const cacheRoot = join(workRoot, "cache");
  const checkpointRoot = join(workRoot, "checkpoints");
  await mkdir(fixtureWorkspace, { recursive: true });
  await mkdir(runsRoot, { recursive: true });
  await mkdir(cacheRoot, { recursive: true });
  await mkdir(checkpointRoot, { recursive: true });
  await writeFixtureFiles(fixtureWorkspace);

  const sandbox = createAnthropicSandboxRuntimeBackend({ workspaceRoot: fixtureWorkspace });
  let executedRequests = 0;
  const recordingBackend = {
    ...sandbox,
    async execute(request) {
      executedRequests += 1;
      return sandbox.execute(request);
    },
  };
  try {
    const status = await sandbox.inspect();
    if (status.state !== "available") {
      skip(
        "sandbox",
        `the sandbox backend is ${status.state}; live quality-stage isolation is unverified, never passed.`,
      );
      return;
    }
    record(
      "sandbox-available",
      true,
      "the sandbox backend is available; fail-closed behavior below is verified against it.",
    );
    const validated = await validateExecutable({
      path: godotPath,
      workspaceRoot: fixtureWorkspace,
    });
    if (!validated.ok) {
      record("executable", false, `the test Godot executable did not validate: ${validated.error}`);
      return;
    }
    const parentEnvironment = {
      ...process.env,
      OPENROUTER_API_KEY: "sk-live-fake",
      DEEPSEEK_API_KEY: "sk-live-fake",
      GITHUB_TOKEN: "gh-live-fake",
      AWS_SECRET_ACCESS_KEY: "aws-live-fake",
      PATH: process.env["PATH"] ?? "",
    };
    const runDirectories = createRunDirectoryProvider({
      workspaceRoot: fixtureWorkspace,
      runsRoot,
    });
    const probeRunner = createGodotProbeRunner({
      backend: recordingBackend,
      runDirectories,
      parentEnvironment,
    });
    const cache = createEngineProfileCache({ rootDirectory: cacheRoot });
    const config = {
      ...DEFAULT_USER_CONFIG.godot,
      installations: {
        "test-install": { path: godotPath, editionHint: "unknown" },
      },
      activeInstallation: null,
      discoverOnPath: false,
    };
    const language = createGDScriptLanguageService({
      workspaceRoot: fixtureWorkspace,
      config,
      preference: { kind: "installation-id", installationId: "test-install" },
      overrideSource: "cli",
      backend: recordingBackend,
      probeRunner,
      cache,
      hostPath: parentEnvironment["PATH"] ?? null,
      hostPathExt: parentEnvironment["PATHEXT"] ?? null,
      platform: process.platform,
      runDirectories,
      checkpointRoot: DEFAULT_CHECKPOINT_ROOT,
      parentEnvironment,
    });
    const diagnostics = createGodotDiagnosticsService({
      workspaceRoot: fixtureWorkspace,
      config,
      preference: { kind: "installation-id", installationId: "test-install" },
      overrideSource: "cli",
      backend: recordingBackend,
      probeRunner,
      cache,
      hostPath: parentEnvironment["PATH"] ?? null,
      hostPathExt: parentEnvironment["PATHEXT"] ?? null,
      platform: process.platform,
      runDirectories,
      checkpointRoot: DEFAULT_CHECKPOINT_ROOT,
      parentEnvironment,
    });
    const checkpoints = await createFilesystemCheckpointStore({
      workspaceRoot: fixtureWorkspace,
      rootDirectory: checkpointRoot,
    });
    const development = createGDScriptDevelopmentService({
      workspaceRoot: fixtureWorkspace,
      platform: process.platform,
      store: checkpoints,
      lock: createMutationLock(),
      language,
      diagnostics,
      git: null,
      // Fail-closed at this stage on every platform: Node offers no
      // directory-relative commit primitive.
      canApplyIdentityBound: false,
      primitives: createFailClosedChangeSetFilePrimitives(),
      qualityStage: {
        reviewer: {
          review: () => Promise.resolve({ status: "completed", findings: [], message: null }),
        },
        validation: {
          discovery: {
            discover: () =>
              Promise.resolve({
                packageScripts: { check: "node --check src/player/player.gd" },
              }),
          },
          executor: {
            run: (step) =>
              Promise.resolve({
                step,
                status: "unavailable",
                exitCode: null,
                summary: "unavailable",
              }),
          },
        },
      },
    });

    const support = await development.support();
    const capabilityTruthful =
      support.state === "unavailable" &&
      support.reason !== null &&
      support.reason.includes("no directory-relative");
    record(
      "capability-truthful",
      capabilityTruthful,
      capabilityTruthful
        ? `the development workflow (and with it the quality stage) truthfully reports unavailable on ${support.platform} with a precise reason.`
        : `the workflow claimed availability or lacked a precise reason (${support.state}); this is a FAIL-CLOSED violation.`,
    );

    const statusBefore = development.status();
    record(
      "quality-not-run",
      statusBefore.session === null,
      statusBefore.session === null
        ? "no workflow exists, so no quality stage, review, or validation command can run."
        : "a workflow became active despite the fail-closed gate.",
    );

    let prepared;
    try {
      prepared = await development.prepareStart("conformance quality request");
    } catch {
      prepared = { status: "failed", message: "preparation refused before any approval." };
    }
    const prepareRefuses =
      prepared.status === "unavailable" ||
      prepared.status === "failed" ||
      prepared.status === "invalid_input";
    record(
      "prepare-refuses",
      prepareRefuses,
      prepareRefuses
        ? `workflow preparation refuses before any approval with status ${prepared.status}: ${prepared.message}`
        : `preparation unexpectedly produced an approvable workflow (${prepared.status}); the workflow must refuse on this stage.`,
    );

    // Structural note (not a probe): the independent reviewer is only
    // reachable inside the quality stage, which cannot start while the
    // workflow is fail-closed; nothing can call the reviewer here.
    console.log(
      "[NOTE] no-reviewer-calls: the reviewer is only reachable inside the quality stage, which cannot start while the workflow is fail-closed.",
    );

    const runs = await readdir(runsRoot).catch(() => []);
    record(
      "nothing-created",
      runs.length === 0,
      runs.length === 0
        ? "no private run directory or mirror entry was created."
        : `unexpected entries were created under the runs root: ${runs.join(", ")}.`,
    );

    record(
      "nothing-executed",
      executedRequests === 0,
      executedRequests === 0
        ? "the sandbox backend never received an execution request; no validation command ran."
        : "the sandbox backend received an execution request; nothing may run at this stage.",
    );

    const checkpointEntries = await readdir(checkpointRoot).catch(() => []);
    record(
      "no-checkpoints-created",
      checkpointEntries.length === 0,
      checkpointEntries.length === 0
        ? "no checkpoint was created."
        : `unexpected checkpoint entries: ${checkpointEntries.join(", ")}.`,
    );

    const sourceEntries = await readdir(fixtureWorkspace);
    record(
      "source-untouched",
      !sourceEntries.includes(".godot") && sourceEntries.includes("src"),
      sourceEntries.includes(".godot")
        ? ".godot was created in the source workspace."
        : "the source workspace has no generated .godot directory.",
    );

    skip(
      "live-quality-isolation",
      "live quality-stage isolation (gates -> validation plan -> fresh review -> repair cycle) cannot run while the change-set applier is fail-closed on this stage; it is never reported as passed.",
    );
  } finally {
    await sandbox.close().catch(() => undefined);
    await rm(workRoot, { recursive: true, force: true }).catch(() => undefined);
  }
}

main().then(
  () => {
    console.log(
      `Result: ${passed.length} passed, ${failed.length} failed, ${skipped.length} skipped.`,
    );
    if (skipped.length > 0) {
      console.log(
        "Skipped probes are never treated as passed: the live quality stage is only verified when identity-bound execution is available and a real editor runs.",
      );
    }
    process.exit(failed.length > 0 ? 1 : 0);
  },
  (error) => {
    console.error(
      `Quality-stage conformance failed: ${error instanceof Error ? error.message : String(error)}`,
    );
    process.exit(1);
  },
);
