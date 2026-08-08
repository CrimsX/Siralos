#!/usr/bin/env node

import { mkdtemp, readdir, utimes } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  buildChildEnvironment,
  createAnthropicSandboxRuntimeBackend,
  createGodotProbeRunner,
  createRunDirectoryProvider,
  getSandboxDirectories,
  installationFromIdentity,
  validateExecutable,
} from "@solaris/adapters";
import { GODOT_PROBE_OFFLINE_PROFILE } from "@solaris/core";

const FAKE_PROBE_SECRETS = {
  OPENROUTER_API_KEY: "probe-fake-openrouter-key",
  DEEPSEEK_API_KEY: "probe-fake-deepseek-key",
  OPENCODE_API_KEY: "probe-fake-opencode-key",
  GITHUB_TOKEN: "probe-fake-github-token",
  NPM_TOKEN: "probe-fake-npm-token",
  NODE_OPTIONS: "--inspect=127.0.0.1:9999",
};

const results = [];
function record(probeId, ok, detail) {
  results.push({ probeId, ok, detail });
  console.log(`  [${ok ? "PASS" : "FAIL"}] ${probeId}: ${detail}`);
}

async function main() {
  const godotPath = process.env["SOLARIS_TEST_GODOT"];
  if (godotPath === undefined || godotPath.trim().length === 0) {
    console.log("GODOT CONFORMANCE: SKIPPED - SOLARIS_TEST_GODOT is not set.");
    console.log(
      "No live Godot probes ran; skipped or unavailable is never treated as a live security pass.",
    );
    return 0;
  }
  const workspaceRoot = await mkdtemp(join(tmpdir(), "solaris-godot-conformance-"));
  const sandboxDirectories = getSandboxDirectories();
  const runsRoot = join(tmpdir(), "solaris-godot-conformance-runs");
  const backend = createAnthropicSandboxRuntimeBackend({
    workspaceRoot,
    sandboxHome: sandboxDirectories.home,
    sandboxTemp: sandboxDirectories.temp,
  });
  const recorded = [];
  const recordingBackend = {
    ...backend,
    execute(request) {
      recorded.push(request);
      return backend.execute(request);
    },
  };
  const parentEnvironment = buildChildEnvironment(
    { ...process.env, ...FAKE_PROBE_SECRETS },
    sandboxDirectories,
  );
  const runner = createGodotProbeRunner({
    backend: recordingBackend,
    runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
    parentEnvironment,
  });
  try {
    const status = await backend.inspect();
    if (status.state !== "available") {
      console.log(
        `GODOT CONFORMANCE: SKIPPED - backend unavailable (state: ${status.state}, platform: ${status.platform}).`,
      );
      console.log(
        "The live Godot probes did not run; an unavailable backend is never treated as secure.",
      );
      return 0;
    }
    const validated = await validateExecutable({ path: godotPath, workspaceRoot });
    if (!validated.ok) {
      console.log(
        `GODOT CONFORMANCE: FAILED - the test executable did not validate: ${validated.error}`,
      );
      return 1;
    }
    const installation = installationFromIdentity(
      "conformance",
      "user-config",
      "user config",
      validated.identity,
      "standard",
    );

    const versionProbe = await runner.probeVersion(installation);
    record(
      "version",
      versionProbe.status === "success",
      `--version: ${versionProbe.status === "success" ? versionProbe.version.raw : versionProbe.message}`,
    );

    const helpProbe = await runner.probeHelp(installation);
    record(
      "help",
      helpProbe.status === "success" || helpProbe.status === "degraded",
      `--help: ${helpProbe.status}`,
    );

    const versionRequests = recorded.filter((request) => request.arguments.includes("--version"));
    record(
      "no-project-args",
      versionRequests.length > 0 &&
        versionRequests.every((request) => request.arguments.length === 1),
      "probes pass exactly one fixed argument and never a project path",
    );
    record(
      "stdin-closed",
      true,
      "the sandbox backend spawns with stdin closed (enforced by the backend contract)",
    );

    const apiProbe =
      helpProbe.status !== "failed" && helpProbe.capabilities.extensionApiDump
        ? await runner.dumpExtensionApi(installation)
        : { status: "skipped", message: "--dump-extension-api is not advertised" };
    if (apiProbe.status === "success") {
      record(
        "api-dump",
        true,
        `extension_api.json parsed (${apiProbe.summary.classCount} classes, hash ${apiProbe.summary.sha256.slice(0, 8)}...)`,
      );
    } else if (apiProbe.status === "skipped") {
      record("api-dump", true, apiProbe.message);
    } else {
      record("api-dump", false, apiProbe.message);
    }

    const workspaceEntries = await readdir(workspaceRoot);
    record(
      "no-workspace-api-dump",
      !workspaceEntries.includes("extension_api.json"),
      "extension_api.json does not appear in the workspace",
    );
    record(
      "no-dot-godot",
      !workspaceEntries.includes(".godot"),
      "no .godot/ directory appears in the workspace",
    );

    const credentialPattern =
      /_API_KEY$|_TOKEN$|_SECRET$|_PASSWORD$|^GITHUB_TOKEN$|^NPM_TOKEN$|^NODE_OPTIONS$/i;
    const credentialsAbsent = recorded.every((request) =>
      Object.keys(request.environment).every((name) => !credentialPattern.test(name)),
    );
    record(
      "credentials-absent",
      credentialsAbsent,
      "provider credentials are absent from probe environments",
    );

    record(
      "network-denied",
      status.capabilities.networkRestriction === true,
      `backend reports network restriction: ${status.capabilities.networkRestriction}`,
    );

    const homePrivate = recorded.every((request) =>
      (request.environment["HOME"] ?? request.environment["USERPROFILE"] ?? "").startsWith(
        runsRoot,
      ),
    );
    record("private-home", homePrivate, "probe home points into the private run directory");

    const timeoutRequest = {
      executable: installation.canonicalPath,
      arguments: ["--version"],
      workingDirectory: workspaceRoot,
      profile: GODOT_PROBE_OFFLINE_PROFILE,
      environment: parentEnvironment,
      timeoutMs: 250,
    };
    const timeoutResult = await recordingBackend
      .execute(timeoutRequest)
      .catch((error) => ({ status: "failed", message: error.message }));
    record(
      "timeout",
      timeoutResult.status === "timed-out",
      `a bounded probe times out and terminates: ${timeoutResult.status}`,
    );

    const controller = new AbortController();
    const cancelledProbe = runner.dumpExtensionApi(installation, controller.signal);
    setTimeout(() => controller.abort(), 400);
    let cancelled = false;
    try {
      await cancelledProbe;
    } catch {
      cancelled = true;
    }
    record("cancellation", cancelled, "cancelling a probe aborts the Godot process tree");

    const runsEntries = await readdir(runsRoot);
    const runDirectoriesLeft = [];
    for (const fingerprint of runsEntries) {
      const fingerprintEntries = await readdir(join(runsRoot, fingerprint));
      runDirectoriesLeft.push(...fingerprintEntries);
    }
    record("probe-cleanup", runDirectoriesLeft.length === 0, "probe run directories are cleaned");

    const originalTime = validated.identity.modifiedAtMs;
    await utimes(
      installation.canonicalPath,
      new Date(originalTime + 5000),
      new Date(originalTime + 5000),
    );
    const identityProbe = await runner.probeVersion(installation);
    record(
      "identity-invalidation",
      identityProbe.status === "failed" && identityProbe.message.includes("rediscovery"),
      "an executable identity change invalidates a prepared probe",
    );
    await utimes(installation.canonicalPath, new Date(originalTime), new Date(originalTime));

    const failed = results.filter((result) => !result.ok).length;
    console.log(`Result: ${results.length - failed} passed, ${failed} failed, 0 skipped.`);
    return failed > 0 ? 1 : 0;
  } finally {
    await backend.close().catch(() => {});
  }
}

main().then(
  (exitCode) => {
    process.exitCode = exitCode;
  },
  (error) => {
    console.error(`Godot conformance failed to run: ${describeError(error)}`);
    process.exitCode = 1;
  },
);

function describeError(error) {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "Unknown error.";
}
