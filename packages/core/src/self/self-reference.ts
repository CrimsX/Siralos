import { COMMAND_CATALOG, COMMAND_CATALOG_REVISION } from "../commands/command-catalog.js";
import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import type { Capability, CapabilityPolicy, PermissionRule } from "../security/capability.js";
import { CAPABILITY_IDS } from "../security/capability.js";
import { SANDBOX_PROFILE_IDS, getBuiltInProfile } from "../security/profile.js";
import type { RegisteredToolInfo } from "../tools/tool-registry.js";
import type { ConfigSchemaSection } from "./config-schema-summary.js";
import { CONFIG_SCHEMA_REVISION } from "./config-schema-summary.js";

/**
 * Built-in Siralos SelfReference (Stage 3 milestone 6).
 *
 * `@siralos` is a host-generated, read-only description of the EXACT
 * installed runtime: version/build identity, the command catalog, the
 * configuration surface, capability names, sandbox profiles, the
 * registered workspace/Godot/reference/research tool surface, and the
 * Task Runtime concepts. It is deliberately NOT an external Reference
 * (which is untrusted supporting material) and it is NOT model training
 * memory — the model answers "what does this Siralos support?" from this
 * installed-version surface.
 *
 * The self-reference is retrieved on demand (self.read / self.search);
 * full documentation is never injected into prompts. Every section is
 * bounded, deterministic, and derived from authoritative runtime metadata
 * where practical (command catalog, capability ids, profile ids, tool
 * registry). It contains no secrets, no absolute sensitive paths, no
 * credentials, and no provider state. There is no mutation tool for it.
 */

export const SELF_REFERENCE_NAME = "@siralos";

export interface SiralosRuntimeIdentity {
  /** Installed package version (package.json), authoritative over model memory. */
  readonly version: string;
  /** Running Node.js major version. */
  readonly nodeMajor: number;
  /** OS family (process.platform). */
  readonly platform: string;
}

export type SelfReferenceSectionId =
  | "runtime"
  | "commands"
  | "configuration"
  | "capabilities"
  | "sandbox"
  | "workspace-tools"
  | "godot"
  | "references"
  | "research"
  | "tasks"
  | "doctor";

export interface SelfReferenceLine {
  readonly key: string;
  readonly value: string;
}

export interface SelfReferenceSection {
  readonly id: SelfReferenceSectionId;
  readonly title: string;
  readonly lines: readonly SelfReferenceLine[];
}

export interface SelfReferenceSearchMatch {
  readonly sectionId: SelfReferenceSectionId;
  readonly title: string;
  readonly lines: readonly SelfReferenceLine[];
}

export interface SelfReferenceInput {
  readonly runtime: SiralosRuntimeIdentity;
  /** The registered tool surface (definitions + capability bindings). */
  readonly registeredTools: readonly RegisteredToolInfo[];
  /** Active sandbox profile id (the profile the session runs under). */
  readonly sandboxProfileId: string;
  /** Active capability policy rules (the profile's policy). */
  readonly policy: CapabilityPolicy;
  /** Config-surface summary; defaults to the built-in summary. */
  readonly configSchema?: readonly ConfigSchemaSection[];
}

const MAX_TOOL_LINES = 200;
const MAX_LINE_CHARS = 240;
const MAX_SEARCH_MATCH_SECTIONS = 8;
const MAX_SEARCH_MATCH_LINES = 20;

function line(key: string, value: string): SelfReferenceLine {
  const bounded = value.length > MAX_LINE_CHARS ? `${value.slice(0, MAX_LINE_CHARS - 1)}…` : value;
  return { key, value: bounded };
}

function policyRule(policy: CapabilityPolicy, capability: Capability): PermissionRule {
  return policy.rules[capability];
}

export interface SelfReferenceRevisionParts {
  readonly version: string;
  readonly nodeMajor: number;
  readonly platform: string;
  readonly commandCatalogRevision: string;
  readonly configSchemaRevision: string;
  readonly capabilitySchemaRevision: string;
  readonly toolAbiRevision: string;
}

