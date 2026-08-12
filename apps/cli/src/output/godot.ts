import type {
  ApprovalRequest,
  GDScriptCompletionResult,
  GDScriptDefinitionResult,
  GDScriptHoverResult,
  GDScriptLSPSessionPreview,
  GDScriptQueryOutcome,
  GDScriptSessionStatus,
  GodotCompatibilityAssessment,
  GodotDiagnosticPreview,
  GodotDiscoveryResult,
  GodotDoctorReport,
  GodotKnowledgeQueryResult,
  GodotKnowledgeStatus,
  GodotProjectCheckResult,
  GodotProjectProfile,
  GodotProjectProbeStatus,
  GodotProbePreview,
  GodotRecoveryProbeResult,
  GodotSelectedInstallation,
} from "@siralos/core";
import { formatBytes, formatDuration, formatFileCount, yesNo } from "./format-utils.js";
import { sanitizeForDisplay } from "./sanitize.js";

export function formatGodotDiagnosticPreview(preview: GodotDiagnosticPreview): string {
  const scripts = preview.scripts;
  const scope =
    scripts.paths !== null && scripts.paths.length > 0
      ? scripts.paths.map((entry) => `  ${sanitizeForDisplay(entry)}`).join("\n")
      : `  ${scripts.count} project scripts`;
  const lines = [
    "GDScript diagnostic probe",
    "",
    "Engine:",
    `  ${preview.engineVersion}`,
    `  ${preview.engineEdition} edition`,
    `  Siralos support: ${preview.support}`,
    `  Static compatibility: ${preview.compatibility}`,
    "",
    "Project:",
    `  ${preview.projectName ?? "(unnamed)"}`,
    "",
    "Scripts:",
    scope,
    `  ${formatBytes(scripts.totalBytes)} total`,
    "",
    "Operation:",
    "  Parse GDScript only (--check-only)",
    "",
    "Project source:",
    "  Disposable mirror",
    "",
    "Game execution:",
    "  disabled",
    "",
    "Scene execution:",
    "  disabled",
    "",
    "Network:",
    "  denied",
    "",
    "Provider credentials:",
    "  absent",
    "",
    "Project modifications:",
    "  none",
    "",
  ];
  return `${lines.join("\n")}\n`;
}

export function formatGodotDiagnosticApprovalPrompt(
  request: Extract<ApprovalRequest, { capability: "godot.diagnose" }>,
): string {
  const preview = request.preview;
  const scripts = preview.scripts;
  const scope =
    scripts.paths !== null && scripts.paths.length > 0
      ? scripts.paths.map((entry) => `  ${sanitizeForDisplay(entry)}`).join("\n")
      : `  ${scripts.count} project scripts`;
  const lines = [
    "GDScript diagnostic probe requires approval",
    "",
    "Engine:",
    `  ${preview.engineVersion}`,
    `  ${preview.engineEdition} edition`,
    `  Siralos support: ${preview.support}`,
    `  Static compatibility: ${preview.compatibility}`,
    "",
    "Project:",
    `  ${preview.projectName ?? "(unnamed)"}`,
    "",
    "Scripts:",
    scope,
    `  ${formatBytes(scripts.totalBytes)} total`,
    "",
    "Operation:",
    "  Parse GDScript only (--check-only)",
    "",
    "Isolation:",
    "  Source workspace     not used as project (never writable)",
    "  Disposable mirror    yes",
    "  Game execution       disabled",
    "  Scene execution      disabled",
    "  Network              denied",
    "  Provider credentials absent",
    "  stdin                closed",
    "  Mirror deleted       after check",
    "",
    `Approval is one-time and binds to plan ${request.digest.slice(0, 8)}.`,
  ];
  return `${lines.join("\n")}\n`;
}

