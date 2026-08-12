import type {
  GodotCompatibilityAssessment,
  GodotInspector,
  GodotProjectProfile,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@siralos/core";
import { errorMessage } from "../../support/error-message.js";

/**
 * Read-only `godot.inspect_project` provider tool.
 *
 * Returns the static project profile, the executable-content inventory, and
 * the selected-engine compatibility assessment, with an explicit statement
 * that no project code was executed and the inspection was static. The tool
 * works without an engine; the provider cannot request project loading.
 */
export function createGodotInspectProjectTool(inspector: GodotInspector): Tool {
  return {
    definition: {
      name: "godot.inspect_project",
      description:
        "Statically inspect the Godot project at the workspace root: detection, name, config version, declared engine version, main scene, language profile, rendering methods, autoloads, enabled plugins, tool scripts, editor plugins, GDExtension descriptors, and compatibility with the selected engine. No project code is executed or imported.",
      inputSchema: { type: "object", additionalProperties: false },
    },
    capability: "godot.inspect",
    async execute(_input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      try {
        const project: GodotProjectProfile = await inspector.projectProfile(context.signal);
        const compatibility: GodotCompatibilityAssessment = await inspector.compatibility(
          context.signal,
        );
        return {
          status: "success",
          output: {
            detected: project.detected,
            projectFileSha256: project.projectFileSha256,
            configVersion: project.configVersion,
            name: project.name,
            applicationVersion: project.applicationVersion,
            declaredFeatures: project.declaredFeatures,
            declaredEngineVersion:
              project.declaredEngineVersion === null
                ? null
                : {
                    major: project.declaredEngineVersion.major,
                    minor: project.declaredEngineVersion.minor,
                    patch: project.declaredEngineVersion.patch,
                    raw: project.declaredEngineVersion.raw,
                  },
            mainScene: project.mainScene,
            mainSceneExists: project.mainSceneExists,
            languageProfile: project.languageProfile,
            renderingMethods: project.renderingMethods,
            autoloads: project.autoloads.map((autoload) => ({
              name: autoload.name,
              target: autoload.target,
              isSingleton: autoload.isSingleton,
            })),
            enabledEditorPlugins: project.enabledEditorPlugins,
            executableContent: {
              toolScripts: project.executableContent.toolScripts,
              editorPlugins: project.executableContent.editorPlugins.map((plugin) => ({
                path: plugin.path,
                name: plugin.name,
                description: plugin.description,
                author: plugin.author,
                version: plugin.version,
                scriptPath: plugin.scriptPath,
                language: plugin.language,
                enabled: plugin.enabled,
                importPluginHeuristic: plugin.importPluginHeuristic,
              })),
              importPlugins: project.executableContent.importPlugins,
              gdextensionDescriptors: project.executableContent.gdextensionDescriptors.map(
                (extension) => ({
                  path: extension.path,
                  compatibilityMinimum: extension.compatibilityMinimum,
                  libraryTargets: extension.libraryTargets,
                  libraryFilesExist: extension.libraryFilesExist,
                  escapesThroughSymlinks: extension.escapesThroughSymlinks,
                }),
              ),
              autoloadCount: project.executableContent.autoloadCount,
              dotnetProjectFiles: project.executableContent.dotnetProjectFiles,
              scanTruncated: project.executableContent.scanTruncated,
            },
            compatibility: {
              status: compatibility.status,
              severity: compatibility.severity,
              reasons: compatibility.reasons,
            },
            warnings: project.warnings.map((warning) => ({
              severity: warning.severity,
              message: warning.message,
            })),
            static: true,
            projectCodeExecuted: false,
            projectImportPerformed: false,
          },
          summary: project.detected
            ? `Static project inspection: ${project.name ?? "unnamed"} (${compatibility.status})`
            : "No Godot project detected at the workspace root.",
        };
      } catch (error: unknown) {
        return {
          status: "failed",
          message: errorMessage(error, "An unknown Godot project inspection failure occurred."),
        };
      }
    },
  };
}
