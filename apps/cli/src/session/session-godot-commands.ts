import {
  describeError,
  formatGodotApiSearchResult,
  formatGodotCompletionResult,
  formatGodotDefinitionResult,
  formatGodotDiagnosticPreview,
  formatGodotDiagnosticsResult,
  formatGodotDoctor,
  formatGodotHoverResult,
  formatGodotInstallations,
  formatGodotKnowledgeStatus,
  formatGodotLSPSessionPreview,
  formatGodotLSPSessionStatus,
  formatGodotProbePreview,
  formatGodotProbeResult,
  formatGodotProbeTerminal,
  formatGodotProject,
  formatGodotSummary,
  formatProviderFailure,
} from "../output.js";
import type { SessionControls, SessionIO, SessionInfo } from "./session-types.js";

export async function runGodotCommand(io: SessionIO, sessionInfo: SessionInfo): Promise<void> {
  try {
    const selected = await sessionInfo.godot.selected();
    const project = await sessionInfo.godot.projectProfile();
    const compatibility = await sessionInfo.godot.compatibility();
    io.write(formatGodotSummary(selected, compatibility, project.detected));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

export async function runGodotInstallationsCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
): Promise<void> {
  try {
    io.write(formatGodotInstallations(await sessionInfo.godot.discover()));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

export async function runGodotProjectCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
): Promise<void> {
  try {
    const project = await sessionInfo.godot.projectProfile();
    const compatibility = await sessionInfo.godot.compatibility();
    io.write(formatGodotProject(project, compatibility));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

export async function runGodotDoctorCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
): Promise<void> {
  try {
    io.write(formatGodotDoctor(await sessionInfo.godot.doctor()));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

export async function runGodotProbeCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
): Promise<void> {
  const controller = controls.beginPrompt();
  try {
    io.write("Checking recovery-probe capability\u2026\n");
    const support = await sessionInfo.godotProbe.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ?? "Recovery-mode project probing is unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Preparing the Godot project probe\u2026\n");
    const prepared = await sessionInfo.godotProbe.prepare(controller.signal);
    if (prepared.status !== "ready") {
      io.write(formatGodotProbeTerminal(prepared.status, prepared.message));
      return;
    }
    io.write(formatGodotProbePreview(prepared.preview));
    const decision = await sessionInfo.reviewer.review(
      {
        id: "godot-probe",
        capability: "godot.probe_project",
        toolName: "godot.probe_project",
        summary: `recovery-mode project probe (${prepared.preview.risks.toolScripts} tool scripts, ${prepared.preview.risks.enabledEditorPlugins} plugins)`,
        preview: prepared.preview,
        digest: prepared.digest,
      },
      controller.signal,
    );
    if (decision.type !== "approve_once") {
      io.write(
        decision.type === "cancelled"
          ? "  \u2715 probe approval cancelled\n"
          : `  \u2715 probe denied: ${decision.reason ?? "not approved"}\n`,
      );
      return;
    }
    io.write("  approval approved\n");
    const result = await sessionInfo.godotProbe.execute(prepared.probe, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    io.write(formatGodotProbeResult(result));
  } catch (error: unknown) {
    io.write(
      controller.signal.aborted
        ? "  \u2715 probe cancelled\n"
        : formatProviderFailure(describeGodotFailure(error)),
    );
  } finally {
    controls.endPrompt();
  }
}

export async function runGDScriptLSPCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
): Promise<void> {
  const controller = controls.beginPrompt();
  try {
    const workflowStatus = sessionInfo.development.status();
    if (workflowStatus.session !== null && workflowStatus.session.state.kind === "active") {
      io.write(
        "The active development workflow owns the language session lifecycle; its approval covers LSP recreation after approved edits.\n",
      );
      return;
    }
    const status = sessionInfo.language.status();
    if (status.state === "ready") {
      io.write(formatGodotLSPSessionStatus(status));
      return;
    }
    io.write("Checking GDScript language-session capability\u2026\n");
    const support = await sessionInfo.language.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ?? "The Godot language session is unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Preparing the GDScript language session\u2026\n");
    const prepared = await sessionInfo.language.prepare(controller.signal);
    if (prepared.status !== "ready") {
      io.write(formatGodotProbeTerminal(prepared.status, prepared.message));
      return;
    }
    io.write(formatGodotLSPSessionPreview(prepared.preview));
    const decision = await sessionInfo.reviewer.review(
      {
        id: "gdscript-lsp",
        capability: "godot.lsp",
        toolName: "godot.lsp_session",
        summary: `Godot GDScript language session (${prepared.preview.projectIntelligence.gdscriptFiles} scripts)`,
        preview: prepared.preview,
        digest: prepared.digest,
      },
      controller.signal,
    );
    if (decision.type !== "approve_once") {
      io.write(
        decision.type === "cancelled"
          ? "  \u2715 session approval cancelled\n"
          : `  \u2715 session denied: ${decision.reason ?? "not approved"}\n`,
      );
      return;
    }
    io.write("  approval approved\n");
    const result = await sessionInfo.language.start(prepared.session, {
      approvedDigest: prepared.digest,
      signal: controller.signal,
    });
    io.write(
      result.status === "ready"
        ? formatGodotLSPSessionStatus(result.session.getStatus())
        : formatGodotProbeTerminal(result.status, result.message),
    );
  } catch (error: unknown) {
    io.write(
      controller.signal.aborted
        ? "  \u2715 session cancelled\n"
        : formatProviderFailure(describeGodotFailure(error)),
    );
  } finally {
    controls.endPrompt();
  }
}

export async function runGDScriptLSPStopCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
): Promise<void> {
  try {
    await sessionInfo.language.closeAll();
    io.write("GDScript language session stopped.\n");
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

export async function runGDScriptPositionCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  operation: "hover" | "complete" | "definition",
  args: readonly string[],
): Promise<void> {
  if (args.length < 3) {
    io.write(`Usage: /gdscript-${operation} <relative-path> <line> <column>\n`);
    return;
  }
  const path = args[0] ?? "";
  const line = Number.parseInt(args[1] ?? "", 10);
  const column = Number.parseInt(args[2] ?? "", 10);
  if (!Number.isInteger(line) || line < 1 || !Number.isInteger(column) || column < 1) {
    io.write("Line and column must be 1-based positive integers.\n");
    return;
  }
  const session = sessionInfo.language.activeSession();
  if (session === null) {
    io.write(
      "No Godot language session is active; start and approve one with /gdscript-lsp first.\n",
    );
    return;
  }
  try {
    if (operation === "hover") {
      io.write(formatGodotHoverResult(await session.hover({ path, line, column })));
    } else if (operation === "complete") {
      io.write(formatGodotCompletionResult(await session.completion({ path, line, column })));
    } else {
      io.write(formatGodotDefinitionResult(await session.definition({ path, line, column })));
    }
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGodotFailure(error)));
  }
}

export async function runGodotKnowledgeRefreshCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
): Promise<void> {
  const controller = controls.beginPrompt();
  try {
    io.write("Checking exact-engine API knowledge capability\u2026\n");
    const support = await sessionInfo.knowledge.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ??
            "Exact-engine API knowledge generation is unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Regenerating the exact-engine API knowledge profile\u2026\n");
    const result = await sessionInfo.knowledge.refresh(controller.signal);
    if (result.status === "ready") {
      io.write("Knowledge profile regenerated.\n");
      io.write(formatGodotKnowledgeStatus(sessionInfo.knowledge.status()));
    } else {
      io.write(formatGodotProbeTerminal(result.status, result.message));
    }
  } catch (error: unknown) {
    io.write(
      controller.signal.aborted
        ? "  \u2715 knowledge refresh cancelled\n"
        : formatProviderFailure(describeGodotFailure(error)),
    );
  } finally {
    controls.endPrompt();
  }
}

export async function runGodotApiCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  args: readonly string[],
): Promise<void> {
  const query = args.join(" ").trim();
  if (query.length === 0) {
    io.write("Usage: /godot-api <query>\n");
    return;
  }
  io.write(formatGodotApiSearchResult(await sessionInfo.knowledge.search({ query })));
}

export async function runGDScriptCheckCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
  args: readonly string[],
): Promise<void> {
  const scriptPath = args.join(" ").trim();
  if (scriptPath.length === 0) {
    io.write("Usage: /gdscript-check <relative-path>\n");
    return;
  }
  const controller = controls.beginPrompt();
  try {
    io.write("Checking GDScript diagnostic capability\u2026\n");
    const support = await sessionInfo.diagnostics.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ?? "GDScript diagnostics are unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Preparing the GDScript check\u2026\n");
    const prepared = await sessionInfo.diagnostics.prepare(
      { paths: [scriptPath] },
      controller.signal,
    );
    if (prepared.status !== "ready") {
      io.write(formatGodotProbeTerminal(prepared.status, prepared.message));
      return;
    }
    io.write(formatGodotDiagnosticPreview(prepared.preview));
    const decision = await sessionInfo.reviewer.review(
      {
        id: "gdscript-check",
        capability: "godot.diagnose",
        toolName: "godot.check_script",
        summary: `GDScript check-only diagnostics (${scriptPath})`,
        preview: prepared.preview,
        digest: prepared.digest,
      },
      controller.signal,
    );
    if (decision.type !== "approve_once") {
      io.write(
        decision.type === "cancelled"
          ? "  \u2715 check approval cancelled\n"
          : `  \u2715 check denied: ${decision.reason ?? "not approved"}\n`,
      );
      return;
    }
    io.write("  approval approved\n");
    io.write(
      formatGodotDiagnosticsResult(
        await sessionInfo.diagnostics.execute(prepared.check, {
          approvedDigest: prepared.digest,
          signal: controller.signal,
        }),
      ),
    );
  } catch (error: unknown) {
    io.write(
      controller.signal.aborted
        ? "  \u2715 check cancelled\n"
        : formatProviderFailure(describeGodotFailure(error)),
    );
  } finally {
    controls.endPrompt();
  }
}

export async function runGDScriptDiagnosticsCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  controls: SessionControls,
): Promise<void> {
  const controller = controls.beginPrompt();
  try {
    io.write("Checking GDScript diagnostic capability\u2026\n");
    const support = await sessionInfo.diagnostics.support();
    if (support.state !== "available") {
      io.write(
        formatGodotProbeTerminal(
          "unavailable",
          support.reason ?? "GDScript diagnostics are unavailable on this platform.",
        ),
      );
      return;
    }
    io.write("Preparing the project-wide GDScript check\u2026\n");
    const prepared = await sessionInfo.diagnostics.prepare({}, controller.signal);
    if (prepared.status !== "ready") {
      io.write(formatGodotProbeTerminal(prepared.status, prepared.message));
      return;
    }
    io.write(formatGodotDiagnosticPreview(prepared.preview));
    const decision = await sessionInfo.reviewer.review(
      {
        id: "gdscript-diagnostics",
        capability: "godot.diagnose",
        toolName: "godot.check_project_scripts",
        summary: `GDScript check-only diagnostics (${prepared.preview.scripts.count} scripts)`,
        preview: prepared.preview,
        digest: prepared.digest,
      },
      controller.signal,
    );
    if (decision.type !== "approve_once") {
      io.write(
        decision.type === "cancelled"
          ? "  \u2715 diagnostics approval cancelled\n"
          : `  \u2715 diagnostics denied: ${decision.reason ?? "not approved"}\n`,
      );
      return;
    }
    io.write("  approval approved\n");
    io.write(
      formatGodotDiagnosticsResult(
        await sessionInfo.diagnostics.execute(prepared.check, {
          approvedDigest: prepared.digest,
          signal: controller.signal,
        }),
      ),
    );
  } catch (error: unknown) {
    io.write(
      controller.signal.aborted
        ? "  \u2715 diagnostics cancelled\n"
        : formatProviderFailure(describeGodotFailure(error)),
    );
  } finally {
    controls.endPrompt();
  }
}

export function describeGodotFailure(error: unknown): string {
  return error instanceof Error && error.message.length > 0 ? error.message : describeError(error);
}
