import { buildCapabilitySnapshot } from "./capability-state.js";
import type {
  CapabilityDiagnosticResult,
  ConfigurationDiagnosticResult,
  DoctorSources,
  GodotDiagnosticResult,
  ProviderDiagnosticResult,
  ReferenceDiagnosticResult,
  ResearchDiagnosticResult,
  RuntimeDiagnosticResult,
  SandboxDiagnosticResult,
  TaskSnapshotDiagnosticResult,
  WorkspaceDiagnosticResult,
} from "./doctor-ports.js";
import {
  DOCTOR_AREAS,
  DOCTOR_SCHEMA_VERSION,
  countDoctorReport,
  normalizeDoctorRequest,
  type CapabilitySnapshot,
  type DoctorArea,
  type DoctorCheckResult,
  type DoctorDetail,
  type DoctorRemediation,
  type DoctorReport,
  type DoctorRequest,
  type DoctorStatus,
} from "./doctor-model.js";

/**
 * Deterministic, read-only CapabilityDoctor (Stage 3 milestone 6).
 *
 * The doctor orchestrates the authoritative subsystem owners through
 * `DoctorSources` and maps their results to structured checks. It never
 * re-implements provider/sandbox/Godot/reference/research/projection
 * logic, never imports concrete implementations, never performs network
 * requests, never mutates anything, and never creates checkpoints.
 * Every probe is bounded by a per-check timeout.
 */

export const DEFAULT_DOCTOR_CHECK_TIMEOUT_MS = 5_000;

export interface CapabilityDoctorOptions {
  /** Per-check timeout in milliseconds (default 5000). */
  readonly checkTimeoutMs?: number;
}

export interface CapabilityDoctor {
  inspect(request: DoctorRequest, signal?: AbortSignal): Promise<DoctorReport>;
}

export class DoctorTimeoutError extends Error {
  readonly code = "doctor_timeout";
  constructor(readonly timeoutMs: number) {
    super(`doctor check timed out after ${timeoutMs}ms`);
    this.name = "DoctorTimeoutError";
  }
}

export class DoctorCancelledError extends Error {
  readonly code = "doctor_cancelled";
  constructor() {
    super("doctor check cancelled");
    this.name = "DoctorCancelledError";
  }
}

function withTimeout<T>(promise: Promise<T>, timeoutMs: number, signal?: AbortSignal): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    if (signal !== undefined && signal.aborted) {
      reject(new DoctorCancelledError());
      return;
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      reject(new DoctorTimeoutError(timeoutMs));
    }, timeoutMs);
    const onAbort = () => {
      clearTimeout(timer);
      reject(new DoctorCancelledError());
    };
    if (signal !== undefined) {
      signal.addEventListener("abort", onAbort, { once: true });
    }
    promise.then(
      (value) => {
        clearTimeout(timer);
        signal?.removeEventListener("abort", onAbort);
        resolve(value);
      },
      (error: unknown) => {
        clearTimeout(timer);
        signal?.removeEventListener("abort", onAbort);
        reject(error instanceof Error ? error : new Error(String(error)));
      },
    );
  });
}

type AreaOutcome<T> =
  | { readonly kind: "ok"; readonly value: T }
  | { readonly kind: "timeout" }
  | { readonly kind: "cancelled" }
  | { readonly kind: "error"; readonly message: string };

function check(
  id: string,
  area: DoctorArea,
  status: DoctorStatus,
  summary: string,
  details?: readonly DoctorDetail[],
  remediation?: readonly DoctorRemediation[],
): DoctorCheckResult {
  return {
    id,
    area,
    status,
    summary,
    ...(details === undefined ? {} : { details }),
    ...(remediation === undefined ? {} : { remediation }),
  };
}

function boundedMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return message.length > 300 ? `${message.slice(0, 299)}…` : message;
}

// --- area check builders (pure; one per area) ---

