#!/usr/bin/env node

import { mkdtemp, mkdir, rm, writeFile, readdir, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  createAnthropicSandboxRuntimeBackend,
  createEngineProfileCache,
  createGodotEngineProfiler,
  createGodotProbeRunner,
  createGodotProjectProbeService,
  createRunDirectoryProvider,
  getSandboxDirectories,
  validateExecutable,
} from "@solaris/adapters";
import { DEFAULT_CHECKPOINT_ROOT } from "@solaris/adapters";
import { DEFAULT_USER_CONFIG } from "@solaris/adapters";

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
    'author="Solaris conformance"',
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

async function findMarkers(root) {
  const markers = [];
  const walk = async (directory) => {
    let entries;
    try {
      entries = await readdir(directory, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const full = join(directory, entry.name);
      if (entry.isDirectory()) {
        await walk(full);
      } else if (entry.name.endsWith(".marker")) {
        markers.push(full);
      }
    }
  };
  await walk(root);
  return markers;
}

async function main() {
  if (!godotPath || godotPath.trim().length === 0) {
    skip(
      "setup",
      "SOLARIS_TEST_GODOT is not set; live recovery conformance requires an explicit Godot editor executable.",
    );
    return;
  }
  const workRoot = await mkdtemp(join(tmpdir(), "solaris-recovery-conformance-"));
  const fixtureWorkspace = join(workRoot, "fixture");
  const runsRoot = join(workRoot, "runs");
  const cacheRoot = join(workRoot, "cache");
  await mkdir(fixtureWorkspace, { recursive: true });
  await mkdir(runsRoot, { recursive: true });
  await mkdir(cacheRoot, { recursive: true });
  await writeFixtureFiles(fixtureWorkspace);

  const sandboxDirectories = getSandboxDirectories();
  const sandbox = createAnthropicSandboxRuntimeBackend({
    workspaceRoot: fixtureWorkspace,
    sandboxHome: sandboxDirectories.home,
    sandboxTemp: sandboxDirectories.temp,
  });
  let executedRequests = [];
  const recordingBackend = {
    ...sandbox,
    async execute(request) {
      const result = await sandbox.execute(request);
      executedRequests.push(request);
      return result;
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
    const profiler = createGodotEngineProfiler({
      config: {
        ...DEFAULT_USER_CONFIG.godot,
        installations: {},
        activeInstallation: null,
        discoverOnPath: false,
      },
      preference: { kind: "path", path: godotPath },
      overrideSource: "cli",
      workspaceRoot: fixtureWorkspace,
      backend: recordingBackend,
      probeRunner,
      cache,
      hostPath: parentEnvironment["PATH"] ?? null,
      hostPathExt: parentEnvironment["PATHEXT"] ?? null,
      platform: process.platform,
    });
    const selected = await profiler.selectedProfile();
    if (selected === null) {
      record(
        "engine-selection",
        false,
        "no selectable Godot installation could be profiled from SOLARIS_TEST_GODOT.",
      );
      return;
    }
    const recoveryMode = selected.profile.capabilities.recoveryMode;
    record(
      "recovery-capability",
      recoveryMode,
      recoveryMode
        ? `the selected editor advertises --recovery-mode (${selected.profile.version.raw}).`
        : `the selected editor does not advertise --recovery-mode (${selected.profile.version.raw}); the probe is unsupported and must not run.`,
    );
    if (!recoveryMode) {
      return;
    }

    const service = createGodotProjectProbeService({
      workspaceRoot: fixtureWorkspace,
      config: {
        ...DEFAULT_USER_CONFIG.godot,
        installations: {},
        activeInstallation: null,
        discoverOnPath: false,
      },
      preference: { kind: "path", path: godotPath },
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
    const prepared = await service.prepare();
    if (prepared.status !== "ready") {
      record("probe-prepare", false, `the probe could not be prepared: ${prepared.message}`);
      return;
    }
    // The conformance harness acts as the user and approves exactly once.
    const result = await service.execute(prepared.probe, { approvedDigest: prepared.digest });

    record(
      "probe-exit",
      result.status === "completed" || result.status === "completed_with_diagnostics",
      `probe finished with status ${result.status} in ${result.process.durationMs}ms (${result.process.exitCode === null ? "no exit" : `exit ${result.process.exitCode}`}).`,
    );
    if (result.status === "completed" || result.status === "completed_with_diagnostics") {
      record(
        "mirror-construction",
        result.mirror.sourceFiles > 0,
        `the disposable mirror was constructed with ${result.mirror.sourceFiles} files (${result.mirror.sourceBytes} bytes).`,
      );
      const recoveryRequest = executedRequests.find((request) =>
        request.arguments.includes("--recovery-mode"),
      );
      record(
        "recovery-launch",
        recoveryRequest !== undefined,
        recoveryRequest !== undefined
          ? "Godot was launched with --recovery-mode against the mirror project."
          : "no engine launch with --recovery-mode was observed.",
      );
      if (recoveryRequest !== undefined) {
        const argumentsOk =
          recoveryRequest.arguments.includes("--headless") &&
          recoveryRequest.arguments.includes("--editor") &&
          recoveryRequest.arguments.includes("--recovery-mode") &&
          recoveryRequest.arguments.includes("--path") &&
          !recoveryRequest.arguments.some((argument) =>
            [
              "--script",
              "--scene",
              "--import",
              "--upwards",
              "--export",
              "--build-solutions",
              "--lsp",
              "--dap",
              "--debug-server",
            ].includes(argument),
          );
        record(
          "command-shape",
          argumentsOk,
          `launch arguments: ${recoveryRequest.arguments.join(" ")}`,
        );
        const networkDenied = recoveryRequest.profile.network.outbound === "deny";
        record(
          "network-denied",
          networkDenied,
          networkDenied
            ? "network is denied in the recovery profile."
            : "network denial is NOT enforced.",
        );
        const environment = recoveryRequest.environment ?? {};
        const secretLeak = Object.entries(environment).some(
          ([name, value]) =>
            /_API_KEY$|_TOKEN$|_SECRET$|_PASSWORD$/i.test(name) ||
            ["sk-live-fake", "gh-live-fake", "aws-live-fake"].includes(value),
        );
        record(
          "secrets-absent",
          !secretLeak,
          secretLeak
            ? "a provider credential leaked into the recovery environment."
            : "provider credentials are absent from the recovery environment.",
        );
        const injected = Object.keys(environment).some((name) =>
          /^LD_PRELOAD$|^LD_LIBRARY_PATH$|^DYLD_/.test(name),
        );
        record(
          "no-library-injection",
          !injected,
          injected
            ? "a library-injection variable reached the engine."
            : "library-injection variables are absent.",
        );
      }
      const mirrorMarkers = await findMarkers(fixtureWorkspace).catch(() => []);
      record(
        "tool-script-suppressed",
        !mirrorMarkers.some((marker) => marker.endsWith("tool-script-executed.marker")),
        mirrorMarkers.some((marker) => marker.endsWith("tool-script-executed.marker"))
          ? "the @tool script produced its side-effect marker."
          : "the @tool script produced no side-effect marker.",
      );
      record(
        "plugin-suppressed",
        !mirrorMarkers.some((marker) => marker.endsWith("plugin-executed.marker")),
        mirrorMarkers.some((marker) => marker.endsWith("plugin-executed.marker"))
          ? "the enabled editor plugin produced its side-effect marker."
          : "the enabled editor plugin produced no side-effect marker.",
      );
      record(
        "workspace-unchanged",
        result.workspaceIntegrity.unchanged,
        result.workspaceIntegrity.unchanged
          ? "the source workspace integrity baseline is unchanged."
          : "the source workspace changed during the probe.",
      );
      const sourceEntries = await readdir(fixtureWorkspace);
      record(
        "no-source-dot-godot",
        !sourceEntries.includes(".godot"),
        sourceEntries.includes(".godot")
          ? ".godot was created in the source workspace."
          : "no .godot exists in the source workspace.",
      );
      record(
        "generated-cache-in-mirror",
        result.mirror.generatedGodotDirectory === true ||
          result.mirror.generatedGodotDirectory === false,
        result.mirror.generatedGodotDirectory
          ? `.godot generation was observed inside the mirror (${result.mirror.generatedFiles ?? "?"} files, ${result.mirror.generatedBytes ?? "?"} bytes; import state: ${result.mirror.importState}).`
          : "no .godot generation was observed; import state is reported as unknown.",
      );
      record(
        "mirror-removed",
        result.cleanup.completed === true,
        result.cleanup.completed
          ? "the disposable mirror was removed."
          : "the disposable mirror was NOT removed.",
      );
    }
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
        "Skipped probes are never treated as passed: live recovery isolation is only verified when the real sandbox and a real recovery-capable editor succeed.",
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
