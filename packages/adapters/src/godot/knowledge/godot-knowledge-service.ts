import {
  classifyGodotManualChannel,
  KNOWLEDGE_SCHEMA_VERSION,
  type GodotApplicationEvent,
  type GodotEngineProfile,
  type GodotInstallation,
  type GodotKnowledge,
  type GodotKnowledgeBase,
  type GodotKnowledgeLookupResult,
  type GodotKnowledgeProfileV1,
  type GodotKnowledgeQueryResult,
  type GodotKnowledgeRefreshResult,
  type GodotKnowledgeStatus,
  type GodotKnowledgeSupport,
  type GodotProbeRunner,
  type GodotSelectionPreference,
  type SandboxBackend,
} from "@solaris/core";
import { join } from "node:path";
import { homedir } from "node:os";
import type { UserGodotConfig } from "../../config/user-config.js";
import type { GodotEngineProfileCache } from "../cache/engine-profile-cache.js";
import type { GodotKnowledgeCache } from "./knowledge-cache.js";
import { createGodotEngineProfiler, type GodotEngineProfiler } from "../profile/engine-profiler.js";
import {
  createGodotKnowledgeRunner,
  GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE,
  type GodotKnowledgeRunner,
} from "../process/godot-knowledge-runner.js";
import { parseGodotVersionText } from "../process/version-parser.js";
import { parseGodotApiDumpWithDocs } from "./api-dump-with-docs.js";
import { buildGodotApiIndex, searchGodotApiIndex, lookupGodotApiSymbol } from "./api-index.js";

/** Solaris-private probe-directory root for API documentation generation. */
export const GODOT_KNOWLEDGE_PROBE_ROOT = "godot-knowledge-probe";

const MAX_QUERY_LENGTH = 4096;
const MAX_SYMBOL_LENGTH = 1024;

export interface GodotKnowledgeServiceDependencies {
  readonly workspaceRoot: string;
  readonly config: UserGodotConfig;
  readonly preference: GodotSelectionPreference;
  readonly overrideSource: "cli" | "environment" | null;
  readonly backend: SandboxBackend;
  readonly probeRunner: GodotProbeRunner;
  /** Knowledge cache; explicitly unavailable at this stage. */
  readonly cache: GodotKnowledgeCache;
  /** Engine-profile cache for the internal profiler (unavailable no-op). */
  readonly engineProfileCache: GodotEngineProfileCache;
  readonly hostPath: string | null;
  readonly hostPathExt: string | null;
  readonly platform: NodeJS.Platform;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment: Readonly<Record<string, string>>;
  readonly onEvent?: (event: GodotApplicationEvent) => void;
  /** Generation runner; production wires the fail-closed runner. */
  readonly generationRunner?: GodotKnowledgeRunner;
  /** Test seam: a fully loaded knowledge base (production never supplies it). */
  readonly knowledgeBase?: GodotKnowledgeBase;
}

/**
 * Version-matched Godot API knowledge service.
 *
 * `refresh` regenerates the exact-engine API documentation profile
 * (`--dump-extension-api-with-docs` in a Solaris-private probe directory)
 * and replaces the loaded knowledge base only after a successful complete
 * generation; cancellation leaves any previous base intact. `search` and
 * `lookup` serve bounded structured results from the loaded base and never
 * expose the raw dump. On this stage generation fails closed (the runner
 * never spawns the executable), so the production service always reports
 * `unavailable` and no probe directory is ever created.
 */
