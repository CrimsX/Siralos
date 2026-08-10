import {
  DEFAULT_DOCTOR_CHECK_TIMEOUT_MS,
  createCapabilityDoctor,
  createToolProjector,
  doctorExitCodeFor,
  type CapabilityPolicy,
  type CapabilityDiagnosticResult,
  type CheckpointStore,
  type DoctorArea,
  type DoctorReport,
  type DoctorSources,
  type GitInspector,
  type GodotDiagnosticResult,
  type GodotInspector,
  type ModelProvider,
  type PermissionRule,
  type ProjectedToolStatus,
  type ProviderDiagnosticResult,
  type ReferenceDiagnosticResult,
  type ReferenceRegistry,
  type RegisteredToolInfo,
  type ResearchDiagnosticResult,
  type ResearchService,
  type ResearchSourcePort,
  type RuntimeDiagnosticResult,
  type SandboxBackend,
  type SandboxDiagnosticResult,
  type SandboxProfile,
  type TaskRuntime,
  type TaskSnapshotDifference,
  type TaskRuntimeSnapshotSources,
  type WorkspaceDiagnosticResult,
} from "@solaris/core";
import { readConfigurationDiagnostics, readConfigurationFileState } from "@solaris/adapters";
import { readdir } from "node:fs/promises";
import { readInstalledSolarisVersion } from "./self-reference.js";

/**
 * Composition-root doctor wiring (Stage 3 milestone 6).
 *
 * Implements the core DoctorSources port from the REAL subsystem owners:
 * the sandbox backend, the Godot inspector, the reference registry, the
 * research service, the ToolProjector, the task runtime, the config
 * loader, Git, and the checkpoint store. Nothing here re-implements
 * subsystem logic — the doctor queries the authoritative owners and the
 * core CapabilityDoctor maps results to checks. Default operation is
 * read-only and offline: no network, no refreshes, no mutations.
 */

export interface CliDoctorDependencies {
  readonly workspaceRoot: string;
  readonly configPath: string;
  readonly policy: CapabilityPolicy;
  readonly profile: SandboxProfile;
  readonly sandbox: SandboxBackend;
  readonly provider: ModelProvider;
  readonly godot: GodotInspector;
  readonly references: ReferenceRegistry;
  readonly referenceConfigError: string | null;
  readonly research: ResearchService;
  readonly researchSources: readonly ResearchSourcePort[];
  readonly tasks: TaskRuntime;
  readonly taskSources: TaskRuntimeSnapshotSources;
  readonly git: GitInspector;
  readonly checkpoints: CheckpointStore;
  readonly tools: readonly RegisteredToolInfo[];
  /** Projection mode the capability trace is derived for (session default: generic). */
  readonly mode?: string;
}

async function readRuntimeDiagnostics(
  configPath: string,
  checkpoints: CheckpointStore,
): Promise<RuntimeDiagnosticResult> {
  const fileState = await readConfigurationFileState(configPath);
  const configurationFile: RuntimeDiagnosticResult["configurationFile"] =
    fileState === "readable"
      ? { state: "readable", detail: null }
      : fileState === "missing"
        ? { state: "missing", detail: null }
        : { state: "unreadable", detail: "configuration path is not a regular file" };
  let checkpointStoreAccessible = true;
  try {
    await checkpoints.list({ limit: 1 });
  } catch {
    checkpointStoreAccessible = false;
  }
  const nodeMajor = Number(process.versions.node.split(".")[0]);
  return {
    version: readInstalledVersion(),
    nodeMajor: Number.isFinite(nodeMajor) ? nodeMajor : 0,
    nodeSupported: Number.isFinite(nodeMajor) && nodeMajor >= 24,
    platform: process.platform,
    configurationFile,
    checkpointStoreAccessible,
  };
}

function readInstalledVersion(): string {
  // Same installed package.json source as the self-reference bootstrap.
  return readInstalledSolarisVersion();
}

