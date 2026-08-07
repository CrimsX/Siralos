import type { GodotCommandCapabilities } from "./capabilities.js";
import type { SafeDiagnostic } from "./diagnostics.js";
import type { GodotInstallation } from "./installations.js";
import type { GodotReleaseChannel, GodotVersion } from "./version.js";

/** Conservative edition classification. `mono` in a filename is never proof of .NET. */
export type GodotEdition = "standard" | "dotnet" | "editor-unknown" | "runtime-only" | "unknown";

export type GodotEditionConfidence = "high" | "medium" | "low";

/**
 * Solaris's tested support classification for an installation. This reflects
 * Solaris's verified support, not Godot's official support status. It is
 * never derived from internet data.
 */
export type SolarisGodotSupport =
  | "verified"
  | "compatible-untested"
  | "prerelease-untested"
  | "custom-build-untested"
  | "unsupported-major"
  | "runtime-only"
  | "invalid";

/** Structured evidence the adapter gathers before core classifies an edition. */
export interface GodotEditionEvidence {
  readonly explicitHint: "standard" | "dotnet" | "unknown" | null;
  /** Canonical filename lowercased. */
  readonly filename: string;
  readonly capabilities: GodotCommandCapabilities;
  /** Features reported by the API dump header when available. */
  readonly apiConfigurationFeatures: readonly string[];
  readonly probesSucceeded: {
    readonly version: boolean;
    readonly help: boolean;
    readonly apiDump: boolean;
  };
}

export interface GodotEditionClassification {
  readonly edition: GodotEdition;
  readonly confidence: GodotEditionConfidence;
  /** Bounded evidence descriptions that led to the classification. */
  readonly evidence: readonly string[];
  /** Bounded conflicting evidence descriptions. */
  readonly conflicts: readonly string[];
}

/** Immutable engine profile produced for one valid installation. */
export interface GodotEngineProfile {
  readonly installationId: string;
  /** Short executable fingerprint (SHA-256 prefix), safe for providers. */
  readonly fingerprint: string;
  readonly version: GodotVersion;
  readonly edition: GodotEdition;
  readonly editionConfidence: GodotEditionConfidence;
  readonly releaseChannel: GodotReleaseChannel;
  readonly capabilities: GodotCommandCapabilities;
  /** Capabilities that were operationally verified by a successful probe. */
  readonly verifiedCapabilities: readonly string[];
  /** Capabilities that were advertised but failed their operational probe. */
  readonly degradedCapabilities: readonly string[];
  readonly executableSha256: string;
  readonly apiDumpSha256: string | null;
  readonly support: SolarisGodotSupport;
  readonly diagnostics: readonly SafeDiagnostic[];
}

export interface GodotSupportClassificationInput {
  readonly version: GodotVersion;
  readonly edition: GodotEdition;
  readonly editionConfidence: GodotEditionConfidence;
  /** True only for the exact verified baseline (Godot 4.7.1 stable standard editor). */
  readonly isVerifiedBaseline: boolean;
}

/**
 * Initial support classification rules. The exact Godot 4.7.1 stable
 * standard editor is `verified`; other stable 4.7 standard editors are
 * `compatible-untested`; 4.7/4.8 prereleases and dev builds are
 * `prerelease-untested`; custom builds are `custom-build-untested`; Godot
 * 3.x is `unsupported-major`; .NET editions are detected but
 * `compatible-untested`; runtime-only binaries and invalid editions are
 * never selected for editor workflows.
 */
export function classifyGodotSupport(input: GodotSupportClassificationInput): SolarisGodotSupport {
  const { version, edition } = input;
  if (edition === "runtime-only") {
    return "runtime-only";
  }
  if (version.major < 4) {
    return "unsupported-major";
  }
  if (edition === "dotnet") {
    return "compatible-untested";
  }
  if (version.status === "custom") {
    return "custom-build-untested";
  }
  if (
    version.status === "dev" ||
    version.status === "rc" ||
    version.status === "beta" ||
    version.status === "alpha"
  ) {
    return "prerelease-untested";
  }
  if (version.status === "unknown") {
    return "prerelease-untested";
  }
  if (input.isVerifiedBaseline && version.status === "stable") {
    return "verified";
  }
  return "compatible-untested";
}