export function createGodotKnowledgeService(
  dependencies: GodotKnowledgeServiceDependencies,
): GodotKnowledge {
  const profiler: GodotEngineProfiler = createGodotEngineProfiler({
    ...dependencies,
    cache: dependencies.engineProfileCache,
  });
  const generationRunner: GodotKnowledgeRunner =
    dependencies.generationRunner ?? createGodotKnowledgeRunner({ backend: dependencies.backend });
  let loadedBase: GodotKnowledgeBase | null = dependencies.knowledgeBase ?? null;

  async function sandboxEnforced(): Promise<boolean> {
    let status;
    try {
      status = await dependencies.backend.inspect();
    } catch {
      return false;
    }
    return (
      status.state === "available" &&
      status.capabilities.filesystemReadRestriction &&
      status.capabilities.filesystemWriteRestriction &&
      status.capabilities.networkRestriction &&
      status.capabilities.processTreeRestriction
    );
  }

  async function support(): Promise<GodotKnowledgeSupport> {
    const available = (await generationRunner.isAvailable()) && (await sandboxEnforced());
    return {
      state: available ? "available" : "unavailable",
      reason: available ? null : GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE,
      platform: dependencies.platform,
    };
  }

  async function refresh(signal?: AbortSignal): Promise<GodotKnowledgeRefreshResult> {
    try {
      if (signal?.aborted) {
        throw createAbortError();
      }
      if (!(await generationRunner.isAvailable())) {
        return {
          status: "unavailable",
          message: GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE,
        };
      }
      const selection = await profiler.selectedProfile(signal);
      if (selection === null) {
        return {
          status: "unsupported",
          message: "No trusted Godot installation is selected; API knowledge cannot be generated.",
        };
      }
      emit("godot_probe_started", selection.installation.id, "knowledge");
      const probeDirectory = join(
        homedir(),
        ".solaris",
        "godot",
        GODOT_KNOWLEDGE_PROBE_ROOT,
        selection.installation.sha256.slice(0, 16),
      );
      const outcome = await generationRunner.generateDocumentation({
        installation: selection.installation,
        engineProfile: selection.profile,
        probeDirectory,
        ...(signal === undefined ? {} : { signal }),
      });
      if (outcome.status === "unsupported") {
        emit("godot_probe_completed", selection.installation.id, "knowledge", "failed");
        return { status: "unsupported", message: outcome.message };
      }
      if (outcome.status !== "completed") {
        emit("godot_probe_completed", selection.installation.id, "knowledge", "failed");
        if (outcome.status === "cancelled") {
          return { status: "cancelled", message: outcome.message };
        }
        return { status: "failed", message: outcome.message };
      }
      if (signal?.aborted) {
        throw createAbortError();
      }
      const built = await buildKnowledgeBase(
        selection.installation,
        selection.profile,
        probeDirectory,
        signal,
      );
      if (!built.ok) {
        emit("godot_probe_completed", selection.installation.id, "knowledge", "failed");
        return { status: "failed", message: built.message };
      }
      const previousProfile = loadedBase?.profile ?? null;
      const stored = await dependencies.cache.store(built.base);
      if (!stored.ok) {
        emit("godot_probe_completed", selection.installation.id, "knowledge", "failed");
        return {
          status: "failed",
          message:
            "The generated knowledge profile could not be stored; the previous profile remains intact.",
        };
      }
      loadedBase = built.base;
      emit("godot_probe_completed", selection.installation.id, "knowledge", "success");
      return { status: "ready", profile: built.base.profile, previousProfile };
    } catch (error: unknown) {
      if (isAbortError(error)) {
        return { status: "cancelled", message: "API knowledge generation was cancelled." };
      }
      return { status: "failed", message: describeError(error) };
    }
  }

  function search(
    query: import("@solaris/core").GodotApiSearchQuery,
    signal?: AbortSignal,
  ): Promise<GodotKnowledgeQueryResult> {
    if (signal?.aborted) {
      return Promise.resolve({ status: "cancelled", message: "API search was cancelled." });
    }
    if (typeof query.query !== "string" || query.query.trim().length === 0) {
      return Promise.resolve({
        status: "invalid_input",
        message: "A non-empty query is required.",
      });
    }
    if (query.query.length > MAX_QUERY_LENGTH) {
      return Promise.resolve({
        status: "invalid_input",
        message: `The query exceeds the ${MAX_QUERY_LENGTH}-character bound.`,
      });
    }
    if (query.kinds !== undefined && !isKindArray(query.kinds)) {
      return Promise.resolve({
        status: "invalid_input",
        message: "The kinds filter contains an unknown symbol kind.",
      });
    }
    if (query.limit !== undefined && (!Number.isInteger(query.limit) || query.limit < 1)) {
      return Promise.resolve({
        status: "invalid_input",
        message: "The result limit must be a positive integer.",
      });
    }
    if (loadedBase === null) {
      return Promise.resolve({
        status: "unavailable",
        message:
          "No Godot API knowledge is loaded: exact-engine API generation is unavailable on this platform.",
      });
    }
    const outcome = searchGodotApiIndex(loadedBase.index, query.query, {
      ...(query.kinds === undefined ? {} : { kinds: query.kinds }),
      ...(query.limit === undefined ? {} : { limit: query.limit }),
    });
    return Promise.resolve({
      status: "ready",
      engineVersion: loadedBase.profile.engine.godotVersion,
      results: outcome.results,
      truncated: outcome.truncated,
    });
  }

  function lookup(symbol: string, signal?: AbortSignal): Promise<GodotKnowledgeLookupResult> {
    if (signal?.aborted) {
      return Promise.resolve({ status: "cancelled", message: "API lookup was cancelled." });
    }
    if (typeof symbol !== "string" || symbol.trim().length === 0) {
      return Promise.resolve({
        status: "invalid_input",
        message: "A non-empty symbol identity is required.",
      });
    }
    if (symbol.length > MAX_SYMBOL_LENGTH) {
      return Promise.resolve({
        status: "invalid_input",
        message: `The symbol identity exceeds the ${MAX_SYMBOL_LENGTH}-character bound.`,
      });
    }
    if (loadedBase === null) {
      return Promise.resolve({
        status: "unavailable",
        message:
          "No Godot API knowledge is loaded: exact-engine API generation is unavailable on this platform.",
      });
    }
    const result = lookupGodotApiSymbol(loadedBase.index, symbol);
    if (result === null) {
      return Promise.resolve({
        status: "not_found",
        message: `Unknown API symbol ${symbol}.`,
      });
    }
    return Promise.resolve({
      status: "ready",
      engineVersion: loadedBase.profile.engine.godotVersion,
      result,
    });
  }

  function status(): GodotKnowledgeStatus {
    if (loadedBase !== null) {
      const version = parseVersion(loadedBase.profile.engine.godotVersion);
      return {
        state: "ready",
        reason: null,
        platform: dependencies.platform,
        profile: loadedBase.profile,
        cacheEnabled: false,
        schemaVersion: KNOWLEDGE_SCHEMA_VERSION,
        manualChannel: version === null ? null : classifyGodotManualChannel(version),
      };
    }
    return {
      state: "unavailable",
      reason: GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE,
      platform: dependencies.platform,
      profile: null,
      cacheEnabled: false,
      schemaVersion: KNOWLEDGE_SCHEMA_VERSION,
      manualChannel: null,
    };
  }

  async function buildKnowledgeBase(
    installation: GodotInstallation,
    engineProfile: GodotEngineProfile,
    probeDirectory: string,
    signal: AbortSignal | undefined,
  ): Promise<
    | { readonly ok: true; readonly base: GodotKnowledgeBase }
    | { readonly ok: false; readonly message: string }
  > {
    const { readFile } = await import("node:fs/promises");
    const dumpPath = join(probeDirectory, "extension_api.json");
    let content: Buffer;
    try {
      content = await readFile(dumpPath);
    } catch {
      return { ok: false, message: "Godot did not produce the expected extension_api.json file." };
    }
    if (signal?.aborted) {
      throw createAbortError();
    }
    const parsed = parseGodotApiDumpWithDocs(content);
    if (!parsed.ok) {
      return { ok: false, message: parsed.message };
    }
    if (signal?.aborted) {
      throw createAbortError();
    }
    const built = buildGodotApiIndex(parsed.document);
    if (!built.ok) {
      return { ok: false, message: built.message };
    }
    const profile: GodotKnowledgeProfileV1 = {
      version: 1,
      engine: {
        installationId: installation.id,
        executableSha256: installation.sha256,
        godotVersion: engineProfile.version.raw,
        edition: engineProfile.edition,
      },
      api: {
        dumpSha256: parsed.document.sha256,
        generatedAt: new Date().toISOString(),
        classCount: parsed.document.classes.length,
        builtinClassCount: parsed.document.builtinClasses.length,
        utilityFunctionCount: parsed.document.utilityFunctions.length,
        globalEnumCount: parsed.document.globalEnums.length,
        globalConstantCount: parsed.document.globalConstants.length,
      },
      index: {
        schemaVersion: KNOWLEDGE_SCHEMA_VERSION,
        symbolCount: built.index.symbols.length,
      },
    };
    return { ok: true, base: { profile, index: built.index } };
  }

  function emit(
    type: "godot_probe_started" | "godot_probe_completed",
    installationId: string,
    probe: "knowledge",
    status?: "success" | "degraded" | "failed",
  ): void {
    if (dependencies.onEvent === undefined) {
      return;
    }
    if (type === "godot_probe_started") {
      dependencies.onEvent({ type, installationId, probe });
    } else if (status !== undefined) {
      dependencies.onEvent({ type, installationId, probe, status });
    }
  }

  return { support, refresh, search, lookup, status };
}

function isKindArray(value: unknown): value is readonly string[] {
  return (
    Array.isArray(value) &&
    value.length > 0 &&
    value.every((entry) => typeof entry === "string" && VALID_API_KINDS.has(entry))
  );
}

const VALID_API_KINDS: ReadonlySet<string> = new Set([
  "class",
  "method",
  "property",
  "signal",
  "constant",
  "enum",
  "utility",
  "operator",
]);

function parseVersion(raw: string): import("@solaris/core").GodotVersion | null {
  try {
    const parsed = parseGodotVersionText(raw);
    return parsed.ok ? parsed.version : null;
  } catch {
    return null;
  }
}

function createAbortError(): Error {
  return new DOMException("The Godot knowledge operation was aborted.", "AbortError");
}

function isAbortError(error: unknown): boolean {
  return error instanceof Error && (error.name === "AbortError" || error.name === "DOMException");
}

function describeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
