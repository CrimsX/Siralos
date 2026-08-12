import {
  createToolRegistry,
  type GDScriptLanguageService,
  type GitInspector,
  type GodotInspector,
  type GodotKnowledge,
  type Tool,
  type ToolRegistry,
} from "@siralos/core";
import { createGitDiffTool } from "../../git/tools/git-diff-tool.js";
import { createGitStatusTool } from "../../git/tools/git-status-tool.js";
import { createGodotApiLookupTool } from "../tools/godot-api-lookup-tool.js";
import { createGodotApiSearchTool } from "../tools/godot-api-search-tool.js";
import {
  createGodotDefinitionTool,
  createGodotHoverTool,
  createGodotLSPDiagnosticsTool,
} from "../tools/godot-lsp-query-tools.js";
import { createGodotInspectProjectTool } from "../tools/godot-inspect-project-tool.js";
import { createWorkspaceListTool } from "../../tools/workspace/workspace-list-tool.js";
import { createWorkspaceReadTool } from "../../tools/workspace/workspace-read-tool.js";
import { createWorkspaceSearchTool } from "../../tools/workspace/workspace-search-tool.js";

/**
 * Read-only reviewer tool registry (ADR 0013 §27).
 *
 * The independent reviewer may inspect the repository but can never
 * mutate it: workspace inspection, Git inspection, static Godot project
 * inspection, API search/lookup, and — when a language session is already
 * active and current — the read-only LSP query tools. Write tools,
 * `process.run`, approval controls, checkpoint mutation, and undo are
 * absent BY CONSTRUCTION: this module never imports their creators, and
 * the architecture check enforces that the reviewer adapter cannot import
 * workspace-mutation, process, checkpoint, or approval modules.
 */
export interface ReviewerToolDependencies {
  readonly workspaceRoot: string;
  readonly git: GitInspector;
  readonly godot: GodotInspector;
  readonly knowledge: GodotKnowledge;
  readonly language: GDScriptLanguageService;
  /** Live workflow language-query gate (injected to avoid a composition cycle). */
  readonly languageQueryGate: () => { readonly blocked: boolean; readonly message: string | null };
}

export function createReviewerToolRegistry(dependencies: ReviewerToolDependencies): ToolRegistry {
  const tools: readonly Tool[] = [
    createWorkspaceListTool(dependencies.workspaceRoot),
    createWorkspaceReadTool(dependencies.workspaceRoot),
    createWorkspaceSearchTool(dependencies.workspaceRoot),
    createGitStatusTool(dependencies.git),
    createGitDiffTool(dependencies.git),
    createGodotInspectProjectTool(dependencies.godot),
    createGodotApiSearchTool(dependencies.knowledge),
    createGodotApiLookupTool(dependencies.knowledge),
    createGodotHoverTool(dependencies.language, dependencies.languageQueryGate),
    createGodotDefinitionTool(dependencies.language, dependencies.languageQueryGate),
    createGodotLSPDiagnosticsTool(dependencies.language, dependencies.languageQueryGate),
  ];
  // `godot.complete` is intentionally excluded: completion candidates
  // include insertText that is never applied, and a reviewer never needs
  // them.
  return createToolRegistry(tools);
}
