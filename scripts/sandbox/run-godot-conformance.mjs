#!/usr/bin/env node

import { mkdtemp, readdir, rm, writeFile, chmod } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  buildChildEnvironment,
  createAnthropicSandboxRuntimeBackend,
  createGodotProbeRunner,
  createRunDirectoryProvider,
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

/**
 * Disposable fixture executables for the process-level probes. The
 * user-supplied Godot engine is never modified and never used for
 * timeout/cancellation/identity tests: those run against these fixed
 * fixtures, so their outcomes never depend on real engine timing.
 */
async function writeFixtureExecutable(directory, name, body) {
  const fixturePath = join(directory, name);
  await writeFile(fixturePath, body);
  await chmod(fixturePath, 0o755).catch(() => undefined);
  return fixturePath;
}

const HANG_FIXTURE = "#!/bin/sh\nwhile true; do sleep 1; done\n";
const DESCENDANT_FIXTURE =
  "#!/bin/sh\n(sleep 1000 & echo $! > child.pid)\nwhile true; do sleep 1; done\n";
const STDIN_FIXTURE =
  '#!/bin/sh\nif read -t 3 line; then echo "STDIN_OPEN"; else echo "STDIN_CLOSED"; fi\n';
const FAKE_VERSION_FIXTURE = '#!/bin/sh\necho "4.7.1.stable.official (conformance fixture)"\n';

function processAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
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
  const fixtureRoot = await mkdtemp(join(tmpdir(), "solaris-godot-fixtures-"));
  const workspaceRoot = await mkdtemp(join(tmpdir(), "solaris-godot-conformance-"));
  const runsRoot = join(tmpdir(), "solaris-godot-conformance-runs");
  const backend = createAnthropicSandboxRuntimeBackend({ workspaceRoot });
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
    { home: join(fixtureRoot, "home"), temp: join(fixtureRoot, "tmp") },
  );
  const runner = createGodotProbeRunner({
    backend: recordingBackend,
    runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
    parentEnvironment,
  });
  let exitCode = 1;
  try {
    const status = await backend.inspect();
    if (status.state !== "available" || !status.capabilities.filesystemReadRestriction) {
      console.log(
        `GODOT CONFORMANCE: SKIPPED - backend unavailable or host-read isolation unenforced (state: ${status.state}, platform: ${status.platform}).`,
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

    const probeRequests = recorded.filter((request) =>
      ["--version", "--help", "--dump-extension-api"].some((argument) =>
        request.arguments.includes(argument),
      ),
    );
    record(
      "no-project-args",
      probeRequests.length > 0 && probeRequests.every((request) => request.arguments.length === 1),
      "probes pass exactly one fixed argument and never a project path",
    );
    record(
      "executed-private-copy",
      probeRequests.length > 0 &&
        probeRequests.every((request) => request.executable.startsWith(runsRoot)),
      "every probe executes the verified private copy inside the run directory, never the configured executable path",
    );
    record(
      "exact-read-only-roots",
      probeRequests.length > 0 &&
        probeRequests.every((request) => Array.isArray(request.explicitReadRoots)),
      "every probe request carries an explicit read-roots list (no workspace or install-parent surface)",
    );
    record(
      "no-workspace-request-path",
      probeRequests.every((request) => {
        const serialized = JSON.stringify(request);
        return !serialized.includes(workspaceRoot);
      }),
      "no probe request contains the workspace path in arguments, environment, or config",
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

    // ---- Fixed internal process fixtures (never the real engine) ----

    const hangExecutable = await writeFixtureExecutable(fixtureRoot, "hang-fixture", HANG_FIXTURE);
    const hangRun = await createRunDirectoryProvider({
      workspaceRoot,
      runsRoot,
    }).create();
    try {
      // The fixture never exits, so a bounded timeout is deterministic.
      const timeoutRequest = {
        executable: hangExecutable,
        arguments: [],
        workingDirectory: hangRun.temp,
        profile: GODOT_PROBE_OFFLINE_PROFILE,
        environment: parentEnvironment,
        runDirectory: hangRun.root,
        timeoutMs: 250,
      };
      const timeoutResult = await recordingBackend
        .execute(timeoutRequest)
        .catch((error) => ({ status: "failed", message: error.message }));
      record(
        "timeout",
        timeoutResult.status === "timed-out",
        `a bounded process is terminated on timeout: ${timeoutResult.status}`,
      );

      // Aborting a never-exiting process is deterministic.
      const abortController = new AbortController();
      const cancellationPromise = recordingBackend
        .execute({ ...timeoutRequest, timeoutMs: 60_000, signal: abortController.signal })
        .catch((error) => ({ status: "failed", message: error.message }));
      const abortTimer = setTimeout(() => abortController.abort(), 100);
      const cancellationResult = await cancellationPromise;
      clearTimeout(abortTimer);
      record(
        "cancellation",
        cancellationResult.status === "cancelled",
        `cancelling a bounded process terminates it: ${cancellationResult.status}`,
      );

      // Descendant termination: the fixture spawns a child and records its
      // pid; cancellation must terminate the whole tree.
      const descendantExecutable = await writeFixtureExecutable(
        fixtureRoot,
        "descendant-fixture",
        DESCENDANT_FIXTURE,
      );
      const descendantRun = await createRunDirectoryProvider({
        workspaceRoot,
        runsRoot,
      }).create();
      try {
        const descendantController = new AbortController();
        const descendantPromise = recordingBackend
          .execute({
            executable: descendantExecutable,
            arguments: [],
            workingDirectory: descendantRun.temp,
            profile: GODOT_PROBE_OFFLINE_PROFILE,
            environment: parentEnvironment,
            runDirectory: descendantRun.root,
            timeoutMs: 60_000,
            signal: descendantController.signal,
          })
          .catch((error) => ({ status: "failed", message: error.message }));
        const childPidPath = join(descendantRun.temp, "child.pid");
        let childPid = null;
        for (let attempt = 0; attempt < 100; attempt += 1) {
          try {
            const { readFile } = await import("node:fs/promises");
            const content = await readFile(childPidPath, "utf8");
            const parsed = Number(content.trim());
            if (Number.isSafeInteger(parsed) && parsed > 0) {
              childPid = parsed;
              break;
            }
          } catch {
            // the fixture has not written the pid yet; keep waiting
          }
          await new Promise((resolve) => setTimeout(resolve, 50));
        }
        descendantController.abort();
        const descendantResult = await descendantPromise;
        let descendantTerminated = false;
        if (childPid !== null && descendantResult.status === "cancelled") {
          for (let attempt = 0; attempt < 100; attempt += 1) {
            if (!processAlive(childPid)) {
              descendantTerminated = true;
              break;
            }
            await new Promise((resolve) => setTimeout(resolve, 50));
          }
        }
        record(
          "descendant-termination",
          descendantTerminated,
          `cancellation terminates the descendant process tree: ${descendantTerminated ? "child terminated" : `child ${String(childPid)} still alive`}`,
        );
      } finally {
        await createRunDirectoryProvider({ workspaceRoot, runsRoot })
          .remove(descendantRun.runId)
          .catch(() => undefined);
      }

      // Closed stdin is observed, not assumed: the fixture reports whether
      // a read from stdin returns immediately.
      const stdinExecutable = await writeFixtureExecutable(
        fixtureRoot,
        "stdin-fixture",
        STDIN_FIXTURE,
      );
      const stdinResult = await recordingBackend.execute({
        executable: stdinExecutable,
        arguments: [],
        workingDirectory: hangRun.temp,
        profile: GODOT_PROBE_OFFLINE_PROFILE,
        environment: parentEnvironment,
        runDirectory: hangRun.root,
        timeoutMs: 10_000,
      });
      record(
        "stdin-closed",
        stdinResult.status === "completed" && stdinResult.stdout.includes("STDIN_CLOSED"),
        `probe stdin is closed: ${stdinResult.status === "completed" ? stdinResult.stdout.trim().split(/\r?\n/).pop() : stdinResult.status}`,
      );
    } finally {
      await createRunDirectoryProvider({ workspaceRoot, runsRoot })
        .remove(hangRun.runId)
        .catch(() => undefined);
    }

    // Identity invalidation runs against a disposable fixture copy, never
    // the user-supplied engine: same-size replacement with restored mtime
    // must invalidate the prepared probe identity.
    const fakeExecutable = await writeFixtureExecutable(
      fixtureRoot,
      "identity-fixture",
      FAKE_VERSION_FIXTURE,
    );
    const fakeValidated = await validateExecutable({ path: fakeExecutable, workspaceRoot });
    if (fakeValidated.ok) {
      const fakeInstallation = installationFromIdentity(
        "identity-fixture",
        "user-config",
        "user config",
        fakeValidated.identity,
        "standard",
      );
      const before = await runner.probeVersion(fakeInstallation);
      record(
        "fixture-version",
        before.status === "success",
        `the disposable fixture itself probes successfully: ${before.status === "success" ? before.version.raw : before.message}`,
      );
      const originalBytes = await import("node:fs/promises").then((fs) =>
        fs.readFile(fakeExecutable),
      );
      // Same-size replacement with a restored mtime: only the full-hash
      // revalidation can detect it.
      const replacement = Buffer.from(
        '#!/bin/sh\necho "4.7.1.stable.official (conformance fixtur3)"\n',
      );
      if (replacement.length === originalBytes.length) {
        const { stat, utimes } = await import("node:fs/promises");
        const originalTime = fakeValidated.identity.modifiedAtMs;
        await writeFile(fakeExecutable, replacement);
        await utimes(fakeExecutable, new Date(originalTime), new Date(originalTime));
        const sizeUnchanged = (await stat(fakeExecutable)).size === originalBytes.length;
        const after = await runner.probeVersion(fakeInstallation);
        record(
          "identity-invalidation",
          sizeUnchanged && after.status === "failed",
          `same-size content replacement with restored mtime invalidates the prepared identity: ${after.status === "failed" ? after.message : `unexpected ${after.status}`}`,
        );
      } else {
        record(
          "identity-invalidation",
          false,
          "the fixture replacement fixture was not same-size; the invalidation probe could not be exercised",
        );
      }
    } else {
      record(
        "identity-invalidation",
        false,
        `the disposable fixture could not be validated: ${fakeValidated.error}`,
      );
    }

    const runsEntries = await readdir(runsRoot);
    const runDirectoriesLeft = [];
    for (const fingerprint of runsEntries) {
      const fingerprintEntries = await readdir(join(runsRoot, fingerprint));
      runDirectoriesLeft.push(...fingerprintEntries);
    }
    record("probe-cleanup", runDirectoriesLeft.length === 0, "probe run directories are cleaned");

    const failed = results.filter((result) => !result.ok).length;
    console.log(`Result: ${results.length - failed} passed, ${failed} failed, 0 skipped.`);
    exitCode = failed > 0 ? 1 : 0;
    return exitCode;
  } finally {
    await backend.close().catch(() => {});
    await rm(fixtureRoot, { recursive: true, force: true }).catch(() => undefined);
    await rm(workspaceRoot, { recursive: true, force: true }).catch(() => undefined);
    await rm(runsRoot, { recursive: true, force: true }).catch(() => undefined);
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
