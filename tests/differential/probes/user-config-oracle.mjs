import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { loadUserConfig } from "../../../packages/adapters/src/config/user-config.ts";
import {
  readConfigurationDiagnostics,
  readConfigurationFileState,
} from "../../../packages/adapters/src/config/config-diagnostics.ts";
import { parseReferenceDeclarationsSection } from "../../../packages/core/src/reference/reference-declaration.ts";

const MAX_CONFIG_FILE_BYTES = 1024 * 1024;

function contentFor(mode) {
  if (mode === "full") {
    return JSON.stringify({
      sandbox: { profile: "develop-offline", backend: "anthropic-runtime" },
      godot: {
        activeInstallation: "stable",
        discoverOnPath: false,
        installations: {
          stable: { path: "/opt/godot", editionHint: "standard" },
        },
      },
      quality: { reviewProvider: "deterministic-fake" },
      references: {
        aa: { kind: "local-directory", path: "/srv/assets", description: "Assets" },
        bb: {
          kind: "repository",
          repository: "godotengine/godot",
          ref: { kind: "commit", commit: "0123456" },
        },
      },
    });
  }
  if (mode === "unknown-top") return JSON.stringify({ permissions: {} });
  if (mode === "unknown-nested") return JSON.stringify({ sandbox: { credential: "secret" } });
  if (mode === "invalid-profile") return JSON.stringify({ sandbox: { profile: "full-access" } });
  if (mode === "invalid-backend") return JSON.stringify({ sandbox: { backend: "docker" } });
  if (mode === "invalid-edition") {
    return JSON.stringify({
      godot: { installations: { stable: { path: "/opt/godot", editionHint: "mono" } } },
    });
  }
  if (mode === "installations-bound") {
    const installations = {};
    for (let index = 0; index < 17; index += 1) {
      installations[`g${String(index).padStart(2, "0")}`] = { path: "/opt/godot" };
    }
    return JSON.stringify({ godot: { installations } });
  }
  if (mode === "references-bound") {
    const references = {};
    for (let index = 0; index < 17; index += 1) {
      references[`r${String(index).padStart(2, "0")}`] = {
        kind: "local-directory",
        path: "/srv/assets",
      };
    }
    return JSON.stringify({ references });
  }
  if (mode === "invalid-godot-path") {
    return JSON.stringify({
      godot: { installations: { stable: { path: "relative/godot" } } },
    });
  }
  if (mode === "invalid-provider")
    return JSON.stringify({ quality: { reviewProvider: "reviewer" } });
  if (mode === "invalid-json") return "{not valid json";
  if (mode === "exact-boundary") return `{}${" ".repeat(MAX_CONFIG_FILE_BYTES - 2)}`;
  if (mode === "over-boundary") return `{}${" ".repeat(MAX_CONFIG_FILE_BYTES - 1)}`;
  if (mode === "invalid-reference-path") {
    return JSON.stringify({ references: { aa: { kind: "local-directory", path: "relative" } } });
  }
  if (mode === "invalid-repository") {
    return JSON.stringify({
      references: { aa: { kind: "repository", repository: "https://example.com/org/repo" } },
    });
  }
  return null;
}

function setupCase(mode, configPath) {
  if (mode === "missing") return;
  if (mode === "directory") {
    mkdirSync(configPath);
    return;
  }
  if (mode === "symlink") {
    const target = join(configPath, "..", "target.json");
    writeFileSync(target, "{}");
    symlinkSync(target, configPath);
    return;
  }
  const content = contentFor(mode);
  if (content === null) throw new Error(`unknown user-config fixture mode ${mode}`);
  writeFileSync(configPath, content);
}

function transformReferencesSection(references) {
  const section = {};
  for (const [alias, declaration] of Object.entries(references)) {
    const description =
      typeof declaration.description === "string" ? { description: declaration.description } : {};
    if (declaration.kind === "local-directory") {
      section[alias] = {
        alias,
        kind: "local-directory",
        source: { kind: "local-directory", path: declaration.path },
        ...description,
      };
    } else {
      section[alias] = {
        alias,
        kind: "repository",
        source: {
          kind: "repository",
          repository: declaration.repository,
          ...(declaration.ref === undefined ? {} : { ref: declaration.ref }),
        },
        ...description,
      };
    }
  }
  return section;
}

function referenceConfigError(config) {
  const parsed = parseReferenceDeclarationsSection(transformReferencesSection(config.references));
  return parsed.ok ? null : parsed.reason;
}

function errorCategory(message) {
  if (message.includes("not a regular file")) return "NOT_REGULAR";
  if (
    message.includes("exceeds the 1048576-byte limit") ||
    message.includes("could not be read within the 1048576-byte limit")
  ) {
    return "TOO_LARGE";
  }
  if (message.includes("not valid JSON")) return "INVALID_JSON";
  if (message.includes("not valid UTF-8")) return "INVALID_UTF8";
  if (message.startsWith("Cannot read Siralos configuration")) return "CANNOT_READ";
  return "INVALID_VALUE";
}

function diagnosticsValue(diagnostics, fileState) {
  return {
    loaded: diagnostics.loaded,
    sections: diagnostics.sections,
    unknownFields: diagnostics.unknownFields,
    validationErrors: diagnostics.validationErrors.map(errorCategory),
    credentialRefs: diagnostics.credentialRefs,
    overrideInUse: diagnostics.overrideInUse,
    fileState,
  };
}

async function runCase(entry) {
  const directory = mkdtempSync(join(tmpdir(), "siralos-r7-4-user-config-"));
  const configPath = join(directory, "config.json");
  try {
    setupCase(entry.mode, configPath);
    const diagnostics = await readConfigurationDiagnostics(configPath);
    const fileState = await readConfigurationFileState(configPath);
    try {
      const config = await loadUserConfig(configPath);
      const reviewProviderId = config.quality.reviewProvider ?? "deterministic-fake";
      if (reviewProviderId !== "deterministic-fake") {
        return {
          status: "error",
          category: "UNKNOWN_REVIEW_PROVIDER",
          diagnostics: diagnosticsValue(diagnostics, fileState),
        };
      }
      return {
        status: "ok",
        config,
        reviewProviderId,
        referenceConfigError: referenceConfigError(config),
        diagnostics: diagnosticsValue(diagnostics, fileState),
      };
    } catch (error) {
      return {
        status: "error",
        category: errorCategory(error instanceof Error ? error.message : String(error)),
        diagnostics: diagnosticsValue(diagnostics, fileState),
      };
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

const input = JSON.parse(readFileSync(0, "utf8"));
const results = await Promise.all(input.cases.map(runCase));
process.stdout.write(JSON.stringify({ cases: results }));
