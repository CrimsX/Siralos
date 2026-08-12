#!/usr/bin/env node

import { mkdtemp, mkdir, rm, writeFile, readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import {
  createAnthropicSandboxRuntimeBackend,
  createEngineProfileCache,
  createGodotProbeRunner,
  createGodotProjectProbeService,
  createRunDirectoryProvider,
  DEFAULT_CHECKPOINT_ROOT,
  DEFAULT_USER_CONFIG,
  validateExecutable,
} from "@siralos/adapters";

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

const RECOVERY_FIXTURE_FILES = {
  "project.godot": [
    "[application]",
    'config/name="Recovery Probe Fixture"',
    'config/features=PackedStringArray("4.7")',
    'run/main_scene="res://main.tscn"',
    "",
    "[editor_plugins]",
    'enabled=PackedStringArray("res://addons/sideeffect")',
    "",
  ].join("\n"),
  "main.gd": 'extends Node\n\nfunc _ready() -> void:\n\tprint("main scene ready")\n',
  "main.tscn":
    '[gd_scene load_steps=2 format=3 uid="uid://recoveryfixture"]\n\n[ext_resource type="Script" path="res://main.gd" id="1"]\n\n[node name="Main" type="Node"]\nscript = ExtResource("1")\n',
  "tools/side-effect.gd": [
    "@tool",
    "extends Node",
    "",
    "func _init() -> void:",
    '\tvar file := FileAccess.open("res://tool-script-executed.marker", FileAccess.WRITE)',
    "\tif file:",
    '\t\tfile.store_string("tool script marker")\n\t\tfile.close()',
    "",
  ].join("\n"),
  "addons/sideeffect/plugin.cfg": [
    "[plugin]",
    'name="Side Effect"',
    'description="Writes a marker file when the plugin enters the editor tree."',
    'author="Siralos conformance"',
    'version="1.0"',
    'script="plugin.gd"',
    "",
  ].join("\n"),
  "addons/sideeffect/plugin.gd": [
    "@tool",
    "extends EditorPlugin",
    "",
    "func _enter_tree() -> void:",
    '\tvar file := FileAccess.open("res://plugin-executed.marker", FileAccess.WRITE)',
    "\tif file:",
    '\t\tfile.store_string("plugin marker")',
    "\t\tfile.close()",
    "",
  ].join("\n"),
  "assets/logo.svg":
    '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="blue"/></svg>\n',
};

async function writeFixtureFiles(root) {
  for (const [relativePath, content] of Object.entries(RECOVERY_FIXTURE_FILES)) {
    const target = join(root, relativePath);
    await mkdir(join(root, relativePath.split("/").slice(0, -1).join("/")), { recursive: true });
    await writeFile(target, content);
  }
}

async function main() {
  if (!godotPath || godotPath.trim().length === 0) {
    skip(
      "setup",
      "SIRALOS_TEST_GODOT is not set; live recovery conformance requires an explicit Godot editor executable.",
    );
    return;
  }
  if (!isAbsolute(godotPath.trim())) {
    skip("setup", "SIRALOS_TEST_GODOT must be an absolute path.");
    return;
  }
  const workRoot = await mkdtemp(join(tmpdir(), "siralos-recovery-conformance-"));
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
        `the sandbox backend is ${status.state}; live recovery isolation is unverified, never passed.`,
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
    const service = createGodotProjectProbeService({
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
        ? `recovery-mode probing truthfully reports unavailable on ${support.platform} with a precise reason.`
        : `recovery-mode probing claimed availability or lacked a precise reason (${support.state}); this is a FAIL-CLOSED violation.`,
    );

    let prepared;
    try {
      prepared = await service.prepare();
    } catch {
      prepared = { status: "failed", message: "preparation refused before any approval." };
    }
    const prepareRefuses = prepared.status === "unsupported" || prepared.status === "failed";
    record(
      "prepare-refuses",
      prepareRefuses,
      prepareRefuses
        ? `preparation refuses before any approval with status ${prepared.status}: ${prepared.message}`
        : `preparation unexpectedly reached a ready probe (${prepared.status}); the probe must refuse on this stage.`,
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
      "live engine isolation (recovery-mode launch against the mirror) cannot run while execution is fail-closed on this stage; it is never reported as passed.",
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
        "Skipped probes are never treated as passed: live recovery isolation is only verified when execution is available and a real recovery-capable editor runs.",
      );
    }
    process.exit(failed.length > 0 ? 1 : 0);
  },
  (error) => {
    console.error(
      `Recovery conformance failed: ${error instanceof Error ? error.message : String(error)}`,
    );
    process.exit(1);
  },
);