function runtimeChecks(result: RuntimeDiagnosticResult): DoctorCheckResult[] {
  return [
    check(
      "runtime.node_version",
      "runtime",
      result.nodeSupported ? "pass" : "fail",
      result.nodeSupported
        ? `Node.js ${result.nodeMajor} is supported`
        : `Node.js ${result.nodeMajor} is not a supported major (>= 24 required)`,
      [{ label: "node-major", value: String(result.nodeMajor) }],
      result.nodeSupported
        ? undefined
        : [
            {
              title: "Use a supported Node.js major",
              steps: ["Install Node.js 24 or newer and relaunch Solaris."],
            },
          ],
    ),
    check(
      "runtime.package_version",
      "runtime",
      result.version.length > 0 ? "pass" : "fail",
      result.version.length > 0
        ? `Installed Solaris runtime version ${result.version}`
        : "Installed Solaris runtime version is unknown",
      [{ label: "version", value: result.version }],
    ),
    check(
      "runtime.configuration_file",
      "runtime",
      result.configurationFile.state === "unreadable" ? "fail" : "pass",
      result.configurationFile.state === "readable"
        ? "User configuration is readable"
        : result.configurationFile.state === "missing"
          ? "No user configuration file; defaults apply"
          : "User configuration file is unreadable",
      result.configurationFile.detail === null
        ? undefined
        : [{ label: "detail", value: result.configurationFile.detail }],
    ),
    check(
      "runtime.checkpoint_store",
      "runtime",
      result.checkpointStoreAccessible ? "pass" : "fail",
      result.checkpointStoreAccessible
        ? "Checkpoint store is accessible"
        : "Checkpoint store is not accessible",
    ),
  ];
}

function configurationChecks(result: ConfigurationDiagnosticResult): DoctorCheckResult[] {
  const validityDetails: DoctorDetail[] = [];
  for (const error of result.validationErrors) {
    validityDetails.push({ label: "validation-error", value: error });
  }
  for (const field of result.unknownFields) {
    validityDetails.push({ label: "unknown-field", value: field });
  }
  const invalid = result.validationErrors.length > 0;
  const unknownOnly = !invalid && result.unknownFields.length > 0;
  const sections: DoctorDetail[] = [];
  for (const section of result.sections) {
    sections.push({
      label: section.name,
      value: section.present ? "configured" : "absent (defaults)",
    });
  }
  const credentials = credentialCheck(
    "configuration.credentials",
    "configuration",
    result.credentialRefs,
  );
  return [
    check(
      "configuration.validity",
      "configuration",
      invalid ? "fail" : unknownOnly ? "warn" : "pass",
      invalid
        ? "Configuration is invalid"
        : unknownOnly
          ? "Configuration has unsupported fields"
          : "Configuration is valid",
      validityDetails,
      invalid
        ? [
            {
              title: "Fix the configuration file",
              steps: ["Correct the reported validation errors and relaunch Solaris."],
            },
          ]
        : undefined,
    ),
    check("configuration.sections", "configuration", "pass", "Configuration sections", sections),
    credentials,
  ];
}

function credentialCheck(
  id: string,
  area: DoctorArea,
  refs: readonly {
    readonly name: string;
    readonly referenced: boolean;
    readonly present: boolean;
  }[],
): DoctorCheckResult {
  if (refs.length === 0) {
    return check(
      id,
      area,
      "skip",
      "No credential environment variables are referenced by this runtime's configuration",
    );
  }
  const missing = refs.filter((ref) => ref.referenced && !ref.present);
  const details: DoctorDetail[] = refs.map((ref) => ({
    label: ref.name,
    value: `referenced: ${ref.referenced ? "yes" : "no"}, present: ${ref.present ? "yes" : "no"}`,
  }));
  return check(
    id,
    area,
    missing.length === 0 ? "pass" : "fail",
    missing.length === 0
      ? "All referenced credential environment variables are present"
      : `${missing.length} referenced credential environment variable${missing.length === 1 ? " is" : "s are"} missing`,
    details,
    missing.length === 0
      ? undefined
      : [
          {
            title: "Provide the missing credentials",
            steps: missing.map(
              (ref) => `Set ${ref.name} (never paste values into reports or chat).`,
            ),
          },
        ],
  );
}

