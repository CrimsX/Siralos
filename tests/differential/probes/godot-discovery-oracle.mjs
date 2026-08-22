/**
 * godot-discovery oracle probe (differential harness, ADR 0033,
 * Stage 3R R8).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes discovery scenarios against the REAL TypeScript reference
 * profiler (createGodotEngineProfiler): one discovery generation over
 * the scenario-declared config and sanitized host PATH/PATHEXT inputs,
 * or the selected-profile query. PATH entries in fixtures never point at
 * existing files, so outcomes are deterministic with zero filesystem
 * effects. Mirrors crates/siralos-cli/src/harness.rs::
 * godot_discovery_record.
 */
import { readFileSync } from "node:fs";
import { createGodotEngineProfiler } from "../../../packages/adapters/src/godot/profile/engine-profiler.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

function unavailableBackend() {
  return {
    id: "unavailable-backend",
    inspect() {
      return Promise.resolve({
        backendId: "unavailable-backend",
        state: "unavailable",
        platform: process.platform,
        version: "0.0.0",
      });
    },
    execute() {
      return Promise.reject(new Error("the oracle probe must never execute"));
    },
    close() {
      return Promise.resolve();
    },
  };
}

function readPreference(value) {
  if (value === undefined || value === null || value === "auto") {
    return { kind: "auto" };
  }
  if (value === "none") {
    return { kind: "none" };
  }
  if (value === "config-active") {
    return { kind: "config-active" };
  }
  if (value !== null && typeof value === "object" && typeof value.path === "string") {
    return { kind: "path", path: value.path };
  }
  if (value !== null && typeof value === "object" && typeof value.installationId === "string") {
    return { kind: "installation-id", installationId: value.installationId };
  }
  throw new Error("preference must be auto, none, config-active, path, or installationId");
}

function readConfig(value) {
  const installations = {};
  for (const entry of value?.installations ?? []) {
    installations[entry.id] = {
      path: entry.path,
      ...(entry.editionHint === undefined ? {} : { editionHint: entry.editionHint }),
    };
  }
  return {
    activeInstallation: value?.activeInstallation ?? null,
    installations,
    discoverOnPath: value?.discoverOnPath === true,
  };
}

function overviewRecord(overview) {
  if (overview === null || overview === undefined) {
    return null;
  }
  return {
    id: overview.installationId,
    sourceLabel: overview.sourceLabel,
    source: overview.source,
    invalid: overview.invalid ?? null,
    isDuplicate: overview.isDuplicate,
    selected: overview.selected,
  };
}

const input = readStdinBounded();
const platform = typeof input.platform === "string" ? input.platform : "win32";
const profiler = createGodotEngineProfiler({
  config: readConfig(input.config),
  preference: readPreference(input.preference),
  overrideSource: input.overrideSource === "cli" ? "cli" : "environment",
  workspaceRoot:
    typeof input.workspaceRoot === "string" ? input.workspaceRoot : "/siralos-differential",
  backend: unavailableBackend(),
  probeRunner: { isAvailable: () => Promise.resolve(false) },
  cache: {
    load: () => Promise.resolve(null),
    store: () => Promise.resolve({ ok: false, reason: "unavailable", message: "unavailable" }),
    count: () => Promise.resolve(0),
  },
  hostPath: typeof input.hostPath === "string" ? input.hostPath : null,
  hostPathExt: typeof input.hostPathExt === "string" ? input.hostPathExt : null,
  platform,
});

let outcome;
switch (input.op) {
  case "discover": {
    try {
      const result = await profiler.discover();
      outcome = {
        ok: true,
        selected: overviewRecord(result.selected ?? null),
        candidates: result.candidates.map(overviewRecord),
        configuration: {
          activeInstallation: result.configuration.activeInstallation ?? null,
          configuredCount: result.configuration.configuredCount,
          discoverOnPath: result.configuration.discoverOnPath,
          overrides: [...result.configuration.overrides],
        },
        rationale: [...result.rationale],
        diagnostics: result.diagnostics.map((diagnostic) => ({
          severity: diagnostic.severity,
          message: diagnostic.message,
        })),
      };
    } catch (error) {
      outcome = { ok: false, error: error.message };
    }
    break;
  }
  case "select": {
    try {
      const selected = await profiler.selectedProfile();
      outcome = { ok: true, selected: selected !== null };
    } catch (error) {
      outcome = { ok: false, error: error.message };
    }
    break;
  }
  default:
    throw new Error(`unknown godot-discovery op ${String(input.op)}`);
}

process.stdout.write(JSON.stringify(outcome));
