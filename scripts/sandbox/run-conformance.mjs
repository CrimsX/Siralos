#!/usr/bin/env node

import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  createAnthropicSandboxRuntimeBackend,
  removeConformanceArtifacts,
  runSandboxConformance,
} from "@solaris/adapters";
import { DEVELOP_OFFLINE_PROFILE } from "@solaris/core";

const FAKE_PROBE_SECRETS = {
  OPENROUTER_API_KEY: "probe-fake-openrouter-key",
  DEEPSEEK_API_KEY: "probe-fake-deepseek-key",
  OPENCODE_API_KEY: "probe-fake-opencode-key",
  GITHUB_TOKEN: "probe-fake-github-token",
  NPM_TOKEN: "probe-fake-npm-token",
  NODE_OPTIONS: "--inspect=127.0.0.1:9999",
};

async function main() {
  const workspaceRoot = await mkdtemp(join(tmpdir(), "solaris-conformance-"));
  const backend = createAnthropicSandboxRuntimeBackend({ workspaceRoot });
  try {
    const status = await backend.inspect();
    if (status.state !== "available") {
      console.log(
        `SANDBOX CONFORMANCE: SKIPPED - backend unavailable (state: ${status.state}, platform: ${status.platform}).`,
      );
      if (status.message !== undefined) {
        console.log(`  ${status.message}`);
      }
      console.log(
        "The conformance suite did not run; an unavailable backend is never treated as secure.",
      );
      return 0;
    }
    if (!status.capabilities.filesystemReadRestriction) {
      console.log(
        `SANDBOX CONFORMANCE: SKIPPED - the backend cannot enforce the host-read allowlist on ${status.platform}.`,
      );
      if (status.message !== undefined) {
        console.log(`  ${status.message}`);
      }
      console.log(
        "Process execution is refused while the host-read boundary cannot be enforced; skipped is never treated as passed.",
      );
      return 0;
    }
    // The per-request run directories (and their home/temp) are created and
    // granted by the conformance runner itself; the parent environment only
    // supplies the allowlisted base plus fake secrets for the env probes.
    const environment = { ...process.env, ...FAKE_PROBE_SECRETS };
    const report = await runSandboxConformance(backend, {
      workspaceRoot,
      profile: DEVELOP_OFFLINE_PROFILE,
      parentEnvironment: environment,
    });
    if (report.passed === 0 && report.failed === 0 && report.skipped > 0) {
      // The suite could not execute (for example because Solaris cannot
      // create verified private run directories at this stage): every probe
      // reports skipped with the truthful reason, and skipped is never
      // treated as passed.
      const firstDetail = report.results[0]?.detail ?? "no probe executed";
      console.log(
        `SANDBOX CONFORMANCE: SKIPPED - ${report.skipped} probes could not execute (backend: ${report.backendId}, profile: ${report.profileId}).`,
      );
      console.log(`  ${firstDetail}`);
      console.log("The conformance suite did not run; a skipped suite is never treated as passed.");
      return 0;
    }
    console.log(`SANDBOX CONFORMANCE (backend: ${report.backendId}, profile: ${report.profileId})`);
    for (const result of report.results) {
      const mark =
        result.outcome === "passed" ? "PASS" : result.outcome === "skipped" ? "SKIP" : "FAIL";
      console.log(`  [${mark}] ${result.probeId}: ${result.description}`);
      console.log(`        ${result.detail}`);
    }
    console.log(
      `Result: ${report.passed} passed, ${report.failed} failed, ${report.skipped} skipped.`,
    );
    return report.failed > 0 ? 1 : 0;
  } finally {
    await backend.close().catch(() => {});
    await removeConformanceArtifacts(workspaceRoot);
  }
}

main().then(
  (exitCode) => {
    process.exitCode = exitCode;
  },
  (error) => {
    console.error(`Sandbox conformance failed to run: ${describeError(error)}`);
    process.exitCode = 1;
  },
);

function describeError(error) {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "Unknown error.";
}
