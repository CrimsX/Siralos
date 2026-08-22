/**
 * godot-diagnostics oracle probe (differential harness, ADR 0033,
 * Stage 3R R8).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes diagnostics scenarios against the REAL TypeScript reference
 * service (createGodotDiagnosticsService over the fail-closed production
 * mirror/check runners): support, prepare refusal/cancellation, unknown
 * prepared-check execution failure, and bounded session state. Thin,
 * bounded, no engine; mirrors crates/siralos-cli/src/harness.rs::
 * godot_diagnostics_record.
 */
import { readFileSync } from "node:fs";
import { createGodotDiagnosticsService } from "../../../packages/adapters/src/godot/diagnostics/godot-diagnostics-service.js";

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

const input = readStdinBounded();
const platform = typeof input.platform === "string" ? input.platform : "win32";
const dependencies = {
  workspaceRoot:
    typeof input.workspaceRoot === "string" ? input.workspaceRoot : "/siralos-differential",
  config: readConfig(input.config),
  preference: readPreference(input.preference),
  overrideSource: input.overrideSource === "cli" ? "cli" : "environment",
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
  parentEnvironment: {},
  checkpointRoot: null,
};

const service = createGodotDiagnosticsService(dependencies);

async function cancelledPrepare(paths) {
  const controller = new AbortController();
  controller.abort();
  try {
    return await service.prepare({ ...(paths === undefined ? {} : { paths }) }, controller.signal);
  } catch (error) {
    return { status: "cancelled", message: error.message };
  }
}

let outcome;
switch (input.op) {
  case "support":
    outcome = await service.support();
    break;
  case "prepare": {
    const paths = input.paths === undefined || input.paths === null ? undefined : input.paths;
    outcome =
      input.cancelled === true
        ? await cancelledPrepare(paths)
        : await service.prepare(paths === undefined ? {} : { paths });
    break;
  }
  case "execute": {
    const result = await service.execute(
      { handle: 1 },
      {
        approvedDigest:
          typeof input.approvedDigest === "string" ? input.approvedDigest : "a".repeat(64),
      },
    );
    outcome =
      result.status === "ready"
        ? (() => {
            throw new Error("diagnostics execute cannot become ready without an engine");
          })()
        : { status: result.status, message: result.message };
    break;
  }
  case "status":
    outcome = { state: service.status().state };
    break;
  default:
    throw new Error(`unknown godot-diagnostics op ${String(input.op)}`);
}

process.stdout.write(JSON.stringify(outcome));
