import type {
  GodotSceneIntelligence,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@solaris/core";
import {
  readJsonObject,
  readRequiredString,
  readOptionalString,
} from "../../tools/workspace/validation.js";

/**
 * Read-only `godot.inspect_scene` provider tool (Stage 3 milestone 8).
 *
 * Parses one `.tscn` statically and returns bounded structural
 * information: identity/revision, base scene, node tree, attached scripts,
 * external resources, signal connections, groups, and diagnostics. Large
 * scenes are explicitly truncated, never silently presented as complete.
 * No Godot process runs, no project code executes, and no file is
 * modified.
 */
export function createGodotInspectSceneTool(intelligence: GodotSceneIntelligence): Tool {
  return {
    definition: {
      name: "godot.inspect_scene",
      description:
        "Statically inspect a .tscn scene at its current workspace revision: uid, base scene, node tree (root/children/parents/owners), attached scripts, ext/sub resources, groups, signal connections, and parse diagnostics. Read-only: no Godot process runs, no project code executes, and nothing is modified.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Workspace-relative .tscn path, e.g. scenes/player.tscn.",
          },
          view: {
            type: "string",
            enum: ["summary", "tree", "full"],
            description:
              "summary (default): compact facts; tree: compact tree text; full: all sections.",
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
      const parsedView = readOptionalString(parsed.value, "view");
      if (!parsedView.ok) {
        return { status: "invalid_input", message: parsedView.message };
      }
      const view = parsedView.value ?? "summary";
      if (view !== "summary" && view !== "tree" && view !== "full") {
        return { status: "invalid_input", message: 'view must be "summary", "tree", or "full".' };
      }
      const result = await intelligence.inspectScene({ path: parsedPath.value });
      if (result.status !== "ok") {
        return {
          status: result.status === "denied" ? "denied" : "failed",
          message: result.message ?? `Inspection failed (${result.status}).`,
        };
      }
      const document = result.document;
      const model = document?.document ?? null;
      const tree = result.tree;
      const output: Record<string, unknown> = {
        path: result.path,
        revision: result.revision,
        status: document?.status ?? "invalid",
        truncated: document?.truncated ?? false,
        uid: model?.uid ?? null,
        format: model?.format ?? null,
        loadSteps: model?.loadSteps ?? null,
        nodeCount: model?.nodes.length ?? 0,
        baseScene: null,
        rootNode: null,
        scripts: [],
        externalResources: [],
        subResources: [],
        connections: [],
        groups: [],
        editableInstances: model?.editableInstances ?? [],
        diagnostics: (document?.diagnostics ?? []).map((diagnostic) => ({
          code: diagnostic.code,
          severity: diagnostic.severity,
          message: diagnostic.message,
          ...(diagnostic.line === undefined ? {} : { line: diagnostic.line }),
        })),
      };
      if (model !== null) {
        output["baseScene"] =
          model.baseScene === undefined
            ? null
            : {
                path: model.baseScene.resource.path ?? null,
                uid: model.baseScene.resource.uid ?? null,
                resolvedPath: model.baseScene.resolvedPath ?? null,
              };
        output["rootNode"] =
          tree?.root === null
            ? null
            : {
                name: tree?.root?.node.name ?? "",
                type: tree?.root?.node.type ?? null,
                path: tree?.root?.path ?? null,
              };
        output["scripts"] = model.nodes
          .filter((node) => node.script !== undefined)
          .map((node) => ({
            node: node.name,
            path: node.script?.resource.path ?? null,
            uid: node.script?.resource.uid ?? null,
            resolvedPath: node.script?.resolvedPath ?? null,
          }));
        output["externalResources"] = model.externalResources.map((resource) => ({
          id: resource.id,
          ...(resource.type === undefined ? {} : { type: resource.type }),
          ...(resource.path === undefined ? {} : { path: resource.path }),
          ...(resource.uid === undefined ? {} : { uid: resource.uid }),
        }));
        output["subResources"] = model.subResources.map((resource) => ({
          id: resource.id,
          type: resource.type,
          propertyCount: resource.properties.length,
        }));
        output["connections"] = model.connections.map((connection) => ({
          signal: connection.signal,
          from: connection.from,
          to: connection.to,
          method: connection.method,
          ...(connection.flags === undefined ? {} : { flags: connection.flags }),
          ...(connection.binds === undefined
            ? {}
            : { binds: connection.binds.map((bind) => summarizeVariant(bind)) }),
        }));
        output["groups"] = [...new Set(model.nodes.flatMap((node) => node.groups))];
      }
      if (view === "tree" && tree !== null) {
        output["tree"] = renderTree(tree);
      } else if (view === "full" && tree !== null) {
        output["tree"] = renderTree(tree);
        output["instances"] =
          model?.nodes
            .filter((node) => node.instance !== undefined)
            .map((node) => ({
              node: node.name,
              path: node.instance?.resource.path ?? null,
              uid: node.instance?.resource.uid ?? null,
              resolvedPath: node.instance?.resolvedPath ?? null,
            })) ?? [];
      }
      return {
        status: "success",
        output: output as never,
        summary: `${result.path} @ ${result.revision ?? "?"}: ${document?.status ?? "invalid"}, ${model?.nodes.length ?? 0} nodes`,
      };
    },
  };
}

function renderTree(
  tree: NonNullable<import("@solaris/core").GodotSceneInspectionResult["tree"]>,
): { readonly text: string; readonly truncated: boolean } {
  const lines: string[] = [];
  const visit = (node: import("@solaris/core").GodotSceneTreeNode, depth: number): void => {
    if (lines.length >= 200) {
      return;
    }
    const kind = node.node.instance !== undefined ? " (instance)" : "";
    const script = node.node.script?.resolvedPath ?? node.node.script?.resource.path;
    const scriptSuffix = script === undefined ? "" : ` script=${script}`;
    lines.push(
      `${"  ".repeat(depth)}${node.node.name}: ${node.node.type ?? "<external>"}${kind}${scriptSuffix}`,
    );
    for (const child of node.children) {
      visit(child, depth + 1);
    }
  };
  if (tree.root !== null) {
    visit(tree.root, 0);
  }
  return { text: lines.join("\n"), truncated: lines.length >= 200 };
}

function summarizeVariant(value: import("@solaris/core").GodotVariantValue): {
  readonly kind: string;
  readonly value: unknown;
} {
  switch (value.kind) {
    case "null":
      return { kind: "null", value: null };
    case "boolean":
    case "integer":
    case "float":
      return { kind: value.kind, value: value.value };
    case "string":
    case "string_name":
      return { kind: value.kind, value: value.value.slice(0, 256) };
    case "node_path":
      return { kind: "node_path", value: value.value };
    case "ext_resource":
      return { kind: "ext_resource", value: value.id };
    case "sub_resource":
      return { kind: "sub_resource", value: value.id };
    case "resource":
      return { kind: "resource", value: value.uid ?? value.path ?? null };
    case "array":
      return { kind: "array", value: value.items.length };
    case "dictionary":
      return { kind: "dictionary", value: value.entries.length };
    case "vector":
      return { kind: "vector", value: value.typeName };
    case "color":
      return { kind: "color", value: value.components };
    case "packed_array":
      return { kind: "packed_array", value: value.items.length };
    case "opaque":
      return { kind: "opaque", value: value.typeName };
  }
}