/**
 * Stable runtime revision/fingerprint of the self-reference. Any change
 * to the installed version, the command catalog, the configuration
 * surface, the capability set, or the registered tool ABI changes the
 * revision — the purpose is future reproducibility, not a cryptographic
 * manifest.
 */
export function computeSelfReferenceRevision(parts: SelfReferenceRevisionParts): string {
  return sha256Hex(
    canonicalizeJson({
      version: parts.version,
      nodeMajor: parts.nodeMajor,
      platform: parts.platform,
      commandCatalogRevision: parts.commandCatalogRevision,
      configSchemaRevision: parts.configSchemaRevision,
      capabilitySchemaRevision: parts.capabilitySchemaRevision,
      toolAbiRevision: parts.toolAbiRevision,
    }),
  );
}

export function toolAbiRevision(registeredTools: readonly RegisteredToolInfo[]): string {
  return sha256Hex(
    canonicalizeJson(
      registeredTools.slice(0, 512).map((tool) => ({
        name: tool.definition.name,
        description: tool.definition.description,
        inputSchema: tool.definition.inputSchema,
        capability: tool.capability,
      })),
    ),
  );
}

export interface SelfReference {
  readonly name: typeof SELF_REFERENCE_NAME;
  readonly runtime: SiralosRuntimeIdentity;
  /** Stable runtime revision/fingerprint (see computeSelfReferenceRevision). */
  readonly revision: string;
  readonly sections: readonly SelfReferenceSection[];
  readSection(id: SelfReferenceSectionId): SelfReferenceSection | null;
  /** Bounded case-insensitive token search over section lines. */
  search(query: string): readonly SelfReferenceSearchMatch[];
}

function runtimeSection(runtime: SiralosRuntimeIdentity, revision: string): SelfReferenceSection {
  return {
    id: "runtime",
    title: "Installed Siralos runtime",
    lines: [
      line("name", SELF_REFERENCE_NAME),
      line("version", runtime.version),
      line("node-major", String(runtime.nodeMajor)),
      line("platform", runtime.platform),
      line("revision", revision),
    ],
  };
}

function commandsSection(): SelfReferenceSection {
  const lines: SelfReferenceLine[] = [];
  let group: string | null = null;
  for (const entry of COMMAND_CATALOG) {
    if (entry.group !== group) {
      group = entry.group;
      lines.push(line(`group:${group}`, ""));
    }
    lines.push(line(`/${entry.id}`, entry.description));
  }
  return {
    id: "commands",
    title: "Interactive commands (derived from the command catalog)",
    lines,
  };
}

function configurationSection(configSchema: readonly ConfigSchemaSection[]): SelfReferenceSection {
  const lines: SelfReferenceLine[] = [];
  for (const section of configSchema) {
    lines.push(line(`section:${section.name}`, section.description));
    for (const key of section.keys) {
      const allowed = key.allowed === undefined ? "" : ` (${key.allowed.join("|")})`;
      lines.push(
        line(`  ${section.name}.${key.name}`, `${key.shape}${allowed} — ${key.description}`),
      );
    }
  }
  return {
    id: "configuration",
    title: "User configuration surface (user-level config file)",
    lines,
  };
}

function capabilitiesSection(policy: CapabilityPolicy): SelfReferenceSection {
  const lines: SelfReferenceLine[] = [];
  for (const capability of CAPABILITY_IDS) {
    lines.push(
      line(capability, `policy rule in active profile: ${policyRule(policy, capability)}`),
    );
  }
  lines.push(
    line(
      "note",
      "Capability ids are authoritative; support/configuration/availability/projection are distinct from the policy rule shown here.",
    ),
  );
  return { id: "capabilities", title: "Capability names", lines };
}