function readProviderDiagnostics(provider: ModelProvider): ProviderDiagnosticResult {
  return {
    active: {
      profileId: provider.id,
      // Absent toolCalling stays unknown (never assumed); the deterministic
      // fake declares toolCalling: true, so the real wiring reports it.
      toolCalling: provider.toolCalling ?? null,
      state: "available",
      reason: null,
    },
    reviewProvider: {
      configured: false,
      resolved: false,
      profileId: null,
      state: "unsupported",
      reason: null,
    },
    credentials: [],
    endpoints: [],
    model: { id: null, toolCalling: null, contextBudgetTokens: null },
  };
}

async function readSandboxDiagnostics(
  sandbox: SandboxBackend,
  profile: SandboxProfile,
): Promise<SandboxDiagnosticResult> {
  const backend = await sandbox.inspect();
  // Mirror the authoritative execution gate (sandboxEnforcesBoundary in the
  // git adapter and the Godot runners): any sandboxed command requires the
  // FULL boundary — filesystem read+write restriction, network restriction,
  // and process-tree restriction — so a process-enabled profile requires all
  // four capabilities. Profiles without process execution (inspect) require
  // no sandboxed execution at all; the doctor reports that explicitly.
  const requiredCapabilitiesMissing: string[] = [];
  if (profile.process.enabled) {
    const required = [
      ["filesystemReadRestriction", "filesystemReadRestriction"],
      ["filesystemWriteRestriction", "filesystemWriteRestriction"],
      ["networkRestriction", "networkRestriction"],
      ["processTreeRestriction", "processTreeRestriction"],
    ] as const;
    for (const [capability, label] of required) {
      if (!backend.capabilities[capability]) {
        requiredCapabilitiesMissing.push(label);
      }
    }
  }
  return {
    backend,
    selectedProfileId: profile.id,
    profileRequiresProcess: profile.process.enabled,
    profileRequiresWrite: profile.filesystem.workspaceAccess !== "read-only",
    requiredCapabilitiesMissing,
    // Architecture-enforced: no unrestricted execution fallback exists in
    // this runtime (the sandbox service fails closed; there is no second
    // execution path).
    unrestrictedFallback: false,
  };
}

async function readWorkspaceDiagnostics(
  workspaceRoot: string,
  git: GitInspector,
  checkpoints: CheckpointStore,
): Promise<WorkspaceDiagnosticResult> {
  let gitAvailable: boolean | null = null;
  let gitState: string | null = null;
  try {
    const status = await git.inspectRepository();
    gitAvailable = status.gitAvailable;
    gitState = status.repositoryState;
  } catch {
    gitAvailable = false;
  }
  let checkpointStoreAccessible = true;
  try {
    await checkpoints.list({ limit: 1 });
  } catch {
    checkpointStoreAccessible = false;
  }
  // A single bounded root listing probes read access without scanning the
  // workspace (the doctor never walks or hashes repository contents).
  let readable = false;
  try {
    await readdir(workspaceRoot, { withFileTypes: false });
    readable = true;
  } catch {
    readable = false;
  }
  return {
    root: workspaceRoot,
    readable,
    // Structural facts of this runtime (architecture-enforced, not
    // probes): behavioral configuration is protected by the shared core
    // classifier wired into every mutation path; the revision registry is
    // session-local; workspace/reference namespaces are structurally
    // separated.
    protectedPathsActive: true,
    gitAvailable,
    gitState,
    checkpointStoreAccessible,
    revisionRegistryOperational: true,
    namespaceIntegrity: true,
  };
}

async function readGodotDiagnostics(
  godot: GodotInspector,
  workspaceRoot: string,
  policy: CapabilityPolicy,
): Promise<GodotDiagnosticResult> {
  const report = await godot.doctor();
  const selected = report.discovery.selected;
  const versionMatch =
    report.cache.enabled && selected !== null
      ? report.cache.cachedProfileCount > 0
        ? // The cache payload carries no fingerprint, so exactness cannot be
          // proven; report unknown rather than guessing "exact".
          {
            state: "unknown" as const,
            reason: "cached profile exists but fingerprint comparison is unavailable",
          }
        : { state: "absent" as const, reason: "no cached profile for the selected engine" }
      : {
          state: "absent" as const,
          reason: report.cache.enabled
            ? "no cached profile"
            : "the engine-profile cache is an explicit no-op at this stage",
        };
  return {
    report,
    versionMatch,
    projectRoot: workspaceRoot,
    policyRules: {
      recoveryProbe: policy.rules["godot.probe_project"],
      lsp: policy.rules["godot.lsp"],
      diagnose: policy.rules["godot.diagnose"],
    },
  };
}