function providerChecks(result: ProviderDiagnosticResult): DoctorCheckResult[] {
  const active = check(
    "providers.active",
    "providers",
    result.active.state === "available" ? "pass" : "fail",
    result.active.state === "available"
      ? `Provider profile ${result.active.profileId} is ready`
      : `Provider profile ${result.active.profileId} is ${result.active.state}`,
    [
      { label: "profile", value: result.active.profileId },
      {
        label: "tool-calls",
        value:
          result.active.toolCalling === null
            ? "unknown"
            : result.active.toolCalling
              ? "supported"
              : "not supported",
      },
      { label: "state", value: result.active.state },
    ],
    result.active.reason === null
      ? undefined
      : [{ title: "Provider unavailable", steps: [result.active.reason] }],
  );
  const review = check(
    "providers.review",
    "providers",
    result.reviewProvider.configured ? (result.reviewProvider.resolved ? "pass" : "fail") : "skip",
    result.reviewProvider.configured
      ? result.reviewProvider.resolved
        ? `Review provider ${result.reviewProvider.profileId} resolves`
        : `Configured review provider ${result.reviewProvider.profileId ?? "(unknown)"} does not resolve`
      : "No review provider configured",
    [{ label: "state", value: result.reviewProvider.state }],
    result.reviewProvider.configured && !result.reviewProvider.resolved
      ? [
          {
            title: "Fix the review provider",
            steps: [
              `Configure a resolvable provider id (reported reason: ${result.reviewProvider.reason ?? "unknown"}).`,
            ],
          },
        ]
      : undefined,
  );
  const credentials = credentialCheck("providers.credentials", "providers", result.credentials);
  const endpoints: DoctorCheckResult =
    result.endpoints.length === 0
      ? check(
          "providers.endpoints",
          "providers",
          "skip",
          "No provider endpoints are configured in this runtime",
        )
      : (() => {
          const invalid = result.endpoints.filter((endpoint) => endpoint.valid === false);
          const details: DoctorDetail[] = result.endpoints.map((endpoint) => ({
            label: endpoint.label,
            value: `https: ${endpoint.https ? "yes" : "no"}, loopback: ${endpoint.loopback ? "yes" : "no"}, valid: ${endpoint.valid === null ? "unknown" : endpoint.valid ? "yes" : "no"}`,
          }));
          return check(
            "providers.endpoints",
            "providers",
            invalid.length === 0 ? "pass" : "fail",
            invalid.length === 0
              ? "All provider endpoints satisfy the HTTPS/loopback transport rule"
              : `${invalid.length} provider endpoint${invalid.length === 1 ? "" : "s"} violate the HTTPS/loopback transport rule`,
            details,
            invalid.length === 0
              ? undefined
              : [
                  {
                    title: "Use a valid endpoint",
                    steps: [
                      "Remote endpoints must use HTTPS; plain-HTTP is allowed only for loopback addresses.",
                    ],
                  },
                ],
          );
        })();
  const model: DoctorCheckResult =
    result.model.id === null
      ? check(
          "providers.model_compatibility",
          "providers",
          "skip",
          "No model route is configured in this runtime",
        )
      : result.model.toolCalling === false
        ? check(
            "providers.model_compatibility",
            "providers",
            "fail",
            `Configured model ${result.model.id} does not support tool calls, which the current workflow requires`,
            [
              { label: "model", value: result.model.id },
              { label: "tool-calls", value: "not supported" },
            ],
            [
              {
                title: "Select a tool-calling model",
                steps: [
                  `Configure a model that supports tool calls instead of ${result.model.id}.`,
                ],
              },
            ],
          )
        : check(
            "providers.model_compatibility",
            "providers",
            "pass",
            `Configured model ${result.model.id} supports the required tool-calling workflow`,
            [
              { label: "model", value: result.model.id },
              {
                label: "tool-calls",
                value: result.model.toolCalling === null ? "unknown" : "supported",
              },
              ...(result.model.contextBudgetTokens === null
                ? []
                : [
                    {
                      label: "context-budget",
                      value: `${result.model.contextBudgetTokens} tokens`,
                    },
                  ]),
            ],
          );
  return [active, review, credentials, endpoints, model];
}