function sandboxSection(sandboxProfileId: string): SelfReferenceSection {
  const profile = (SANDBOX_PROFILE_IDS as readonly string[]).includes(sandboxProfileId)
    ? getBuiltInProfile(sandboxProfileId as (typeof SANDBOX_PROFILE_IDS)[number])
    : undefined;
  const lines: SelfReferenceLine[] = [
    line("active-profile", sandboxProfileId),
    line(
      "workspace-access",
      profile === undefined ? "unknown" : profile.filesystem.workspaceAccess,
    ),
    line("process-enabled", profile === undefined ? "unknown" : String(profile.process.enabled)),
    line("network-outbound", "deny (all built-in profiles)"),
  ];
  for (const id of SANDBOX_PROFILE_IDS) {
    lines.push(line(`profile:${id}`, id === sandboxProfileId ? "(active)" : ""));
  }
  return { id: "sandbox", title: "Sandbox profiles", lines };
}

function workspaceToolsSection(
  registeredTools: readonly RegisteredToolInfo[],
): SelfReferenceSection {
  const lines: SelfReferenceLine[] = [];
  for (const tool of registeredTools.slice(0, MAX_TOOL_LINES)) {
    lines.push(line(tool.definition.name, `${tool.definition.description} [${tool.capability}]`));
  }
  if (registeredTools.length > MAX_TOOL_LINES) {
    lines.push(line("note", `${registeredTools.length - MAX_TOOL_LINES} further tools not listed`));
  }
  return { id: "workspace-tools", title: "Registered tool surface", lines };
}

const GODOT_LINES: readonly SelfReferenceLine[] = [
  line("engine-discovery", "fixed-name PATH discovery, canonicalization, SHA-256 fingerprinting"),
  line("engine-selection", "deterministic selection policy over ranked candidates"),
  line(
    "static-project-profiling",
    "bounded project.godot profile (GDScript/.NET, plugins, autoloads, GDExtension)",
  ),
  line(
    "engine-probing",
    "unavailable at this stage: execution cannot be identity-bound (no engine profile is produced)",
  ),
  line(
    "api-knowledge",
    "unavailable: generation runner never spawns; the knowledge cache is an explicit no-op",
  ),
  line("gdscript-diagnostics", "unavailable: check-only runner never spawns"),
  line("lsp-session", "unavailable: session runner never spawns; loopback profile is lsp-only"),
  line(
    "recovery-probe",
    "unavailable: recovery runner never spawns; approval protocol exists but execution fails closed",
  ),
  line(
    "development",
    "development workflow is read-only inspected; source mutation application fails closed",
  ),
  line(
    "no-project-execution",
    "Siralos does not open, import, execute, or run a project at this stage",
  ),
];

function godotSection(): SelfReferenceSection {
  return {
    id: "godot",
    title: "Godot capability status (installed-runtime truth)",
    lines: GODOT_LINES,
  };
}

const REFERENCES_LINES: readonly SelfReferenceLine[] = [
  line("name-space", "@reference/<alias> names are never filesystem paths"),
  line("alias-pattern", "^[a-z][a-z0-9._-]{1,63}$ (max 16 references)"),
  line("kinds", "local-directory | repository (pinned commit/tag; mutable refs are refused)"),
  line("trust-classes", "explicit-user | trusted-project | untrusted-project | managed"),
  line("containment", "reference roots must resolve outside the workspace namespace"),
  line("revisions", "immutable revisions; refresh is explicit and never silent"),
  line(
    "materialization",
    "local directories are direct read-only roots; repository materialization is unavailable at this stage",
  ),
];

function referencesSection(): SelfReferenceSection {
  return { id: "references", title: "External reference surface", lines: REFERENCES_LINES };
}

const RESEARCH_LINES: readonly SelfReferenceLine[] = [
  line("capability", "research.fetch — denied by every built-in profile"),
  line(
    "source-kinds",
    "repository (GitHub known-file/release content) | godot_docs (Godot documentation pages)",
  ),
  line("transport", "single bounded https-only transport; no other network surface"),
  line("bounds", "downloads/documents/sections bounded with explicit truncation disclosure"),
  line("provenance", "requested vs resolved revision recorded; stale async results discarded"),
];

function researchSection(): SelfReferenceSection {
  return { id: "research", title: "Research source surface", lines: RESEARCH_LINES };
}