/** `main`/`dotnet`/`editor-unknown` variants that cannot be selected as an editor. */
export function isEditorSelectionCandidate(profile: GodotEngineProfile): boolean {
  return (
    profile.edition === "standard" ||
    profile.edition === "dotnet" ||
    profile.edition === "editor-unknown"
  );
}

/**
 * Conservative edition classification. Evidence rules:
 *
 * - A `.NET` edition is never claimed solely from a `mono` filename: the
 *   user hint, filename, `--build-solutions` advertisement, API dump
 *   features, and successful probes are combined, and confidence is high
 *   only when several independent signals agree.
 * - `standard` is never claimed solely because a filename lacks `mono`.
 * - Conflicting evidence lowers confidence and is reported.
 * - A successful `--version` probe with a failed `--help` probe leaves the
 *   edition unknown (the binary cannot be characterized).
 * - A successful help probe without any editor signal is classified as
 *   `runtime-only` (a heuristic; export templates and runtime binaries must
 *   not be selected for editor workflows).
 */
export function classifyGodotEdition(evidence: GodotEditionEvidence): GodotEditionClassification {
  const conflicts: string[] = [];
  if (!evidence.probesSucceeded.version) {
    return {
      edition: "unknown",
      confidence: "low",
      evidence: ["the version probe did not succeed"],
      conflicts: [],
    };
  }
  const dotnetSignals: string[] = [];
  const standardSignals: string[] = [];
  const filename = evidence.filename.toLowerCase();
  if (evidence.explicitHint === "dotnet") {
    dotnetSignals.push("explicit user edition hint: dotnet");
  }
  if (evidence.explicitHint === "standard") {
    standardSignals.push("explicit user edition hint: standard");
  }
  if (filename.includes("mono") || filename.includes("dotnet")) {
    dotnetSignals.push(`canonical filename contains a .NET marker`);
  }
  if (evidence.capabilities.buildSolutions) {
    dotnetSignals.push("advertises --build-solutions");
  }
  if (
    evidence.apiConfigurationFeatures.some((feature) =>
      ["dotnet", "mono", "csharp", "managed"].includes(feature),
    )
  ) {
    dotnetSignals.push("API dump configuration reports .NET features");
  }
  if (evidence.explicitHint === "standard" && dotnetSignals.length > 0) {
    conflicts.push("the user hint says standard while other evidence suggests .NET");
  }
  if (
    evidence.explicitHint === "dotnet" &&
    filename.includes("mono") === false &&
    filename.includes("dotnet") === false
  ) {
    conflicts.push("the user hint says dotnet while the filename shows no .NET marker");
  }
  const editorSignals =
    evidence.capabilities.editor ||
    evidence.capabilities.projectManager ||
    evidence.capabilities.extensionApiDump;
  if (!evidence.probesSucceeded.help) {
    return {
      edition: "unknown",
      confidence: "low",
      evidence: ["the help probe did not succeed, so the binary cannot be characterized"],
      conflicts,
    };
  }
  if (dotnetSignals.length > 0) {
    const confident =
      standardSignals.length === 0 &&
      (dotnetSignals.length >= 2 || evidence.explicitHint === "dotnet");
    return {
      edition: "dotnet",
      confidence: confident ? "high" : "medium",
      evidence: dotnetSignals,
      conflicts,
    };
  }
  if (!editorSignals) {
    if (standardSignals.length > 0) {
      conflicts.push(
        "the user hint says standard, but the help output advertises no editor signal",
      );
      return {
        edition: "standard",
        confidence: "medium",
        evidence: standardSignals,
        conflicts,
      };
    }
    return {
      edition: "runtime-only",
      confidence: "medium",
      evidence: [
        "the help probe succeeded but no editor signal is advertised (heuristic runtime-only inference)",
      ],
      conflicts,
    };
  }
  if (standardSignals.length > 0) {
    return {
      edition: "standard",
      confidence: evidence.probesSucceeded.apiDump ? "high" : "medium",
      evidence: standardSignals,
      conflicts,
    };
  }
  return {
    edition: "editor-unknown",
    confidence: "medium",
    evidence: ["editor signals are advertised, but no positive standard or .NET evidence exists"],
    conflicts,
  };
}

export function describeInstallationProvenance(installation: GodotInstallation): string {
  return `${installation.sourceLabel} (${installation.id})`;
}