function sandboxChecks(result: SandboxDiagnosticResult): DoctorCheckResult[] {
  const backendStatus: DoctorStatus =
    result.backend.state === "available"
      ? "pass"
      : result.backend.state === "degraded"
        ? "warn"
        : result.backend.state === "setup-required" || result.backend.state === "dependency-missing"
          ? "warn"
          : "fail";
  const enforcement: DoctorCheckResult =
    result.requiredCapabilitiesMissing.length === 0
      ? check(
          "sandbox.required_enforcement",
          "sandbox",
          "pass",
          `Profile ${result.selectedProfileId} enforcement is satisfied by the backend`,
          [
            {
              label: "filesystem-read-restriction",
              value: String(result.backend.capabilities.filesystemReadRestriction),
            },
            {
              label: "filesystem-write-restriction",
              value: String(result.backend.capabilities.filesystemWriteRestriction),
            },
            {
              label: "network-restriction",
              value: String(result.backend.capabilities.networkRestriction),
            },
            {
              label: "process-tree-restriction",
              value: String(result.backend.capabilities.processTreeRestriction),
            },
          ],
        )
      : check(
          "sandbox.required_enforcement",
          "sandbox",
          "fail",
          `Profile ${result.selectedProfileId} requires enforcement the backend cannot provide`,
          result.requiredCapabilitiesMissing.map((capability) => ({
            label: "missing",
            value: capability,
          })),
          [
            {
              title: "Restore required enforcement",
              steps: [
                `The backend reports missing enforcement: ${result.requiredCapabilitiesMissing.join(", ")}. Solaris fails closed until enforcement is available.`,
              ],
            },
          ],
        );
  return [
    check(
      "sandbox.backend",
      "sandbox",
      backendStatus,
      result.backend.state === "available"
        ? `Sandbox backend ${result.backend.backendId} is available`
        : `Sandbox backend ${result.backend.backendId} is ${result.backend.state}`,
      [
        { label: "backend", value: result.backend.backendId },
        { label: "state", value: result.backend.state },
        { label: "profile", value: result.selectedProfileId },
        { label: "platform", value: result.backend.platform },
      ],
      result.backend.state === "available" || result.backend.state === "unsupported"
        ? undefined
        : result.backend.message === undefined
          ? undefined
          : [{ title: "Sandbox backend status", steps: [result.backend.message] }],
    ),
    enforcement,
    check(
      "sandbox.unrestricted_fallback",
      "sandbox",
      result.unrestrictedFallback ? "fail" : "pass",
      result.unrestrictedFallback
        ? "An unrestricted execution fallback exists — execution is NOT fail-closed"
        : "No unrestricted execution fallback exists; execution fails closed when the backend cannot enforce",
    ),
  ];
}

function workspaceChecks(result: WorkspaceDiagnosticResult): DoctorCheckResult[] {
  return [
    check(
      "workspace.identity",
      "workspace",
      result.root === null ? "fail" : "pass",
      result.root === null ? "Workspace root is unknown" : "Workspace root is canonicalized",
      result.root === null ? undefined : [{ label: "root", value: result.root }],
    ),
    check(
      "workspace.read",
      "workspace",
      result.readable ? "pass" : "fail",
      result.readable ? "Workspace is readable" : "Workspace is not readable",
    ),
    check(
      "workspace.protected_paths",
      "workspace",
      result.protectedPathsActive ? "pass" : "fail",
      result.protectedPathsActive
        ? "Behavioral configuration (AGENTS.md, .solaris/**) is protected from mutation"
        : "Behavioral configuration protection is not active",
    ),
    check(
      "workspace.git",
      "workspace",
      result.gitAvailable === true ? "pass" : "skip",
      result.gitAvailable === true
        ? `Git is available${result.gitState === null ? "" : ` (${result.gitState})`}`
        : result.gitAvailable === false
          ? "Git is unavailable (read-only inspection is unaffected)"
          : "Git availability not probed",
    ),
    check(
      "workspace.checkpoints",
      "workspace",
      result.checkpointStoreAccessible ? "pass" : "fail",
      result.checkpointStoreAccessible
        ? "Checkpoint store is accessible"
        : "Checkpoint store is not accessible",
    ),
    check(
      "workspace.namespaces",
      "workspace",
      result.namespaceIntegrity ? "pass" : "fail",
      result.namespaceIntegrity
        ? "Workspace/reference namespace separation is intact"
        : "Workspace/reference namespace separation is broken",
    ),
  ];
}