export function formatGodotKnowledgeStatus(status: GodotKnowledgeStatus): string {
  if (status.state !== "ready" || status.profile === null) {
    return [
      "Godot API knowledge",
      "",
      `Engine: none selected (${status.platform})`,
      "Knowledge status: unavailable",
      "",
      `Reason: ${sanitizeForDisplay(status.reason ?? "unknown")}`,
      "",
      "Documentation channels:",
      "  Engine API:          exact executable-derived (not generated yet)",
      "  Manual docs:         not locally synchronized",
    ].join("\n");
  }
  const profile = status.profile;
  return [
    "Godot API knowledge",
    "",
    `Engine: ${profile.engine.godotVersion}`,
    `Executable profile: ${profile.engine.executableSha256.slice(0, 16)}`,
    "Knowledge status: ready",
    "API documentation: exact engine-generated",
    `Classes: ${profile.api.classCount} (+${profile.api.builtinClassCount} built-in)`,
    `Methods/utilities: ${profile.api.utilityFunctionCount} utility functions`,
    `Enums: ${profile.api.globalEnumCount} global`,
    `Constants: ${profile.api.globalConstantCount} global`,
    `Symbols indexed: ${profile.index.symbolCount}`,
    `Generated: ${profile.api.generatedAt}`,
    `Manual documentation channel: ${status.manualChannel ?? "unverified"} (not synchronized)`,
    "",
    "Documentation channels:",
    "  Engine API:          exact executable-derived",
    "  Manual docs:         not locally synchronized",
  ].join("\n");
}