const TASKS_LINES: readonly SelfReferenceLine[] = [
  line("task-contract", "revisioned request/constraints/acceptance criteria/pause policy"),
  line("task-state", "authoritative single-owner state with phases, bounded steps, findings"),
  line("evidence", "evidence-backed step completion with task-scoped refs"),
  line(
    "snapshot",
    "immutable runtime snapshot at task start (provider/sandbox/policy/workspace/engine/workflow)",
  ),
  line("events", "append-only typed activity log (never event sourcing)"),
  line("dispositions", "model complete still passes the host completion gate"),
];

function tasksSection(): SelfReferenceSection {
  return { id: "tasks", title: "Task Runtime concepts", lines: TASKS_LINES };
}

const DOCTOR_LINES: readonly SelfReferenceLine[] = [
  line(
    "areas",
    "runtime, configuration, providers, sandbox, workspace, godot, project, references, research, capabilities",
  ),
  line("statuses", "pass | warn | fail | skip"),
  line("read-only", "doctor never modifies config, workspace, checkpoints, or task snapshots"),
  line("offline", "default doctor performs no network requests and no live probes"),
  line(
    "exit-codes",
    "0 = no failures, 1 = one or more failures, 2 = doctor invocation/config failure",
  ),
  line("reports", "--json machine-readable output; --report-safe sanitized report for bug reports"),
];

function doctorSection(): SelfReferenceSection {
  return { id: "doctor", title: "CapabilityDoctor surface", lines: DOCTOR_LINES };
}

export function createSelfReference(input: SelfReferenceInput): SelfReference {
  const configSchema = input.configSchema ?? [];
  const sections: readonly SelfReferenceSection[] = [
    runtimeSection(input.runtime, ""), // revision filled below
    commandsSection(),
    configurationSection(configSchema),
    capabilitiesSection(input.policy),
    sandboxSection(input.sandboxProfileId),
    workspaceToolsSection(input.registeredTools),
    godotSection(),
    referencesSection(),
    researchSection(),
    tasksSection(),
    doctorSection(),
  ];
  const revision = computeSelfReferenceRevision({
    version: input.runtime.version,
    nodeMajor: input.runtime.nodeMajor,
    platform: input.runtime.platform,
    commandCatalogRevision: COMMAND_CATALOG_REVISION,
    configSchemaRevision: CONFIG_SCHEMA_REVISION,
    capabilitySchemaRevision: sha256Hex(canonicalizeJson(CAPABILITY_IDS)),
    toolAbiRevision: toolAbiRevision(input.registeredTools),
  });
  const withRevision = sections.map((section) =>
    section.id === "runtime"
      ? {
          ...section,
          lines: section.lines.map((l) => (l.key === "revision" ? { ...l, value: revision } : l)),
        }
      : section,
  );
  const readSection = (id: SelfReferenceSectionId): SelfReferenceSection | null =>
    withRevision.find((section) => section.id === id) ?? null;
  const search = (query: string): readonly SelfReferenceSearchMatch[] => {
    const tokens = query
      .toLowerCase()
      .split(/\s+/)
      .filter((token) => token.length > 0);
    if (tokens.length === 0) {
      return [];
    }
    const matches: SelfReferenceSearchMatch[] = [];
    for (const section of withRevision) {
      if (matches.length >= MAX_SEARCH_MATCH_SECTIONS) {
        break;
      }
      const matched = section.lines.filter((entry) =>
        tokens.some((token) => `${entry.key} ${entry.value}`.toLowerCase().includes(token)),
      );
      if (matched.length > 0) {
        matches.push({
          sectionId: section.id,
          title: section.title,
          lines: matched.slice(0, MAX_SEARCH_MATCH_LINES),
        });
      }
    }
    return matches;
  };
  return Object.freeze({
    name: SELF_REFERENCE_NAME,
    runtime: { ...input.runtime },
    revision,
    sections: withRevision,
    readSection,
    search,
  });
}

/**
 * Read-only model-facing port for the self-reference tools. The concrete
 * SelfReference implements it; the tools never see a mutation surface.
 */
export interface SelfReferencePort {
  readSection(id: SelfReferenceSectionId): SelfReferenceSection | null;
  search(query: string): readonly SelfReferenceSearchMatch[];
  readonly name: typeof SELF_REFERENCE_NAME;
  readonly revision: string;
}