function godotChecks(result: GodotDiagnosticResult): DoctorCheckResult[] {
  const discovery = result.report.discovery;
  const selected = discovery.selected;
  const discoveryDetails: DoctorDetail[] = [
    { label: "candidates", value: String(discovery.candidates.length) },
    { label: "configured", value: String(discovery.configuration.configuredCount) },
    { label: "discover-on-path", value: discovery.configuration.discoverOnPath ? "yes" : "no" },
    ...discovery.configuration.overrides.map((override) => ({
      label: "override",
      value: override,
    })),
  ];
  const discoveryStatus: DoctorStatus = discovery.diagnostics.length === 0 ? "pass" : "warn";
  const selectionDetails: DoctorDetail[] = selected
    ? [
        { label: "installation-id", value: selected.installationId },
        { label: "version", value: selected.version?.raw ?? "unknown" },
        { label: "edition", value: selected.edition ?? "unknown" },
        { label: "support", value: selected.support ?? "unknown" },
        { label: "fingerprint", value: selected.fingerprint ?? "none" },
        { label: "profiled", value: selected.profiled ? "yes" : "no" },
      ]
    : [];
  const selectionStatus: DoctorStatus = selected
    ? result.report.degradedCapabilities.length > 0
      ? "warn"
      : "pass"
    : discovery.configuration.activeInstallation !== null
      ? "warn"
      : "skip";
  const versionMatch: DoctorCheckResult =
    result.versionMatch.state === "exact"
      ? check(
          "godot.version_match",
          "godot",
          "pass",
          "Cached API/docs surface matches the selected engine exactly",
          [{ label: "api-index", value: "exact match" }],
        )
      : result.versionMatch.state === "stale"
        ? check(
            "godot.version_match",
            "godot",
            "warn",
            "Cached API/docs surface is stale for the selected engine",
            [
              {
                label: "api-index",
                value: `stale${result.versionMatch.reason === null ? "" : ` (${result.versionMatch.reason})`}`,
              },
            ],
            [
              {
                title: "Regenerate the API knowledge profile",
                steps: [
                  "Regenerate the exact-engine API knowledge profile when execution is available (godot-knowledge-refresh).",
                ],
              },
            ],
          )
        : result.versionMatch.state === "absent"
          ? check(
              "godot.version_match",
              "godot",
              "skip",
              "No API cache exists for the selected engine",
              [
                {
                  label: "api-index",
                  value: `absent${result.versionMatch.reason === null ? "" : ` (${result.versionMatch.reason})`}`,
                },
              ],
            )
          : check("godot.version_match", "godot", "skip", "API cache version-match is unknown", [
              { label: "api-index", value: result.versionMatch.reason ?? "unknown" },
            ]);
  return [
    check(
      "godot.discovery",
      "godot",
      discoveryStatus,
      discovery.diagnostics.length === 0
        ? `Godot discovery completed (${discovery.candidates.length} candidate${discovery.candidates.length === 1 ? "" : "s"})`
        : `Godot discovery completed with ${discovery.diagnostics.length} diagnostic${discovery.diagnostics.length === 1 ? "" : "s"}`,
      discoveryDetails,
    ),
    check(
      "godot.selection",
      "godot",
      selectionStatus,
      selected
        ? `Selected Godot installation ${selected.installationId} is valid`
        : discovery.configuration.activeInstallation !== null
          ? "A configured Godot installation is not selected"
          : "No Godot installation selected",
      selectionDetails,
      selected === null && discovery.configuration.activeInstallation !== null
        ? [{ title: "Selection failed", steps: [...discovery.rationale.slice(0, 4)] }]
        : undefined,
    ),
    versionMatch,
  ];
}

function projectChecks(result: GodotDiagnosticResult): DoctorCheckResult[] {
  const project = result.report.project;
  const found = project.detected;
  const details: DoctorDetail[] = found
    ? [
        { label: "canonical-root", value: result.projectRoot ?? "unknown" },
        { label: "project-file-sha256", value: project.projectFileSha256 ?? "none" },
        { label: "name", value: project.name ?? "(unnamed)" },
        { label: "language-profile", value: project.languageProfile },
        { label: "declared-engine", value: project.declaredEngineVersion?.raw ?? "unknown" },
        { label: "autoloads", value: String(project.autoloads.length) },
        { label: "editor-plugins", value: String(project.enabledEditorPlugins.length) },
        {
          label: "gdextension",
          value: String(project.executableContent.gdextensionDescriptors.length),
        },
        {
          label: "dotnet",
          value: project.executableContent.dotnetProjectFiles.length > 0 ? "yes" : "no",
        },
        { label: "tool-scripts", value: String(project.executableContent.toolScripts.length) },
      ]
    : [];
  const profileWarnings = found && project.warnings.length > 0 ? "warn" : "pass";
  const recovery: DoctorCheckResult = check(
    "project.recovery_lsp",
    "project",
    "skip",
    "Recovery-mode probing, check-only diagnostics, and LSP sessions are not executed by the doctor",
    [
      {
        label: "recovery-probe-execution",
        value: `${result.report.recoveryProbe.state}${result.report.recoveryProbe.reason === null ? "" : ` (${result.report.recoveryProbe.reason})`}`,
      },
      {
        label: "recovery-probe-policy",
        value: `rule ${result.policyRules.recoveryProbe} (approval required before any execution)`,
      },
      {
        label: "check-only-execution",
        value: `${result.report.diagnostics.state}${result.report.diagnostics.reason === null ? "" : ` (${result.report.diagnostics.reason})`}`,
      },
      { label: "check-only-policy", value: `rule ${result.policyRules.diagnose}` },
      {
        label: "api-knowledge-execution",
        value: `${result.report.knowledge.state}${result.report.knowledge.reason === null ? "" : ` (${result.report.knowledge.reason})`}`,
      },
      { label: "lsp-execution", value: "unavailable (session runner never spawns)" },
      { label: "lsp-policy", value: `rule ${result.policyRules.lsp}` },
      {
        label: "api-cache",
        value: result.report.cache.enabled
          ? `enabled (${result.report.cache.cachedProfileCount} profiles)`
          : "disabled (explicit no-op)",
      },
    ],
  );
  return [
    check(
      "project.profile",
      "project",
      found ? profileWarnings : "skip",
      // The absolute project root stays in details only; summaries must
      // never carry absolute paths (the safe report keeps only summaries).
      found
        ? "Godot project detected in the workspace"
        : "No Godot project (project.godot) in the workspace",
      details,
    ),
    recovery,
  ];
}

