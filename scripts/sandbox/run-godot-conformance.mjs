#!/usr/bin/env node

import { isAbsolute } from "node:path";

/**
 * Live Godot conformance.
 *
 * Engine probing fails closed at this stage: Node and the pinned sandbox
 * runtime offer no identity-bound launch primitive, so the staged
 * executable copy's pathname is re-opened at spawn time and a same-user
 * process can substitute different bytes between final verification and
 * launch. The verified fingerprint could then be attached to bytes that
 * never execute. The probe runner therefore reports probing unavailable
 * and never spawns the executable, so no live probe can be verified.
 *
 * This suite reports that state loudly and never represents it as passed.
 * The user-supplied Godot executable is never read or modified here.
 */
async function main() {
  const godotPath = process.env["SOLARIS_TEST_GODOT"];
  if (godotPath === undefined || godotPath.trim().length === 0) {
    console.log("GODOT CONFORMANCE: SKIPPED - SOLARIS_TEST_GODOT is not set.");
    console.log(
      "No live Godot probes ran; skipped or unavailable is never treated as a live security pass.",
    );
    return 0;
  }
  if (!isAbsolute(godotPath.trim())) {
    console.log(
      "GODOT CONFORMANCE: SKIPPED - SOLARIS_TEST_GODOT must be an absolute path to a Godot executable, for example:",
    );
    console.log('  $env:SOLARIS_TEST_GODOT = "C:\\absolute\\path\\to\\godot.exe"');
    console.log("  npm run test:godot");
    console.log(
      "No live Godot probes ran; skipped or unavailable is never treated as a live security pass.",
    );
    return 0;
  }
  console.log("GODOT CONFORMANCE: UNAVAILABLE - Godot engine probing fails closed at this stage.");
  console.log(
    "The pinned Node runtime cannot bind a sandboxed launch to the exact fingerprinted executable bytes:",
  );
  console.log(
    "  the backend re-opens the staged copy's pathname at spawn time, and a same-user process can",
  );
  console.log(
    "  substitute different bytes between final verification and launch, executing unverified",
  );
  console.log(
    "  content under a recorded trusted fingerprint. Post-launch verification is not prevention.",
  );
  console.log(
    "The probe runner reports probing unavailable and never spawns the executable, so live",
  );
  console.log(
    "probes cannot be verified at this stage; this is never treated as a live security pass.",
  );
  console.log(
    `SOLARIS_TEST_GODOT was set (${godotPath.trim().length} characters) but was not executed or modified.`,
  );
  return 0;
}

main().then(
  (exitCode) => {
    process.exitCode = exitCode;
  },
  (error) => {
    console.error(
      `GODOT CONFORMANCE FAILED: ${error instanceof Error ? error.message : String(error)}`,
    );
    process.exitCode = 1;
  },
);