function readReferenceDiagnostics(
  references: ReferenceRegistry,
  configError: string | null,
): ReferenceDiagnosticResult {
  return {
    configError,
    references: references.list().map((reference) => {
      const revision = references.revision(reference.id);
      return {
        alias: reference.alias,
        kind: reference.kind,
        trust: reference.trust,
        status: reference.status,
        failureReason: reference.failureReason,
        revision:
          revision === null
            ? null
            : revision.identity.kind === "local-directory"
              ? { kind: "fingerprint", fingerprint: revision.identity.fingerprint, commit: null }
              : { kind: "commit", fingerprint: null, commit: revision.identity.commit },
        materialized: referenceMaterializationLabel(reference),
      };
    }),
  };
}

function referenceMaterializationLabel(reference: {
  readonly kind: string;
  readonly status: string;
}): string {
  if (reference.status !== "ready") {
    return "n/a (not ready)";
  }
  if (reference.kind === "local-directory") {
    return "not-required (direct read-only root)";
  }
  return "unavailable (repository materialization is not available at this stage)";
}

function readResearchDiagnostics(
  research: ResearchService,
  sources: readonly ResearchSourcePort[],
  policy: CapabilityPolicy,
): ResearchDiagnosticResult {
  const rule: PermissionRule = policy.rules["research.fetch"];
  return {
    sources: sources.map((source) => ({
      kind: source.kind,
      id: source.id,
      label: source.label,
    })),
    policyRule: rule,
    gate: rule === "deny" ? "blocked_by_policy" : "allowed",
    // Both research adapters are implemented (repository = GitHub
    // known-file/release content over the bounded HTTPS transport; godot_docs
    // = Godot documentation pages). Availability is NOT the gate — the
    // research.fetch policy rule is, and every built-in profile denies it.
    adapterAvailability: sources.map((source) => ({
      kind: source.kind,
      available: true,
      reason: null,
    })),
    latestEvidenceCount: research.latestEvidence().length,
  };
}

function readCapabilityDiagnostics(
  tools: readonly RegisteredToolInfo[],
  policy: CapabilityPolicy,
  profile: SandboxProfile,
  mode: string,
): CapabilityDiagnosticResult {
  const projector = createToolProjector({ policy, profile });
  const projection = projector.project({
    mode: mode as never,
    registeredTools: tools,
  });
  const projectedByName = new Map(projection.tools.map((tool) => [tool.name, tool]));
  const projectedTools: ProjectedToolStatus[] = tools.map((tool) => {
    const projected = projectedByName.get(tool.definition.name);
    const state = projected?.visibility ?? "hidden";
    const rule = tool.capability === undefined ? null : policy.rules[tool.capability];
    let reason: string | null = null;
    if (state === "available") {
      reason = null;
    } else if (state === "gated") {
      reason = rule === "ask" ? `policy rule ${rule} (approval required)` : `policy rule ${rule}`;
    } else {
      reason = rule === undefined ? "not registered for projection" : `policy rule ${rule}`;
    }
    return { name: tool.definition.name, state, reason };
  });
  return {
    mode,
    trace: [
      { step: "registered", detail: `${tools.length} tools registered in the tool registry` },
      { step: "runtime profile", detail: profile.id },
      {
        step: "resource policy",
        detail: `capability rules of the active policy (profile ${profile.id})`,
      },
      { step: "task requirements", detail: `projection mode ${mode}` },
      { step: "model compatibility", detail: "tool calls supported by the active provider" },
      {
        step: "projected state",
        detail: `${projectedTools.filter((tool) => tool.state === "available").length} available, ${projectedTools.filter((tool) => tool.state === "gated").length} gated, ${projectedTools.filter((tool) => tool.state === "hidden").length} hidden`,
      },
    ],
    tools: projectedTools,
  };
}