function referenceChecks(result: ReferenceDiagnosticResult): DoctorCheckResult[] {
  const config = check(
    "references.configuration",
    "references",
    result.configError === null ? "pass" : "fail",
    result.configError === null
      ? "Reference configuration is valid"
      : "Reference configuration is invalid",
    result.configError === null ? undefined : [{ label: "error", value: result.configError }],
  );
  const revisionDetails: DoctorDetail[] = result.references.map((entry) => ({
    label: entry.alias,
    value: `kind: ${entry.kind}, trust: ${entry.trust}, status: ${entry.status}${entry.failureReason === null ? "" : ` (${entry.failureReason})`}, revision: ${entry.revision === null ? "none" : `${entry.revision.kind} ${entry.revision.fingerprint ?? entry.revision.commit ?? ""}`}, materialized: ${entry.materialized}`,
  }));
  const failed = result.references.filter(
    (entry) => entry.status === "declined" || entry.status === "resolution-failed",
  );
  const revisions: DoctorCheckResult =
    result.references.length === 0
      ? check("references.revisions", "references", "skip", "No references configured")
      : check(
          "references.revisions",
          "references",
          failed.length === 0 ? "pass" : "warn",
          failed.length === 0
            ? `${result.references.length} configured reference${result.references.length === 1 ? "" : "s"} report ${result.references.filter((entry) => entry.status === "ready").length} ready`
            : `${failed.length} reference${failed.length === 1 ? "" : "s"} failed to resolve`,
          revisionDetails,
          failed.length === 0
            ? undefined
            : [
                {
                  title: "Fix or remove failed references",
                  steps: [
                    "Resolve or remove the failed reference declarations (the doctor never fetches or refreshes references).",
                  ],
                },
              ],
        );
  return [config, revisions];
}

function researchChecks(result: ResearchDiagnosticResult): DoctorCheckResult[] {
  const policy = check(
    "research.policy",
    "research",
    "pass",
    result.gate === "blocked_by_policy"
      ? "Research fetching is blocked by policy (no network requests are made by the doctor)"
      : "Research fetching is permitted by policy",
    [
      { label: "rule", value: result.policyRule },
      { label: "gate", value: result.gate },
      { label: "latest-evidence", value: String(result.latestEvidenceCount) },
    ],
  );
  const sources: DoctorCheckResult =
    result.sources.length === 0
      ? check("research.sources", "research", "skip", "No research sources registered")
      : check(
          "research.sources",
          "research",
          "pass",
          `${result.sources.length} research source${result.sources.length === 1 ? "" : "s"} registered (adapter availability reported; nothing is fetched)`,
          result.adapterAvailability.map((adapter) => ({
            label: adapter.kind,
            value: adapter.available
              ? "available"
              : `unavailable${adapter.reason === null ? "" : ` (${adapter.reason})`}`,
          })),
        );
  return [policy, sources];
}

function capabilitiesChecks(
  result: CapabilityDiagnosticResult,
  taskResult: TaskSnapshotDiagnosticResult | null,
): DoctorCheckResult[] {
  const available = result.tools.filter((tool) => tool.state === "available").length;
  const gated = result.tools.filter((tool) => tool.state === "gated").length;
  const hidden = result.tools.filter((tool) => tool.state === "hidden").length;
  const projection: DoctorCheckResult = check(
    "capabilities.projection",
    "capabilities",
    "pass",
    `Tool projection for mode ${result.mode}: ${available} available, ${gated} gated, ${hidden} hidden`,
    result.tools.slice(0, 200).map((tool) => ({
      label: tool.name,
      value: `${tool.state}${tool.reason === null ? "" : ` — ${tool.reason}`}`,
    })),
  );
  const trace: DoctorCheckResult = check(
    "capabilities.trace",
    "capabilities",
    "pass",
    "Capability-resolution trace (diagnostic metadata only; it does not change resolution)",
    result.trace.map((step) => ({ label: step.step, value: step.detail })),
  );
  const task: DoctorCheckResult =
    taskResult === null || !taskResult.activeTask
      ? check(
          "capabilities.task_snapshot",
          "capabilities",
          "skip",
          "No active task; nothing to compare",
        )
      : taskResult.differences.length === 0
        ? check(
            "capabilities.task_snapshot",
            "capabilities",
            "pass",
            "Current task runtime snapshot matches the current global configuration",
          )
        : check(
            "capabilities.task_snapshot",
            "capabilities",
            "warn",
            "Current task runtime snapshot differs from the current global configuration (immutable task semantics; the doctor does not mutate the task)",
            taskResult.differences.map((difference) => ({
              label: difference.field,
              value: `task snapshot: ${difference.snapshotValue ?? "unknown"} — current: ${difference.currentValue ?? "unknown"}`,
            })),
          );
  return [projection, trace, task];
}