export function formatGodotApiSearchResult(result: GodotKnowledgeQueryResult): string {
  if (result.status !== "ready") {
    return `API search unavailable: ${sanitizeForDisplay(result.message)}`;
  }
  const lines = [
    `Godot API search (${result.engineVersion})`,
    "",
    ...result.results.map((entry) => {
      const location = entry.owner === null ? entry.name : `${entry.owner}.${entry.name}`;
      const summary = entry.summary.length > 0 ? ` - ${sanitizeForDisplay(entry.summary)}` : "";
      return `  ${entry.rank.padEnd(8)} ${entry.kind.padEnd(9)} ${location}${summary}`;
    }),
  ];
  if (result.results.length === 0) {
    lines.push("  (no results)");
  } else if (result.truncated) {
    lines.push("  (results truncated)");
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotDiagnosticsResult(result: GodotProjectCheckResult): string {
  if (result.status !== "checked") {
    return `GDScript diagnostics unavailable: ${sanitizeForDisplay(result.message)}`;
  }
  const lines = [
    "GDScript diagnostics",
    "",
    `Scripts checked: ${result.scriptsChecked}`,
    `Valid: ${result.validCount}`,
    `Invalid: ${result.invalidCount}`,
    "",
  ];
  let shown = 0;
  let currentPath: string | null = null;
  for (const diagnostic of result.diagnostics) {
    if (shown >= 25) {
      lines.push(`  ... ${result.diagnostics.length - shown} more diagnostics`);
      break;
    }
    shown += 1;
    if (diagnostic.path !== currentPath) {
      currentPath = diagnostic.path;
      lines.push(sanitizeForDisplay(diagnostic.path ?? "(unknown file)"));
    }
    const location =
      diagnostic.line === null
        ? ""
        : diagnostic.column === null
          ? `${diagnostic.line}`
          : `${diagnostic.line}:${diagnostic.column}`;
    lines.push(
      `  ${diagnostic.severity.toUpperCase().padEnd(7)} ${location.padEnd(9)} ${sanitizeForDisplay(diagnostic.message)}`,
    );
  }
  if (shown === 0) {
    lines.push("  (no diagnostics)");
  } else if (result.truncated) {
    lines.push("  (diagnostics truncated)");
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotLSPSessionStatus(status: GDScriptSessionStatus): string {
  if (status.state !== "ready") {
    return (
      [
        "Godot GDScript language session",
        "",
        "Status: inactive",
        `Network isolation: ${status.networkIsolation}`,
      ].join("\n") + "\n"
    );
  }
  const ageMs = status.startedAtMs === null ? null : Date.now() - status.startedAtMs;
  return (
    [
      "Godot GDScript language session",
      "",
      "Status: active",
      `Engine: ${status.engineVersion ?? "unknown"}`,
      `Project: ${status.projectName ?? "(unnamed)"}`,
      `Session age: ${ageMs === null ? "unknown" : `${Math.floor(ageMs / 1000)}s`}`,
      `Idle time: ${status.idleMs === null ? "unknown" : `${Math.floor(status.idleMs / 1000)}s`}`,
      "Capabilities:",
      `  diagnostics  ${yesNo(status.capabilities.diagnostics)}`,
      `  hover        ${yesNo(status.capabilities.hover)}`,
      `  completion   ${yesNo(status.capabilities.completion)}`,
      `  definition   ${yesNo(status.capabilities.definition)}`,
      `Open documents: ${status.openDocumentCount}`,
      `Diagnostics: ${status.diagnosticCount}`,
      `Network isolation: ${status.networkIsolation}`,
    ].join("\n") + "\n"
  );
}

export function formatGodotLSPSessionPreview(preview: GDScriptLSPSessionPreview): string {
  const project = preview.projectIntelligence;
  const lines = [
    "Godot GDScript language-server session requires approval",
    "",
    "Project:",
    `  ${preview.projectName ?? "(unnamed)"}`,
    "",
    "Engine:",
    `  ${preview.engineVersion}`,
    `  ${preview.engineEdition} edition`,
    `  Siralos support: ${preview.support}`,
    `  Static compatibility: ${preview.compatibility}`,
    "",
    "Project intelligence:",
    `  GDScript files       ${project.gdscriptFiles}`,
    `  @tool scripts        ${project.toolScripts}`,
    `  editor plugins       ${project.editorPlugins}`,
    `  GDExtensions         ${project.gdextensions}`,
    "",
    "Session:",
    "  Source project       disposable mirror",
    "  Godot mode           headless recovery editor",
    "  LSP network          loopback only",
    "  External network     denied",
    "  Source writes        denied",
    "  Provider secrets     removed",
    "  LSP mutations        disabled",
    "",
    "Capabilities requested:",
    "  diagnostics",
    "  hover",
    "  completion",
    "  definition",
    "",
  ];
  return `${lines.join("\n")}\n`;
}

export function formatGodotHoverResult(result: GDScriptQueryOutcome<GDScriptHoverResult>): string {
  if (result.status !== "ready") {
    return `Hover unavailable: ${sanitizeForDisplay(result.message)}`;
  }
  const range = result.result.range;
  const lines = [
    `Hover: ${sanitizeForDisplay(result.result.path)}`,
    ...(range === null
      ? []
      : [`Range: ${range.start.line}:${range.start.column}-${range.end.line}:${range.end.column}`]),
    ...result.result.contents.map((section) => sanitizeForDisplay(section.text)),
  ];
  return `${lines.join("\n")}\n`;
}

export function formatGodotCompletionResult(
  result: GDScriptQueryOutcome<GDScriptCompletionResult>,
): string {
  if (result.status !== "ready") {
    return `Completion unavailable: ${sanitizeForDisplay(result.message)}`;
  }
  const lines = [
    `Completion: ${sanitizeForDisplay(result.result.path)}`,
    ...result.result.items.slice(0, 50).map((item) => {
      const detail = item.detail === null ? "" : ` - ${sanitizeForDisplay(item.detail)}`;
      return `  ${sanitizeForDisplay(item.label)}${detail}`;
    }),
  ];
  if (result.result.items.length === 0) {
    lines.push("  (no results)");
  } else if (result.result.truncated) {
    lines.push("  (results truncated)");
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotDefinitionResult(
  result: GDScriptQueryOutcome<GDScriptDefinitionResult>,
): string {
  if (result.status !== "ready") {
    return `Definition unavailable: ${sanitizeForDisplay(result.message)}`;
  }
  const lines = [
    `Definition: ${sanitizeForDisplay(result.result.path)}`,
    ...result.result.locations.slice(0, 25).map((location) => {
      const marker = location.external ? " (external)" : "";
      return `  ${sanitizeForDisplay(location.path)}:${location.range.start.line}:${location.range.start.column}${marker}`;
    }),
  ];
  if (result.result.locations.length === 0) {
    lines.push("  (no locations)");
  } else if (result.result.truncated) {
    lines.push("  (results truncated)");
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotSummary(
  selected: GodotSelectedInstallation | null,
  compatibility: GodotCompatibilityAssessment,
  projectDetected: boolean,
): string {
  const lines: string[] = ["Godot:"];
  if (selected === null) {
    lines.push("  Selected installation: none");
  } else {
    lines.push(`  Selected installation: ${selected.installationId}`);
    lines.push(`  Executable: ${selected.sourceLabel}`);
    lines.push(`  Version: ${selected.version.raw}`);
    lines.push(`  Edition: ${selected.edition}`);
    lines.push(`  Release channel: ${selected.releaseChannel}`);
    lines.push(`  Siralos support: ${selected.support}`);
  }
  lines.push(`  Project detected: ${projectDetected ? "yes" : "no"}`);
  if (projectDetected) {
    lines.push(`  Compatibility: ${compatibility.status} (${compatibility.severity})`);
  }
  if (selected !== null) {
    lines.push("  Capabilities:");
    lines.push(`    editor                  ${yesNo(selected.capabilities.editor)}`);
    lines.push(`    headless                ${yesNo(selected.capabilities.headless)}`);
    lines.push(`    recovery mode           ${yesNo(selected.capabilities.recoveryMode)}`);
    lines.push(`    import                  ${yesNo(selected.capabilities.import)}`);
    lines.push(`    GDScript LSP            ${yesNo(selected.capabilities.lsp)}`);
    lines.push(`    GDScript DAP            ${yesNo(selected.capabilities.dap)}`);
    lines.push(`    extension API dump      ${yesNo(selected.capabilities.extensionApiDump)}`);
  }
  lines.push("No project code was executed.");
  lines.push("No project import was performed.");
  return `${lines.join("\n")}\n`;
}

export function formatGodotInstallations(discovery: GodotDiscoveryResult): string {
  const lines: string[] = [];
  if (discovery.candidates.length === 0) {
    lines.push("No Godot installations were discovered.");
  } else {
    lines.push(
      `${"ID".padEnd(14)}${"VERSION".padEnd(24)}${"EDITION".padEnd(18)}${"SOURCE".padEnd(12)}SUPPORT`,
    );
    for (const candidate of discovery.candidates) {
      const marker = candidate.selected ? "*" : " ";
      const version = candidate.version?.raw ?? "-";
      const edition = candidate.edition ?? "-";
      const source = candidate.sourceLabel;
      const support = candidate.support ?? "invalid";
      lines.push(
        `${marker}${candidate.installationId.padEnd(13)}${version.padEnd(24)}${edition.padEnd(18)}${source.padEnd(12)}${support}${candidate.invalid === null ? "" : `  [${candidate.invalid}]`}`,
      );
    }
  }
  const duplicates = discovery.candidates.filter((candidate) => candidate.isDuplicate);
  if (duplicates.length > 0) {
    lines.push(
      `Canonical-path duplicates: ${duplicates.map((candidate) => candidate.installationId).join(", ")}`,
    );
  }
  lines.push("");
  if (discovery.rationale.length > 0) {
    lines.push("Selection rationale:");
    for (const reason of discovery.rationale) {
      lines.push(`  ${reason}`);
    }
  }
  if (discovery.configuration.overrides.length > 0) {
    lines.push(`Overrides: ${discovery.configuration.overrides.join(", ")}`);
  }
  for (const diagnostic of discovery.diagnostics) {
    lines.push(`Warning: ${diagnostic.message}`);
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotProject(
  project: GodotProjectProfile,
  compatibility: GodotCompatibilityAssessment,
): string {
  if (!project.detected) {
    return "No Godot project detected at the workspace root.\n";
  }
  const lines: string[] = [
    `Project: ${project.name ?? "(unnamed)"}`,
    `Config version: ${project.configVersion ?? "unknown"}`,
    `Declared version: ${project.declaredEngineVersion?.raw ?? "none"}`,
    `Main scene: ${project.mainScene ?? "none"}${project.mainScene === null ? "" : project.mainSceneExists === true ? " (exists)" : project.mainSceneExists === false ? " (missing)" : ""}`,
    `Language profile: ${project.languageProfile}`,
    `Rendering methods: ${project.renderingMethods.length > 0 ? project.renderingMethods.join(", ") : "none"}`,
    `Autoloads: ${project.autoloads.length}`,
    `Enabled plugins: ${project.enabledEditorPlugins.length}`,
    `Tool scripts: ${project.executableContent.toolScripts.length}`,
    `GDExtensions: ${project.executableContent.gdextensionDescriptors.length}`,
    `Compatibility: ${compatibility.status} (${compatibility.severity})`,
  ];
  if (project.executableContent.scanTruncated) {
    lines.push("Scan: truncated (results are partial)");
  }
  for (const warning of project.warnings) {
    lines.push(`${warning.severity === "warning" ? "Warning" : "Note"}: ${warning.message}`);
  }
  lines.push("No project code was executed.");
  lines.push("No project import was performed.");
  return `${lines.join("\n")}\n`;
}

export function formatGodotProbePreview(preview: GodotProbePreview): string {
  const risks = preview.risks;
  const mirror = preview.mirror;
  const lines = [
    "Godot project probe requires approval.",
    "",
    "Project:",
    `  ${preview.projectName ?? "(unnamed)"}`,
    "",
    "Engine:",
    `  ${preview.engineVersion}`,
    `  ${preview.engineEdition} edition`,
    `  Siralos support: ${preview.support}`,
    `  Static compatibility: ${preview.compatibility}`,
    "",
    "Static risk inventory:",
    `  @tool scripts         ${risks.toolScripts}`,
    `  enabled plugins       ${risks.enabledEditorPlugins}`,
    `  GDExtensions          ${risks.gdextensions}`,
    `  autoloads             ${risks.autoloads}`,
    `  .NET projects         ${risks.dotnetProjects}`,
    "",
    "Probe isolation:",
    `  Source workspace     not used as project (never writable)`,
    `  Disposable mirror    yes (~${formatFileCount(mirror.estimatedFileCount)} files, ${formatBytes(mirror.estimatedBytes)})`,
    `  Recovery mode        required`,
    `  Headless editor      yes`,
    `  Network              denied`,
    `  Provider secrets     removed`,
    `  Runtime game         disabled`,
    `  Project scripts      recovery-mode restricted`,
    `  Mirror deleted       after probe`,
    "",
    "The probe may cause Godot to import resources inside the disposable mirror.",
    "",
  ];
  return `${lines.join("\n")}\n`;
}

export function formatGodotProbeTerminal(status: string, message: string): string {
  return `  \u2715 ${status}: ${sanitizeForDisplay(message)}
`;
}

export function formatGodotProbeResult(result: GodotRecoveryProbeResult): string {
  const lines = [
    "Recovery probe:",
    `  Status: ${result.status}`,
    `  Engine: ${result.engine.version} (${result.engine.installationId})`,
    `  Recovery mode: ${result.recoveryMode ? "active" : "not used"}`,
    `  Source workspace loaded: no`,
    `  Mirror: ${result.mirror.sourceFiles} files, ${formatBytes(result.mirror.sourceBytes)} copied`,
    `  Generated .godot in mirror: ${result.mirror.generatedGodotDirectory ? `yes (${result.mirror.generatedFiles ?? "?"} files, ${result.mirror.generatedBytes === null ? "unknown" : formatBytes(result.mirror.generatedBytes)})` : "no"}`,
    `  Import state: ${result.mirror.importState}`,
    `  Errors: ${result.diagnostics.errors.length}`,
    `  Warnings: ${result.diagnostics.warnings.length}${result.diagnostics.truncated ? " (truncated)" : ""}`,
    `  Exit: ${result.process.exitCode === null ? "none" : String(result.process.exitCode)} in ${formatDuration(result.process.durationMs)}${result.process.timedOut ? " (timed out)" : ""}`,
    `  Workspace integrity: ${result.workspaceIntegrity.unchanged ? "unchanged" : "changed during probe"}${result.workspaceIntegrity.bounded ? " (bounded baseline)" : ""}`,
    `  Mirror removed: ${result.cleanup.completed ? "yes" : "no"}${result.cleanup.message === undefined ? "" : ` (${result.cleanup.message})`}`,
  ];
  if (result.diagnostics.errors.length > 0) {
    lines.push("  Errors:");
    for (const error of result.diagnostics.errors.slice(0, 10)) {
      lines.push(`    [${error.category}] ${sanitizeForDisplay(error.message)}`);
    }
  }
  if (result.diagnostics.warnings.length > 0) {
    lines.push("  Warnings:");
    for (const warning of result.diagnostics.warnings.slice(0, 10)) {
      lines.push(`    [${warning.category}] ${sanitizeForDisplay(warning.message)}`);
    }
  }
  lines.push("");
  lines.push(
    "Recovery mode reduces editor-side execution risk but does not make arbitrary",
    "project data inherently safe. The probe also relies on a disposable mirror",
    "and the OS sandbox.",
  );
  return `${lines.join("\n")}\n`;
}

export function formatGodotProbeStatus(status: GodotProjectProbeStatus): string {
  const lines = [
    "Project probe:",
    `  Trust state: ${status.state}`,
    `  Manifest digest: ${status.lastManifestDigest === null ? "none" : `${status.lastManifestDigest.slice(0, 12)}\u2026`}`,
    `  Last engine: ${status.lastEngineVersion ?? "none"}`,
  ];
  if (status.lastResult === null) {
    lines.push("  Last result: never run");
  } else {
    const result = status.lastResult;
    lines.push(`  Last result: ${result.status}`);
    lines.push(
      `  Diagnostics: ${result.diagnostics.errors.length} errors, ${result.diagnostics.warnings.length} warnings${result.diagnostics.truncated ? " (truncated)" : ""}`,
    );
    lines.push(
      `  Workspace integrity: ${result.workspaceIntegrity.unchanged ? "unchanged" : "changed"}`,
    );
    lines.push(`  Mirror removed: ${result.cleanup.completed ? "yes" : "no"}`);
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotDoctor(report: GodotDoctorReport): string {
  const discovery = report.discovery;
  const lines: string[] = [
    "Siralos Godot doctor",
    "",
    "Configuration:",
    `  Active installation: ${discovery.configuration.activeInstallation ?? "none"}`,
    `  Configured installations: ${discovery.configuration.configuredCount}`,
    `  PATH discovery: ${discovery.configuration.discoverOnPath ? "enabled" : "disabled"}`,
    `  Overrides: ${discovery.configuration.overrides.length > 0 ? discovery.configuration.overrides.join(", ") : "none"}`,
    "",
    "Sandbox:",
    `  State: ${report.sandbox.state}`,
    `  Backend: ${report.sandbox.backendId}`,
    `  Network restriction: ${yesNo(report.sandbox.networkRestriction)}`,
    `  Filesystem write restriction: ${yesNo(report.sandbox.filesystemWriteRestriction)}`,
    `  Process-tree restriction: ${yesNo(report.sandbox.processTreeRestriction)}`,
    "",
    "Cache:",
    `  Schema version: ${report.cache.schemaVersion}`,
    `  Cached profiles: ${report.cache.cachedProfileCount}`,
    "",
    `Recovery-mode project probe: ${report.recoveryProbe.state} (${report.recoveryProbe.platform})`,
    `API knowledge: ${report.knowledge.state} (${report.knowledge.platform})`,
    `GDScript diagnostics: ${report.diagnostics.state} (${report.diagnostics.platform})`,
  ];
  if (report.recoveryProbe.reason !== null) {
    lines.push(`  ${sanitizeForDisplay(report.recoveryProbe.reason)}`);
  }
  if (report.knowledge.reason !== null) {
    lines.push(`  ${sanitizeForDisplay(report.knowledge.reason)}`);
  }
  if (report.diagnostics.reason !== null) {
    lines.push(`  ${sanitizeForDisplay(report.diagnostics.reason)}`);
  }
  lines.push("");
  lines.push(formatGodotInstallations(discovery).trimEnd());
  if (report.degradedCapabilities.length > 0) {
    lines.push("");
    lines.push(
      `Degraded capabilities: ${report.degradedCapabilities.join(", ")} (probes ran but did not fully verify)`,
    );
  }
  if (report.project.detected) {
    lines.push("");
    lines.push(formatGodotProject(report.project, report.compatibility).trimEnd());
  } else {
    lines.push("");
    lines.push("Project: not detected (no project.godot at the workspace root)");
  }
  lines.push("");
  lines.push("No project code was executed.");
  lines.push("No project import was performed.");
  return `${lines.join("\n")}\n`;
}
