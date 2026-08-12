import type {
  GodotInspector,
  GodotSelectedInstallation,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@siralos/core";
import { errorMessage } from "../../support/error-message.js";

/**
 * Read-only `godot.inspect_engine` provider tool.
 *
 * Returns only the safe subset: installation id, source, exact version,
 * edition, release channel, Siralos support, advertised and verified
 * capabilities, the API dump fingerprint, and bounded diagnostics. Absolute
 * executable paths, raw help output, and the complete API dump never enter
 * provider context. The provider cannot select an executable or trigger
 * probes with custom parameters.
 */
export function createGodotInspectEngineTool(inspector: GodotInspector): Tool {
  return {
    definition: {
      name: "godot.inspect_engine",
      description:
        "Inspect the selected Godot installation: exact version, edition, release channel, Siralos support classification, advertised and operationally verified capabilities, and the extension API dump fingerprint. Read-only; no project is opened or imported.",
      inputSchema: { type: "object", additionalProperties: false },
    },
    capability: "godot.inspect",
    async execute(_input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      try {
        const selected: GodotSelectedInstallation | null = await inspector.selected(context.signal);
        if (selected === null) {
          return {
            status: "success",
            output: {
              selected: false,
              reason: "No Godot installation is selected.",
              version: null,
              edition: null,
              releaseChannel: null,
              support: null,
              capabilities: null,
              verifiedCapabilities: [],
              degradedCapabilities: [],
              apiDumpSha256: null,
              diagnostics: [],
            },
            summary: "No Godot installation is selected.",
          };
        }
        return {
          status: "success",
          output: {
            selected: true,
            installationId: selected.installationId,
            source: selected.sourceLabel,
            version: selected.version.raw,
            major: selected.version.major,
            minor: selected.version.minor,
            patch: selected.version.patch,
            status: selected.version.status,
            edition: selected.edition,
            editionConfidence: selected.editionConfidence,
            releaseChannel: selected.releaseChannel,
            support: selected.support,
            capabilities: {
              editor: selected.capabilities.editor,
              projectManager: selected.capabilities.projectManager,
              recoveryMode: selected.capabilities.recoveryMode,
              headless: selected.capabilities.headless,
              projectPath: selected.capabilities.projectPath,
              scene: selected.capabilities.scene,
              script: selected.capabilities.script,
              checkOnly: selected.capabilities.checkOnly,
              import: selected.capabilities.import,
              quit: selected.capabilities.quit,
              quitAfter: selected.capabilities.quitAfter,
              lsp: selected.capabilities.lsp,
              dap: selected.capabilities.dap,
              debugServer: selected.capabilities.debugServer,
              buildSolutions: selected.capabilities.buildSolutions,
              extensionApiDump: selected.capabilities.extensionApiDump,
              extensionApiWithDocsDump: selected.capabilities.extensionApiWithDocsDump,
              extensionApiValidation: selected.capabilities.extensionApiValidation,
              docTool: selected.capabilities.docTool,
              movieWriting: selected.capabilities.movieWriting,
            },
            verifiedCapabilities: selected.verifiedCapabilities,
            degradedCapabilities: selected.degradedCapabilities,
            executableFingerprint: selected.executableFingerprint,
            apiDumpSha256: selected.apiDumpSha256,
            diagnostics: selected.diagnostics.map((diagnostic) => ({
              severity: diagnostic.severity,
              message: diagnostic.message,
            })),
          },
          summary: `${selected.version.raw} (${selected.edition}, ${selected.support})`,
        };
      } catch (error: unknown) {
        return {
          status: "failed",
          message: errorMessage(error, "An unknown Godot inspection failure occurred."),
        };
      }
    },
  };
}