const AREA_METHODS: Record<DoctorArea, keyof DoctorSources> = {
  runtime: "runtime",
  configuration: "configuration",
  providers: "providers",
  sandbox: "sandbox",
  workspace: "workspace",
  godot: "godot",
  project: "godot",
  references: "references",
  research: "research",
  capabilities: "capabilities",
};

export function createCapabilityDoctor(
  sources: DoctorSources,
  options: CapabilityDoctorOptions = {},
): CapabilityDoctor {
  const checkTimeoutMs = options.checkTimeoutMs ?? DEFAULT_DOCTOR_CHECK_TIMEOUT_MS;

  async function inspect(request: DoctorRequest, signal?: AbortSignal): Promise<DoctorReport> {
    const areas = normalizeDoctorRequest(request);

    // The report always carries runtime identity; probe it once up front.
    let runtimeResult: RuntimeDiagnosticResult | null = null;
    try {
      runtimeResult = await withTimeout(sources.runtime(), checkTimeoutMs, signal);
    } catch {
      runtimeResult = null;
    }
    const runtime =
      runtimeResult === null
        ? { version: "unknown", nodeMajor: 0, nodeSupported: false, platform: "unknown" }
        : {
            version: runtimeResult.version,
            nodeMajor: runtimeResult.nodeMajor,
            platform: runtimeResult.platform,
          };

    const checks: DoctorCheckResult[] = [];
    const collected = new Map<DoctorArea, unknown>();

    // The capabilities area reports the FULL capability snapshot, so it
    // probes every area; checks are still emitted only for requested areas.
    const probeAreas: readonly DoctorArea[] = areas.includes("capabilities") ? DOCTOR_AREAS : areas;
    for (const area of probeAreas) {
      if (signal !== undefined && signal.aborted) {
        checks.push(check(`${area}.cancelled`, area, "skip", "Check cancelled before it started"));
        continue;
      }
      let outcome: AreaOutcome<unknown>;
      try {
        if (area === "runtime" && runtimeResult !== null) {
          outcome = { kind: "ok", value: runtimeResult };
        } else {
          const probe = (sources[AREA_METHODS[area]] as () => Promise<unknown>)();
          outcome = { kind: "ok", value: await withTimeout(probe, checkTimeoutMs, signal) };
        }
      } catch (error) {
        if (error instanceof DoctorCancelledError || (signal !== undefined && signal.aborted)) {
          outcome = { kind: "cancelled" };
        } else if (error instanceof DoctorTimeoutError) {
          outcome = { kind: "timeout" };
        } else {
          outcome = { kind: "error", message: boundedMessage(error) };
        }
      }
      if (outcome.kind === "timeout") {
        if (areas.includes(area)) {
          checks.push(
            check(
              `${area}.timeout`,
              area,
              "fail",
              `Diagnostic probe for area ${area} timed out after ${checkTimeoutMs}ms`,
            ),
          );
        }
        continue;
      }
      if (outcome.kind === "cancelled") {
        if (areas.includes(area)) {
          checks.push(check(`${area}.cancelled`, area, "skip", "Check cancelled"));
        }
        continue;
      }
      if (outcome.kind === "error") {
        if (areas.includes(area)) {
          checks.push(
            check(
              `${area}.probe_failed`,
              area,
              "fail",
              `Diagnostic probe for area ${area} failed`,
              [{ label: "error", value: outcome.message }],
            ),
          );
        }
        continue;
      }
      collected.set(area, outcome.value);
      if (!areas.includes(area)) {
        continue;
      }
      if (area === "capabilities") {
        let taskResult: TaskSnapshotDiagnosticResult | null = null;
        let taskCheckEmitted = false;
        if (signal !== undefined && signal.aborted) {
          checks.push(
            check("capabilities.task_snapshot", "capabilities", "skip", "Check cancelled"),
          );
          taskCheckEmitted = true;
        } else {
          try {
            const taskProbe = sources.tasks();
            taskResult = await withTimeout(taskProbe, checkTimeoutMs, signal);
          } catch (error) {
            taskCheckEmitted = true;
            if (error instanceof DoctorCancelledError || (signal !== undefined && signal.aborted)) {
              checks.push(
                check("capabilities.task_snapshot", "capabilities", "skip", "Check cancelled"),
              );
            } else if (error instanceof DoctorTimeoutError) {
              checks.push(
                check(
                  "capabilities.task_snapshot",
                  "capabilities",
                  "fail",
                  `Task snapshot probe timed out after ${checkTimeoutMs}ms`,
                ),
              );
            } else {
              checks.push(
                check(
                  "capabilities.task_snapshot",
                  "capabilities",
                  "fail",
                  "Task snapshot probe failed",
                  [{ label: "error", value: boundedMessage(error) }],
                ),
              );
            }
          }
        }
        // When the task probe itself failed, do not emit a second,
        // contradictory task_snapshot check from the capabilities builder.
        const capabilityChecks = capabilitiesChecks(
          outcome.value as CapabilityDiagnosticResult,
          taskCheckEmitted ? null : taskResult,
        );
        checks.push(
          ...(taskCheckEmitted
            ? capabilityChecks.filter((entry) => entry.id !== "capabilities.task_snapshot")
            : capabilityChecks),
        );
      } else {
        checks.push(...buildAreaChecks(area, outcome.value, runtimeResult));
      }
    }

    let snapshot: CapabilitySnapshot | null = null;
    if (areas.includes("capabilities") && runtimeResult !== null) {
      snapshot = tryBuildSnapshot(collected, runtimeResult);
    }

    const report: DoctorReport = {
      schemaVersion: DOCTOR_SCHEMA_VERSION,
      generatedAtMs: Date.now(),
      runtime,
      requestedAreas: areas,
      checks,
      counts: countDoctorReport({ checks }),
      snapshot,
    };
    return report;
  }

  return Object.freeze({ inspect });
}

