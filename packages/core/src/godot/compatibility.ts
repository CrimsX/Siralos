import type { GodotEngineProfile } from "./engine-profile.js";
import type { GodotProjectProfile } from "./project.js";

export type GodotCompatibilityStatus =
  | "compatible"
  | "likely-compatible"
  | "engine-older-than-project"
  | "major-version-mismatch"
  | "edition-mismatch"
  | "project-version-unknown"
  | "engine-unverified"
  | "no-engine"
  | "no-project";

export interface GodotCompatibilityAssessment {
  readonly status: GodotCompatibilityStatus;
  readonly severity: "info" | "warning" | "error";
  readonly reasons: readonly string[];
}

/**
 * Conservative static comparison between the selected engine profile and
 * the static project profile. Compatibility is never claimed as
 * guaranteed; every assessment explains itself and preserves uncertainty.
 * Declared project versions are non-authoritative.
 */
export function assessGodotCompatibility(
  engine: GodotEngineProfile | null,
  project: GodotProjectProfile,
): GodotCompatibilityAssessment {
  if (!project.detected) {
    return {
      status: "no-project",
      severity: "info",
      reasons: ["No project.godot exists at the workspace root; nothing to compare."],
    };
  }
  if (engine === null) {
    return {
      status: "no-engine",
      severity: "warning",
      reasons: [
        "The project was detected, but no trusted Godot installation is selected; compatibility cannot be assessed.",
      ],
    };
  }
  const reasons: string[] = [];
  if (engine.support === "verified") {
    reasons.push(`Solaris verified support: ${engine.version.raw} standard editor.`);
  } else {
    reasons.push(`Solaris support: ${engine.support} (${engine.version.raw}).`);
  }
  const declared = project.declaredEngineVersion;
  if (declared === null) {
    reasons.push("The project declares no engine feature; static compatibility is unknown.");
    return {
      status: "project-version-unknown",
      severity: "warning",
      reasons,
    };
  }
  if (engine.version.major < declared.major) {
    reasons.push(
      `The engine major (${engine.version.major}) is lower than the declared project major (${declared.major}).`,
    );
    return {
      status: "major-version-mismatch",
      severity: "error",
      reasons,
    };
  }
  if (engine.version.major > declared.major) {
    reasons.push(
      `The engine major (${engine.version.major}) is newer than the declared project major (${declared.major}); migration-sensitive.`,
    );
    return { status: "likely-compatible", severity: "warning", reasons };
  }
  if (engine.version.minor < declared.minor) {
    reasons.push(
      `The engine minor (${engine.version.minor}) is older than the declared project minor (${declared.minor}).`,
    );
    return {
      status: "engine-older-than-project",
      severity: "error",
      reasons,
    };
  }
  if (engine.version.minor > declared.minor) {
    reasons.push(
      `The engine minor (${engine.version.minor}) is newer than the declared project minor (${declared.minor}); migration-sensitive.`,
    );
    return { status: "likely-compatible", severity: "warning", reasons };
  }
  if (
    engine.support === "unsupported-major" ||
    engine.support === "invalid" ||
    engine.support === "runtime-only"
  ) {
    reasons.push("The selected engine is not a supported editor for Solaris.");
    return { status: "engine-unverified", severity: "error", reasons };
  }
  if (engine.support !== "verified" && engine.support !== "compatible-untested") {
    reasons.push("The selected engine build is unverified for Solaris.");
    return { status: "engine-unverified", severity: "warning", reasons };
  }
  if (project.languageProfile === "dotnet" && engine.edition === "standard") {
    reasons.push(
      "The project uses .NET, but the selected engine is the standard (non-.NET) editor.",
    );
    return { status: "edition-mismatch", severity: "error", reasons };
  }
  if (project.languageProfile === "gdscript" && engine.edition === "dotnet") {
    reasons.push(
      "The project appears GDScript-only; a .NET engine is selected and remains unverified for Solaris.",
    );
    return { status: "likely-compatible", severity: "warning", reasons };
  }
  reasons.push(
    `The engine (${engine.version.raw}) matches the declared project version (${declared.raw}) within the same minor line.`,
  );
  return { status: "compatible", severity: "info", reasons };
}
