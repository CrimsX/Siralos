import { canonicalizeJson, sha256Hex } from "../godot/digest.js";

/**
 * Authoritative Siralos slash-command catalog (Stage 3 milestone 6).
 *
 * This is the SINGLE source of truth for the interactive command surface:
 * the CLI parser derives its `SlashCommand` union and lookup list from it,
 * `/help` renders its descriptions, and the built-in `@siralos`
 * self-reference documents it. A command cannot exist in the interactive
 * session without appearing here (the exhaustive switch would not compile
 * for an id the parser cannot produce), and it cannot appear here without
 * being documented in the self-reference — no separate hand-maintained
 * command list can drift.
 *
 * The catalog is host-owned and immutable; providers and projects can
 * never register commands.
 */

export type CommandCatalogGroup =
  | "session"
  | "inspection"
  | "workspace"
  | "workflow"
  | "godot"
  | "knowledge"
  | "references"
  | "doctor";

export interface CommandCatalogEntry {
  readonly id: string;
  readonly description: string;
  readonly group: CommandCatalogGroup;
}

export const COMMAND_CATALOG = [
  // --- session ---
  { id: "help", description: "Show this help", group: "session" },
  { id: "status", description: "Show provider, session, and workspace status", group: "session" },
  { id: "clear", description: "Clear the terminal (conversation is kept)", group: "session" },
  { id: "exit", description: "Close Siralos", group: "session" },
  // --- inspection ---
  { id: "tools", description: "List the available tools", group: "inspection" },
  { id: "sandbox", description: "Show the sandbox backend status", group: "inspection" },
  { id: "permissions", description: "Show capability rules", group: "inspection" },
  { id: "commands", description: "Show command runners and command status", group: "inspection" },
  {
    id: "context",
    description: "Show the projected context (stable/contextual/volatile, pressure)",
    group: "inspection",
  },
  {
    id: "instructions",
    description: "Show discovered project instruction files with revisions",
    group: "inspection",
  },
  {
    id: "knowledge",
    description: "Show current project knowledge facts (/knowledge why: last retrieval trace)",
    group: "inspection",
  },
  {
    id: "references",
    description: "Show configured external references and their status",
    group: "inspection",
  },
  {
    id: "reference",
    description: "Show one reference's identity and availability",
    group: "inspection",
  },
  {
    id: "research-status",
    description: "Show research capability, sources, and recent evidence",
    group: "inspection",
  },
  {
    id: "development-status",
    description: "Show the active development workflow's bounded status",
    group: "inspection",
  },
  {
    id: "brief",
    description:
      "Show the compiled executor brief for the current task (task goal, manifest/contract identity, touchpoints, invariants, acceptance ids)",
    group: "inspection",
  },
  {
    id: "milestone",
    description: "Show the current milestone manifest and its evidence-backed acceptance status",
    group: "inspection",
  },
  // --- workspace ---
  {
    id: "git-status",
    description: "Show Git availability and repository status",
    group: "workspace",
  },
  {
    id: "diff",
    description: "Show a bounded Git diff (working, staged, or head)",
    group: "workspace",
  },
  { id: "checkpoints", description: "List recorded recovery checkpoints", group: "workspace" },
  {
    id: "undo",
    description: "Undo the latest Siralos mutation (or /undo <checkpoint-id>)",
    group: "workspace",
  },
  // --- workflow ---
  { id: "cancel", description: "Cancel the running command", group: "workflow" },
  {
    id: "task",
    description: "Start a host-owned ad-hoc task (completion requires host verification)",
    group: "workflow",
  },
  {
    id: "task-status",
    description: "Show the current task: phase, contract revision, criteria, steps, progress",
    group: "workflow",
  },
  {
    id: "develop",
    description:
      "Start one GDScript development workflow (host-controlled planning; one-time approval; each source change is approved separately; /develop --plan <request> forces full planning before execution)",
    group: "workflow",
  },
  {
    id: "plan",
    description:
      "Plan-only mode: run read-only planning for a request and return a structured plan; no source is modified, no mutation approval is requested, and no execution follows",
    group: "workflow",
  },
  {
    id: "quality",
    description: "Show the current or final development quality report",
    group: "workflow",
  },
  {
    id: "review-change",
    description:
      "Run a fresh read-only independent review of the current development change (no approval, no modifications)",
    group: "workflow",
  },
  // --- godot ---
  {
    id: "godot",
    description: "Show the selected Godot installation and project compatibility",
    group: "godot",
  },
  {
    id: "godot-installations",
    description: "Show all discovered Godot installations and selection rationale",
    group: "godot",
  },
  { id: "godot-project", description: "Show the static Godot project profile", group: "godot" },
  { id: "godot-doctor", description: "Run bounded Godot diagnostics", group: "godot" },
  {
    id: "godot-probe",
    description:
      "Prepare one recovery-mode Godot project probe (approval required; reports unavailable when the platform cannot bind execution)",
    group: "godot",
  },
  {
    id: "godot-probe-status",
    description: "Show the recovery probe capability and last outcome",
    group: "godot",
  },
  {
    id: "godot-knowledge",
    description: "Show the exact-engine API knowledge status",
    group: "godot",
  },
  {
    id: "godot-knowledge-refresh",
    description:
      "Regenerate the exact-engine API knowledge profile (reports unavailable when the platform cannot bind execution)",
    group: "godot",
  },
  {
    id: "godot-api",
    description: "Search the exact engine's API documentation locally",
    group: "godot",
  },
  {
    id: "gdscript-check",
    description: "Check one .gd script with --check-only (approval required)",
    group: "godot",
  },
  {
    id: "gdscript-diagnostics",
    description:
      "Check the project's .gd scripts sequentially with --check-only (approval required)",
    group: "godot",
  },
  {
    id: "gdscript-lsp",
    description: "Start (approval required) or show the Godot GDScript language session",
    group: "godot",
  },
  {
    id: "gdscript-lsp-stop",
    description: "Gracefully stop the language session (no approval needed)",
    group: "godot",
  },
  {
    id: "gdscript-hover",
    description: "Hover information from the language session",
    group: "godot",
  },
  {
    id: "gdscript-complete",
    description: "Completion candidates from the language session",
    group: "godot",
  },
  {
    id: "gdscript-definition",
    description: "Definition locations from the language session",
    group: "godot",
  },
  // --- knowledge ---
  {
    id: "read-structure",
    description: "Show the GDScript declaration structure of a workspace file",
    group: "knowledge",
  },
  // --- references ---
  // --- doctor ---
  {
    id: "doctor",
    description:
      "Run read-only Siralos capability diagnostics (areas: runtime, configuration, providers, sandbox, workspace, godot, project, references, research, capabilities)",
    group: "doctor",
  },
  {
    id: "siralos",
    description: "Show the installed Siralos runtime identity and self-reference revision",
    group: "doctor",
  },
] as const satisfies readonly CommandCatalogEntry[];

export type CommandCatalog = Readonly<typeof COMMAND_CATALOG>;

/** Literal union of every catalogued command id (the CLI SlashCommand type). */
export type CommandId = (typeof COMMAND_CATALOG)[number]["id"];

/** Stable revision of the command surface, for self-reference fingerprints. */
export const COMMAND_CATALOG_REVISION: string = sha256Hex(
  canonicalizeJson(
    COMMAND_CATALOG.map((entry) => ({ id: entry.id, description: entry.description })),
  ),
);

export function catalogEntry(id: CommandId): CommandCatalogEntry | undefined {
  return COMMAND_CATALOG.find((entry) => entry.id === id);
}

/** All catalog ids in registration order (the CLI lookup list derives from this). */
export const COMMAND_CATALOG_IDS: readonly string[] = COMMAND_CATALOG.map((entry) => entry.id);
