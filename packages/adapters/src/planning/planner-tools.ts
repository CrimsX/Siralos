import {
  createToolRegistry,
  type GodotInspector,
  type GodotKnowledge,
  type Tool,
  type ToolRegistry,
} from "@solaris/core";
import { createGodotApiLookupTool } from "../godot/tools/godot-api-lookup-tool.js";
import { createGodotApiSearchTool } from "../godot/tools/godot-api-search-tool.js";
import { createGodotInspectEngineTool } from "../godot/tools/godot-inspect-engine-tool.js";
import { createGodotInspectProjectTool } from "../godot/tools/godot-inspect-project-tool.js";
import { createWorkspaceListTool } from "../tools/workspace/workspace-list-tool.js";
import { createWorkspaceReadTool } from "../tools/workspace/workspace-read-tool.js";
import { createWorkspaceSearchTool } from "../tools/workspace/workspace-search-tool.js";

/**
 * Read-only planner tool registry (Stage 3 milestone 7, ADR 0020).
 *
 * The planner may inspect: the workspace (list/read/search), static Godot
 * engine/project inspection, Godot API knowledge, configured references,
 * bounded research (policy-gated), and self-reference. Write tools,
 * `process.run`, approval controls, checkpoint mutation, undo, probes,
 * diagnostics, and language-session tools are absent BY CONSTRUCTION: this
 * module never imports their creators, and the architecture check enforces
 * that the planner adapter cannot import workspace-mutation, process,
 * checkpoint, approval, or prepared-tool modules. The ToolProjector in
 * `planning` mode additionally hides anything outside the read-only
 * allowlist from the provider-visible schema.
 */
export interface PlannerToolDependencies {
  readonly workspaceRoot: string;
  readonly godot: GodotInspector;
  readonly knowledge: GodotKnowledge;
  /** Read-only reference tools (`reference.list`/`reference.read`/`reference.search`). */
  readonly referenceTools?: readonly Tool[];
  /** Research tools (`research.repository`/`research.godot_docs`; policy-gated). */
  readonly researchTools?: readonly Tool[];
  /** Self-reference tools (`self.read`/`self.search`). */
  readonly selfTools?: readonly Tool[];
}

export function createPlannerToolRegistry(dependencies: PlannerToolDependencies): ToolRegistry {
  const tools: readonly Tool[] = [
    createWorkspaceListTool(dependencies.workspaceRoot),
    createWorkspaceReadTool(dependencies.workspaceRoot),
    createWorkspaceSearchTool(dependencies.workspaceRoot),
    createGodotInspectEngineTool(dependencies.godot),
    createGodotInspectProjectTool(dependencies.godot),
    createGodotApiSearchTool(dependencies.knowledge),
    createGodotApiLookupTool(dependencies.knowledge),
    ...(dependencies.referenceTools ?? []),
    ...(dependencies.researchTools ?? []),
    ...(dependencies.selfTools ?? []),
  ];
  return createToolRegistry(tools);
}