function tryBuildSnapshot(
  collected: ReadonlyMap<DoctorArea, unknown>,
  runtimeResult: RuntimeDiagnosticResult,
): CapabilitySnapshot | null {
  const get = <T>(area: DoctorArea): T | null =>
    collected.has(area) ? (collected.get(area) as T) : null;
  const providers = get<ProviderDiagnosticResult>("providers");
  const sandbox = get<SandboxDiagnosticResult>("sandbox");
  const workspace = get<WorkspaceDiagnosticResult>("workspace");
  const godot = get<GodotDiagnosticResult>("godot");
  const references = get<ReferenceDiagnosticResult>("references");
  const research = get<ResearchDiagnosticResult>("research");
  const tools = get<CapabilityDiagnosticResult>("capabilities");
  if (
    providers === null ||
    sandbox === null ||
    workspace === null ||
    godot === null ||
    references === null ||
    research === null ||
    tools === null
  ) {
    return null;
  }
  return buildCapabilitySnapshot({
    runtime: runtimeResult,
    providers,
    sandbox,
    workspace,
    godot,
    references,
    research,
    tools,
  });
}

function buildAreaChecks(
  area: DoctorArea,
  value: unknown,
  runtimeResult: RuntimeDiagnosticResult | null,
): DoctorCheckResult[] {
  switch (area) {
    case "runtime":
      return runtimeChecks(runtimeResult ?? (value as RuntimeDiagnosticResult));
    case "configuration":
      return configurationChecks(value as ConfigurationDiagnosticResult);
    case "providers":
      return providerChecks(value as ProviderDiagnosticResult);
    case "sandbox":
      return sandboxChecks(value as SandboxDiagnosticResult);
    case "workspace":
      return workspaceChecks(value as WorkspaceDiagnosticResult);
    case "godot":
    case "project":
      return [
        ...godotChecks(value as GodotDiagnosticResult),
        ...projectChecks(value as GodotDiagnosticResult),
      ];
    case "references":
      return referenceChecks(value as ReferenceDiagnosticResult);
    case "research":
      return researchChecks(value as ResearchDiagnosticResult);
    case "capabilities":
      return capabilitiesChecks(value as CapabilityDiagnosticResult, null);
    default:
      return [];
  }
}