function readTaskSnapshotDiagnostics(
  tasks: TaskRuntime,
  taskSources: TaskRuntimeSnapshotSources,
): {
  readonly activeTask: boolean;
  readonly runtimeVersion: string | null;
  readonly differences: readonly TaskSnapshotDifference[];
} {
  const task = tasks.latestTask();
  if (task === null) {
    return { activeTask: false, runtimeVersion: null, differences: [] };
  }
  const snapshot = task.runtimeSnapshot();
  const differences: TaskSnapshotDifference[] = [];
  if (snapshot.provider?.profileId !== taskSources.provider?.profileId) {
    differences.push({
      field: "provider profile",
      snapshotValue: snapshot.provider?.profileId ?? null,
      currentValue: taskSources.provider?.profileId ?? null,
    });
  }
  if (snapshot.sandboxProfileId !== taskSources.sandboxProfileId) {
    differences.push({
      field: "sandbox profile",
      snapshotValue: snapshot.sandboxProfileId,
      currentValue: taskSources.sandboxProfileId,
    });
  }
  if (snapshot.capabilityPolicyRevision !== taskSources.capabilityPolicyRevision) {
    differences.push({
      field: "capability policy revision",
      snapshotValue: snapshot.capabilityPolicyRevision,
      currentValue: taskSources.capabilityPolicyRevision,
    });
  }
  if (snapshot.workspaceIdentity !== taskSources.workspaceIdentity) {
    differences.push({
      field: "workspace identity",
      snapshotValue: snapshot.workspaceIdentity,
      currentValue: taskSources.workspaceIdentity,
    });
  }
  if ((snapshot.godotEngineFingerprint ?? null) !== (taskSources.godotEngineFingerprint ?? null)) {
    differences.push({
      field: "godot engine fingerprint",
      snapshotValue: snapshot.godotEngineFingerprint,
      currentValue: taskSources.godotEngineFingerprint,
    });
  }
  return { activeTask: true, runtimeVersion: snapshot.runtimeVersion, differences };
}

export function createCliDoctorSources(dependencies: CliDoctorDependencies): DoctorSources {
  return {
    runtime: () => readRuntimeDiagnostics(dependencies.configPath, dependencies.checkpoints),
    configuration: () => readConfigurationDiagnostics(dependencies.configPath),
    providers: () => Promise.resolve(readProviderDiagnostics(dependencies.provider)),
    sandbox: () => readSandboxDiagnostics(dependencies.sandbox, dependencies.profile),
    workspace: () =>
      readWorkspaceDiagnostics(
        dependencies.workspaceRoot,
        dependencies.git,
        dependencies.checkpoints,
      ),
    godot: () =>
      readGodotDiagnostics(dependencies.godot, dependencies.workspaceRoot, dependencies.policy),
    references: () =>
      Promise.resolve(
        readReferenceDiagnostics(dependencies.references, dependencies.referenceConfigError),
      ),
    research: () =>
      Promise.resolve(
        readResearchDiagnostics(
          dependencies.research,
          dependencies.researchSources,
          dependencies.policy,
        ),
      ),
    capabilities: () =>
      Promise.resolve(
        readCapabilityDiagnostics(
          dependencies.tools,
          dependencies.policy,
          dependencies.profile,
          dependencies.mode ?? "generic",
        ),
      ),
    tasks: () =>
      Promise.resolve(readTaskSnapshotDiagnostics(dependencies.tasks, dependencies.taskSources)),
  };
}

export function createCliDoctor(dependencies: CliDoctorDependencies) {
  return createCapabilityDoctor(createCliDoctorSources(dependencies));
}

export function doctorExitCode(report: DoctorReport): number {
  return doctorExitCodeFor(report);
}

export { DEFAULT_DOCTOR_CHECK_TIMEOUT_MS };

export function isDoctorArea(value: string): value is DoctorArea {
  return (
    value === "runtime" ||
    value === "configuration" ||
    value === "providers" ||
    value === "sandbox" ||
    value === "workspace" ||
    value === "godot" ||
    value === "project" ||
    value === "references" ||
    value === "research" ||
    value === "capabilities"
  );
}
