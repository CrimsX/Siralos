import type {
  GodotSceneIntelligence,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@solaris/core";
import { readJsonObject, readRequiredString } from "../../tools/workspace/validation.js";

/**
 * Read-only `godot.inspect_resource` provider tool (Stage 3 milestone 8).
 *
 * Parses one `.tres`/`.theme` statically and returns bounded structural
 * information: resource type, revision, uid, dependencies, properties
 * summary, subresources, and diagnostics. Properties are summarized (name,
 * variant kind, bounded value), never dumped wholesale. No Godot process
 * runs, no project code executes, and no file is modified.
 */
export function createGodotInspectResourceTool(intelligence: GodotSceneIntelligence): Tool {
  return {
    definition: {
      name: "godot.inspect_resource",
      description:
        "Statically inspect a .tres (or .theme) resource at its current workspace revision: type, uid, script, ext/sub resources, bounded property summary, and parse diagnostics. Read-only: no Godot process runs, no project code executes, and nothing is modified.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Workspace-relative resource path, e.g. resources/player_stats.tres.",
          },
        },
        required: ["path"],
        additionalProperties: false,
      },
    },
    capability: "godot.inspect",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Inspection was cancelled." };
      }
      const parsed = readJsonObject(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const parsedPath = readRequiredString(parsed.value, "path");
      if (!parsedPath.ok) {
        return { status: "invalid_input", message: parsedPath.message };
      }
      const result = await intelligence.inspectResource({ path: parsedPath.value });
      if (result.status !== "ok") {
        return {
          status: result.status === "denied" ? "denied" : "failed",
          message: result.message ?? `Inspection failed (${result.status}).`,
        };
      }
      const document = result.document;
      const model = document?.document ?? null;
      const maxPropertiesShown = 128;
      const maxListItems = 128;
      const properties = model === null ? [] : model.properties.slice(0, maxPropertiesShown);
      const externalResources = model?.externalResources ?? [];
      const subResources = model?.subResources ?? [];
      const diagnostics = document?.diagnostics ?? [];
      return {
        status: "success",
        output: {
          path: result.path,
          revision: result.revision,
          status: document?.status ?? "invalid",
          truncated: document?.truncated ?? false,
          type: model?.type ?? null,
          uid: model?.uid ?? null,
          format: model?.format ?? null,
          loadSteps: model?.loadSteps ?? null,
          script:
            model?.script === undefined
              ? null
              : {
                  path: model.script.resource.path ?? null,
                  uid: model.script.resource.uid ?? null,
                  resolvedPath: model.script.resolvedPath ?? null,
                },
          externalResources: externalResources.slice(0, maxListItems).map((resource) => ({
            id: resource.id,
            ...(resource.type === undefined ? {} : { type: resource.type }),
            ...(resource.path === undefined ? {} : { path: resource.path }),
            ...(resource.uid === undefined ? {} : { uid: resource.uid }),
          })),
          externalResourcesTruncated: externalResources.length > maxListItems,
          subResources: subResources.slice(0, maxListItems).map((resource) => ({
            id: resource.id,
            type: resource.type,
            propertyCount: resource.properties.length,
          })),
          subResourcesTruncated: subResources.length > maxListItems,
          propertyCount: model?.properties.length ?? 0,
          properties: properties.map((property) => ({
            name: property.name,
            kind: property.value.kind,
            value: summarizeVariant(property.value),
          })),
          propertiesTruncated: model !== null && model.properties.length > maxPropertiesShown,
          diagnostics: diagnostics.slice(0, maxListItems).map((diagnostic) => ({
            code: diagnostic.code,
            severity: diagnostic.severity,
            message: diagnostic.message,
            ...(diagnostic.line === undefined ? {} : { line: diagnostic.line }),
          })),
          diagnosticsTruncated: diagnostics.length > maxListItems,
        } as never,
        summary: `${result.path} @ ${result.revision ?? "?"}: ${model?.type ?? "unknown"}, ${model?.properties.length ?? 0} properties`,
      };
    },
  };
}

function summarizeVariant(
  value: import("@solaris/core").GodotVariantValue,
): string | number | boolean | null {
  switch (value.kind) {
    case "null":
      return null;
    case "boolean":
    case "integer":
    case "float":
      return value.value;
    case "string":
    case "string_name":
      return value.value.slice(0, 256);
    case "node_path":
      return value.value;
    case "ext_resource":
      return `ExtResource("${value.id}")`;
    case "sub_resource":
      return `SubResource("${value.id}")`;
    case "resource":
      return value.uid ?? value.path ?? null;
    case "vector":
      return `${value.typeName}(${value.components.join(", ")})`;
    case "color":
      return `Color(${value.components.join(", ")})`;
    case "array":
      return `[${value.items.length} items]`;
    case "dictionary":
      return `{${value.entries.length} entries}`;
    case "packed_array":
      return `${value.typeName}(${value.items.length} items)`;
    case "opaque":
      return value.raw.text.slice(0, 256);
  }
}
