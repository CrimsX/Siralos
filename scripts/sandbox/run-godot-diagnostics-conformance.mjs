import { mkdtemp, mkdir, rm, writeFile, readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import {
  createAnthropicSandboxRuntimeBackend,
  createEngineProfileCache,
  createGodotDiagnosticsService,
  createGodotProbeRunner,
  createRunDirectoryProvider,
  DEFAULT_CHECKPOINT_ROOT,
  DEFAULT_USER_CONFIG,
  validateExecutable,
} from "@solaris/adapters";

const godotPath = process.env["SOLARIS_TEST_GODOT"];
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

const DIAGNOSTIC_FIXTURE_FILES = {
  "project.godot": [
    "[application]",
    'config/name="Diagnostics Fixture"',
    'config/features=PackedStringArray("4.7")',
    "",
  ].join("\n"),
  "src/player/player.gd": 'extends CharacterBody2D\n\nfunc _ready() -> void:\n\tprint("player")\n',
  "src/ui/menu.gd": "extends Control\n\nfunc _ready() -> void:\n\tpass\n",
};

async function writeFixtureFiles(root) {
  for (const [relativePath, content] of Object.entries(DIAGNOSTIC_FIXTURE_FILES)) {
    const target = join(root, relativePath);
    await mkdir(join(root, relativePath.split("/").slice(0, -1).join("/")), { recursive: true });
    await writeFile(target, content);
  }
}

async function main() {
  if (!godotPath || godotPath.trim().length === 0) {
    skip(
      "setup",
      "SOLARIS_TEST_GODOT is not set; live diagnostics conformance requires an explicit Godot editor executable.",
    );
    return;
  }
  if (!isAbsolute(godotPath.trim())) {
    skip("setup", "SOLARIS_TEST_GODOT must be an absolute path.");
    return;
  }
  const workRoot = await mkdtemp(join(tmpdir(), "solaris-diagnostics-conformance-"));
  const fixtureWorkspace = join(workRoot, "fixture");
  const runsRoot = join(workRoot, "runs");
  const cacheRoot = join(workRoot, "cache");
  await mkdir(fixtureWorkspace, { recursive: true });
  await mkdir(runsRoot, { recursive: true });
  await mkdir(cacheRoot, { recursive: true });
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
        `the sandbox backend is ${status.state}; live diagnostics isolation is unverified, never passed.`,
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
      GODOT_EDITOR_PATH: "/evil/godot",
      LD_PRELOAD: "/lib/evil.so",
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
    const service = createGodotDiagnosticsService({
      workspaceRoot: fixtureWorkspace,
      config: {
        ...DEFAULT_USER_CONFIG.godot,
        installations: {
          "test-install": { path: godotPath, editionHint: "unknown" },
        },
        activeInstallation: null,
        discoverOnPath: false,
      },
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

    const support = await service.support();
    const capabilityTruthful =
      support.state === "unavailable" &&
      support.reason !== null &&
      support.reason.includes("exec-by-handle");
    record(
      "capability-truthful",
      capabilityTruthful,
      capabilityTruthful
        ? `GDScript check-only diagnostics truthfully report unavailable on ${support.platform} with a precise reason.`
        : `diagnostics claimed availability or lacked a precise reason (${support.state}); this is a FAIL-CLOSED violation.`,
    );

    let prepared;
    try {
      prepared = await service.prepare({ paths: ["src/player/player.gd"] });
    } catch {
      prepared = { status: "failed", message: "preparation refused before any approval." };
    }
    const prepareRefuses =
      prepared.status === "unsupported" ||
      prepared.status === "failed" ||
      prepared.status === "unavailable";
    record(
      "prepare-refuses",
      prepareRefuses,
      prepareRefuses
        ? `preparation refuses before any approval with status ${prepared.status}: ${prepared.message}`
        : `preparation unexpectedly reached a ready check (${prepared.status}); the check must refuse on this stage.`,
    );

    const runs = await readdir(runsRoot).catch(() => []);
    const nothingCreated = runs.length === 0;
    record(
      "nothing-created",
      nothingCreated,
      nothingCreated
        ? "no private run directory or mirror entry was created."
        : `unexpected entries were created under the runs root: ${runs.join(", ")}.`,
    );

    const nothingExecuted = executedRequests === 0;
    record(
      "nothing-executed",
      nothingExecuted,
      nothingExecuted
        ? "the sandbox backend never received an execution request."
        : "the sandbox backend received an execution request; nothing may run at this stage.",
    );

    const sourceEntries = await readdir(fixtureWorkspace);
    record(
      "source-untouched",
      !sourceEntries.includes(".godot"),
      sourceEntries.includes(".godot")
        ? ".godot was created in the source workspace."
        : "the source workspace has no generated .godot directory.",
    );

    skip(
      "live-isolation",
      "live engine isolation (--check-only against the disposable mirror) cannot run while execution is fail-closed on this stage; it is never reported as passed.",
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
        "Skipped probes are never treated as passed: live diagnostics isolation is only verified when execution is available and a real check-capable editor runs.",
      );
    }
    process.exit(failed.length > 0 ? 1 : 0);
  },
  (error) => {
    console.error(
      `Diagnostics conformance failed: ${error instanceof Error ? error.message : String(error)}`,
    );
    process.exit(1);
  },
);
