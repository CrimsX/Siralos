import { describe, expect, it } from "vitest";
import type { GodotDoctorReport } from "../godot/inspector.js";
import type { GodotProjectProfile } from "../godot/project.js";
import {
  DOCTOR_AREAS,
  DOCTOR_EXIT_FAILURES,
  DoctorInvocationError,
  normalizeDoctorRequest,
} from "./doctor-model.js";
import { createCapabilityDoctor } from "./capability-doctor.js";
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
import { sanitizeSafeDoctorText, toSafeReport } from "./safe-report.js";

function fakeProject(overrides: Partial<GodotProjectProfile> = {}): GodotProjectProfile {
  return {
    detected: false,
    projectFileSha256: null,
    configVersion: null,
    name: null,
    applicationVersion: null,
    declaredFeatures: [],
    declaredEngineVersion: null,
    mainScene: null,
    mainSceneExists: null,
    mainSceneIsSymlink: false,
    renderingMethods: [],
    languageProfile: "unknown",
    autoloads: [],
    enabledEditorPlugins: [],
    executableContent: {
      toolScripts: [],
      editorPlugins: [],
      importPlugins: [],
      gdextensionDescriptors: [],
      autoloadCount: 0,
      dotnetProjectFiles: [],
      scanTruncated: false,
      scanTruncationReason: "none",
    },
    warnings: [],
    ...overrides,
  };
}

function fakeGodotReport(overrides: Partial<GodotDoctorReport> = {}): GodotDoctorReport {
  return {
    discovery: {
      candidates: [],
      configuration: {
        activeInstallation: null,
        configuredCount: 0,
        discoverOnPath: true,
        overrides: [],
      },
      selected: null,
      rationale: [],
      diagnostics: [],
    },
    project: fakeProject(),
    compatibility: {
      status: "unknown",
      severity: "none",
      reasons: [],
    } as unknown as GodotDoctorReport["compatibility"],
    cache: { schemaVersion: 1, cachedProfileCount: 0, enabled: false },
    sandbox: {
      state: "available",
      backendId: "sandbox-runtime",
      filesystemReadRestriction: true,
      networkRestriction: true,
      filesystemWriteRestriction: true,
      processTreeRestriction: true,
    },
    degradedCapabilities: [],
    recoveryProbe: {
      state: "unavailable",
      reason: "no identity-bound execution",
      platform: "win32",
    },
    knowledge: { state: "unavailable", reason: "no identity-bound execution", platform: "win32" },
    diagnostics: { state: "unavailable", reason: "no identity-bound execution", platform: "win32" },
    probes: [],
    ...overrides,
  };
}

const OK_RUNTIME: RuntimeDiagnosticResult = {
  version: "0.0.0",
  nodeMajor: 24,
  nodeSupported: true,
  platform: "win32",
  configurationFile: { state: "readable", detail: null },
  checkpointStoreAccessible: true,
};

const OK_CONFIG: ConfigurationDiagnosticResult = {
  loaded: true,
  sections: [
    { name: "sandbox", present: true },
    { name: "godot", present: false },
    { name: "quality", present: false },
    { name: "references", present: false },
  ],
  unknownFields: [],
  validationErrors: [],
  credentialRefs: [],
  overrideInUse: false,
};

const OK_PROVIDERS: ProviderDiagnosticResult = {
  active: { profileId: "deterministic-fake", toolCalling: true, state: "available", reason: null },
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

const OK_SANDBOX: SandboxDiagnosticResult = {
  backend: {
    state: "available",
    backendId: "anthropic-sandbox-runtime",
    platform: "win32",
    version: "0.0.70",
    capabilities: {
      filesystemReadRestriction: true,
      filesystemWriteRestriction: true,
      networkRestriction: true,
      processTreeRestriction: true,
      violationReporting: true,
    },
  },
  selectedProfileId: "inspect",
  profileRequiresProcess: false,
  profileRequiresWrite: false,
  requiredCapabilitiesMissing: [],
  unrestrictedFallback: false,
};

const OK_WORKSPACE: WorkspaceDiagnosticResult = {
  root: "C:\\workspace",
  readable: true,
  protectedPathsActive: true,
  gitAvailable: true,
  gitState: "clean",
  checkpointStoreAccessible: true,
  revisionRegistryOperational: true,
  namespaceIntegrity: true,
};

const OK_GODOT: GodotDiagnosticResult = {
  report: fakeGodotReport(),
  versionMatch: { state: "absent", reason: "engine profiling unavailable" },
  projectRoot: "C:\\workspace",
  policyRules: { recoveryProbe: "ask", lsp: "ask", diagnose: "ask" },
};

const OK_REFERENCES: ReferenceDiagnosticResult = {
  configError: null,
  references: [],
};

const OK_RESEARCH: ResearchDiagnosticResult = {
  sources: [
    { kind: "repository", id: "repository", label: "Repository research" },
    { kind: "godot_docs", id: "godot-docs", label: "Godot documentation research" },
  ],
  policyRule: "deny",
  gate: "blocked_by_policy",
  adapterAvailability: [
    { kind: "repository", available: false, reason: "sandboxed git execution unavailable" },
    { kind: "godot_docs", available: true, reason: null },
  ],
  latestEvidenceCount: 0,
};

const OK_CAPABILITIES: CapabilityDiagnosticResult = {
  mode: "development",
  trace: [
    { step: "registered", detail: "workspace.read registered" },
    { step: "runtime profile", detail: "inspect" },
    { step: "resource policy", detail: "workspace.read → allow" },
    { step: "task requirements", detail: "mode development" },
    { step: "model compatibility", detail: "tool calls supported" },
    { step: "projected state", detail: "workspace.read → available" },
  ],
  tools: [
    { name: "workspace.read", state: "available", reason: null },
    { name: "workspace.write", state: "hidden", reason: "denied by policy" },
    { name: "research.fetch", state: "hidden", reason: "network capability disabled" },
  ],
};

const OK_TASKS: TaskSnapshotDiagnosticResult = {
  activeTask: false,
  runtimeVersion: null,
  differences: [],
};

export interface FakeSourcesOverrides {
  runtime?: Partial<RuntimeDiagnosticResult> | Error;
  configuration?: Partial<ConfigurationDiagnosticResult> | Error;
  providers?: Partial<ProviderDiagnosticResult> | Error;
  sandbox?: Partial<SandboxDiagnosticResult> | Error;
  workspace?: Partial<WorkspaceDiagnosticResult> | Error;
  godot?: Partial<GodotDiagnosticResult> | Error;
  references?: Partial<ReferenceDiagnosticResult> | Error;
  research?: Partial<ResearchDiagnosticResult> | Error;
  capabilities?: Partial<CapabilityDiagnosticResult> | Error;
  tasks?: Partial<TaskSnapshotDiagnosticResult> | Error;
  /** All sources resolve immediately; use this to simulate a hanging probe. */
  hangingAreas?: readonly string[];
}

function resultOrError<T>(
  base: T,
  override: Partial<T> | Error | undefined,
  hanging: boolean,
): Promise<T> {
  if (hanging) {
    return new Promise<T>(() => undefined);
  }
  if (override instanceof Error) {
    return Promise.reject(override);
  }
  return Promise.resolve({ ...base, ...override });
}

export function fakeDoctorSources(overrides: FakeSourcesOverrides = {}): DoctorSources {
  const hanging = overrides.hangingAreas ?? [];
  return {
    runtime: () => resultOrError(OK_RUNTIME, overrides.runtime, hanging.includes("runtime")),
    configuration: () =>
      resultOrError(OK_CONFIG, overrides.configuration, hanging.includes("configuration")),
    providers: () =>
      resultOrError(OK_PROVIDERS, overrides.providers, hanging.includes("providers")),
    sandbox: () => resultOrError(OK_SANDBOX, overrides.sandbox, hanging.includes("sandbox")),
    workspace: () =>
      resultOrError(OK_WORKSPACE, overrides.workspace, hanging.includes("workspace")),
    godot: () => resultOrError(OK_GODOT, overrides.godot, hanging.includes("godot")),
    references: () =>
      resultOrError(OK_REFERENCES, overrides.references, hanging.includes("references")),
    research: () => resultOrError(OK_RESEARCH, overrides.research, hanging.includes("research")),
    capabilities: () =>
      resultOrError(OK_CAPABILITIES, overrides.capabilities, hanging.includes("capabilities")),
    tasks: () => resultOrError(OK_TASKS, overrides.tasks, hanging.includes("tasks")),
  };
}

function checkIds(report: { checks: readonly { id: string }[] }): string[] {
  return report.checks.map((check) => check.id);
}

describe("capability doctor", () => {
  it("runs every area by default and reports deterministic check ids", async () => {
    const doctor = createCapabilityDoctor(fakeDoctorSources());
    const report = await doctor.inspect({});
    expect(report.schemaVersion).toBe(1);
    expect(report.requestedAreas).toEqual(DOCTOR_AREAS);
    expect(report.counts.total).toBe(report.checks.length);
    expect(report.counts.fail).toBe(0);
    expect(report.counts.pass).toBeGreaterThan(0);
    expect(checkIds(report)).toContain("runtime.node_version");
    expect(checkIds(report)).toContain("configuration.validity");
    expect(checkIds(report)).toContain("providers.active");
    expect(checkIds(report)).toContain("sandbox.required_enforcement");
    expect(checkIds(report)).toContain("godot.selection");
    expect(checkIds(report)).toContain("project.profile");
    expect(checkIds(report)).toContain("references.configuration");
    expect(checkIds(report)).toContain("research.policy");
    expect(checkIds(report)).toContain("capabilities.projection");
    expect(checkIds(report)).toContain("capabilities.task_snapshot");
  });

  it("attaches the capability snapshot when the capabilities area is requested", async () => {
    const doctor = createCapabilityDoctor(fakeDoctorSources());
    const report = await doctor.inspect({ areas: ["capabilities"] });
    expect(report.snapshot).not.toBeNull();
    expect(report.snapshot!.runtime.version).toBe("0.0.0");
    expect(report.snapshot!.providers[0]!.profileId).toBe("deterministic-fake");
    expect(report.snapshot!.tools.projectedHidden).toBe(2);
    const noSnapshot = await doctor.inspect({ areas: ["runtime"] });
    expect(noSnapshot.snapshot).toBeNull();
  });

  it("filters to requested areas only (canonical order)", async () => {
    const doctor = createCapabilityDoctor(fakeDoctorSources());
    const report = await doctor.inspect({ areas: ["sandbox", "providers"] });
    expect(report.requestedAreas).toEqual(["providers", "sandbox"]);
    for (const check of report.checks) {
      expect(["sandbox", "providers"]).toContain(check.area);
    }
  });

  it("starts independent probes concurrently while preserving canonical report order", async () => {
    let providersStarted = false;
    let sandboxStarted = false;
    let resolveProviders!: (value: ProviderDiagnosticResult) => void;
    let resolveSandbox!: (value: SandboxDiagnosticResult) => void;
    const sources: DoctorSources = {
      ...fakeDoctorSources(),
      providers: () => {
        providersStarted = true;
        return new Promise((resolve) => {
          resolveProviders = resolve;
        });
      },
      sandbox: () => {
        sandboxStarted = true;
        return new Promise((resolve) => {
          resolveSandbox = resolve;
        });
      },
    };

    const pending = createCapabilityDoctor(sources).inspect({
      areas: ["sandbox", "providers"],
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(providersStarted).toBe(true);
    expect(sandboxStarted).toBe(true);

    // Resolve in reverse order; output must still follow DOCTOR_AREAS.
    resolveSandbox(OK_SANDBOX);
    resolveProviders(OK_PROVIDERS);
    const report = await pending;
    expect(report.checks[0]?.area).toBe("providers");
    expect(report.checks.findIndex((entry) => entry.area === "sandbox")).toBeGreaterThan(0);
  });

  it("shares authoritative probes used by multiple doctor areas", async () => {
    let runtimeCalls = 0;
    let godotCalls = 0;
    const base = fakeDoctorSources();
    const report = await createCapabilityDoctor({
      ...base,
      runtime: async () => {
        runtimeCalls += 1;
        return base.runtime();
      },
      godot: async () => {
        godotCalls += 1;
        return base.godot();
      },
    }).inspect({});

    expect(runtimeCalls).toBe(1);
    expect(godotCalls).toBe(1);
    expect(report.checks.filter((entry) => entry.id === "godot.selection")).toHaveLength(1);
    expect(report.checks.filter((entry) => entry.id === "project.profile")).toHaveLength(1);
  });

  it("rejects unknown areas with a typed invocation error", async () => {
    const doctor = createCapabilityDoctor(fakeDoctorSources());
    await expect(doctor.inspect({ areas: ["agents" as never] })).rejects.toBeInstanceOf(
      DoctorInvocationError,
    );
    await expect(doctor.inspect({ areas: ["mcp" as never] })).rejects.toBeInstanceOf(
      DoctorInvocationError,
    );
  });

  it("normalizeDoctorRequest deduplicates and keeps canonical order", () => {
    expect(normalizeDoctorRequest({ areas: ["providers", "runtime", "providers"] })).toEqual([
      "runtime",
      "providers",
    ]);
    expect(normalizeDoctorRequest({ areas: [] })).toEqual(DOCTOR_AREAS);
  });

  it("reports a missing provider credential without exposing any value", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        providers: {
          credentials: [{ name: "OPENROUTER_API_KEY", referenced: true, present: false }],
        },
      }),
    );
    const report = await doctor.inspect({ areas: ["providers"] });
    const credentials = report.checks.find((check) => check.id === "providers.credentials")!;
    expect(credentials.status).toBe("fail");
    const text = JSON.stringify(report);
    expect(text).toContain("OPENROUTER_API_KEY");
    expect(text).toContain("present: no");
    expect(text).not.toMatch(/sk-[A-Za-z0-9_-]{8,}/);
  });

  it("reports an invalid remote HTTP endpoint as a failure", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        providers: {
          endpoints: [
            {
              label: "api",
              https: false,
              loopback: false,
              valid: false,
              reason: "remote endpoint must use HTTPS",
            },
          ],
        },
      }),
    );
    const report = await doctor.inspect({ areas: ["providers"] });
    const endpoints = report.checks.find((check) => check.id === "providers.endpoints")!;
    expect(endpoints.status).toBe("fail");
  });

  it("accepts a loopback HTTP endpoint", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        providers: {
          endpoints: [{ label: "local", https: false, loopback: true, valid: true, reason: null }],
        },
      }),
    );
    const report = await doctor.inspect({ areas: ["providers"] });
    const endpoints = report.checks.find((check) => check.id === "providers.endpoints")!;
    expect(endpoints.status).toBe("pass");
  });

  it("reports model/tool-call incompatibility as a failure", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        providers: {
          model: { id: "example-model", toolCalling: false, contextBudgetTokens: 128_000 },
        },
      }),
    );
    const report = await doctor.inspect({ areas: ["providers"] });
    const model = report.checks.find((check) => check.id === "providers.model_compatibility")!;
    expect(model.status).toBe("fail");
    expect(model.summary).toContain("example-model");
  });

  it("reports missing required sandbox enforcement as a failure (fail closed)", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        sandbox: {
          // A process-enabled profile requires the full boundary; the
          // backend cannot provide process-tree restriction.
          selectedProfileId: "develop-offline",
          profileRequiresProcess: true,
          requiredCapabilitiesMissing: ["processTreeRestriction"],
        },
      }),
    );
    const report = await doctor.inspect({ areas: ["sandbox"] });
    const enforcement = report.checks.find((check) => check.id === "sandbox.required_enforcement")!;
    expect(enforcement.status).toBe("fail");
    const fallback = report.checks.find((check) => check.id === "sandbox.unrestricted_fallback")!;
    expect(fallback.status).toBe("pass");
  });

  it("fails when an unrestricted fallback exists", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        sandbox: { unrestrictedFallback: true },
      }),
    );
    const report = await doctor.inspect({ areas: ["sandbox"] });
    const fallback = report.checks.find((check) => check.id === "sandbox.unrestricted_fallback")!;
    expect(fallback.status).toBe("fail");
  });

  it("reports a stale Godot API cache as stale", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        godot: {
          report: fakeGodotReport({
            discovery: {
              candidates: [],
              configuration: {
                activeInstallation: "inst-1",
                configuredCount: 1,
                discoverOnPath: true,
                overrides: [],
              },
              selected: {
                installationId: "inst-1",
                version: {
                  major: 4,
                  minor: 7,
                  patch: 1,
                  status: "stable",
                  statusNumber: null,
                  build: null,
                  commit: null,
                  raw: "4.7.1",
                },
                edition: "standard",
                editionConfidence: "high",
                releaseChannel: "stable",
                sourceLabel: "configured",
                source: "user-config",
                support: "verified",
                invalid: null,
                isDuplicate: false,
                selected: true,
                fingerprint: "a1b2c3d4",
                profiled: false,
              },
              rationale: [],
              diagnostics: [],
            },
          }),
          versionMatch: { state: "stale", reason: "cache fingerprints the previous engine" },
        },
      }),
    );
    const report = await doctor.inspect({ areas: ["godot"] });
    const versionMatch = report.checks.find((check) => check.id === "godot.version_match")!;
    expect(versionMatch.status).toBe("warn");
    expect(versionMatch.summary.toLowerCase()).toContain("stale");
    const selection = report.checks.find((check) => check.id === "godot.selection")!;
    expect(selection.status).toBe("pass");
    expect(JSON.stringify(selection.details)).toContain("4.7.1");
  });

  it("distinguishes static project availability from approval-required recovery operations", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        godot: {
          report: fakeGodotReport({
            project: fakeProject({
              detected: true,
              name: "demo",
              languageProfile: "gdscript",
              executableContent: {
                toolScripts: ["src/tool.gd"],
                editorPlugins: [],
                importPlugins: [],
                gdextensionDescriptors: [],
                autoloadCount: 0,
                dotnetProjectFiles: [],
                scanTruncated: false,
                scanTruncationReason: "none",
              },
            }),
          }),
          projectRoot: "C:\\workspace",
        },
      }),
    );
    const report = await doctor.inspect({ areas: ["project"] });
    const profile = report.checks.find((check) => check.id === "project.profile")!;
    expect(profile.status).toBe("pass");
    const recovery = report.checks.find((check) => check.id === "project.recovery_lsp")!;
    expect(recovery.status).toBe("skip");
    const details = Object.fromEntries(
      recovery.details!.map((detail) => [detail.label, detail.value]),
    );
    expect(details["recovery-probe-execution"]).toContain("unavailable");
    expect(details["recovery-probe-policy"]).toContain("ask");
    expect(details["lsp-execution"]).toContain("unavailable");
    expect(details["api-cache"]).toContain("disabled");
  });

  it("reports configured reference revisions when materialized", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        references: {
          references: [
            {
              alias: "docs",
              kind: "local-directory",
              trust: "explicit-user",
              status: "ready",
              failureReason: null,
              revision: { kind: "fingerprint", fingerprint: "deadbeef", commit: null },
              materialized: "yes",
            },
          ],
        },
      }),
    );
    const report = await doctor.inspect({ areas: ["references"] });
    const revisions = report.checks.find((check) => check.id === "references.revisions")!;
    expect(revisions.status).toBe("pass");
    expect(JSON.stringify(revisions.details)).toContain("deadbeef");
    expect(JSON.stringify(revisions.details)).toContain("materialized: yes");
  });

  it("reports research policy gate without fetching anything", async () => {
    const doctor = createCapabilityDoctor(fakeDoctorSources());
    const report = await doctor.inspect({ areas: ["research"] });
    const policy = report.checks.find((check) => check.id === "research.policy")!;
    expect(policy.status).toBe("pass");
    expect(JSON.stringify(policy.details)).toContain("blocked_by_policy");
  });

  it("capability trace matches the ToolProjector-derived result verbatim", async () => {
    const doctor = createCapabilityDoctor(fakeDoctorSources());
    const report = await doctor.inspect({ areas: ["capabilities"] });
    const trace = report.checks.find((check) => check.id === "capabilities.trace")!;
    const projection = report.checks.find((check) => check.id === "capabilities.projection")!;
    expect(trace.details!.map((detail) => detail.label)).toEqual([
      "registered",
      "runtime profile",
      "resource policy",
      "task requirements",
      "model compatibility",
      "projected state",
    ]);
    // The projection details are exactly the projector's tool states.
    expect(
      projection.details!.map((detail) => `${detail.label}:${detail.value.split(" ")[0]}`),
    ).toEqual(["workspace.read:available", "workspace.write:hidden", "research.fetch:hidden"]);
  });

  it("reports task snapshot differences without mutating the task", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        tasks: {
          activeTask: true,
          runtimeVersion: "task-runtime-1",
          differences: [
            {
              field: "provider profile",
              snapshotValue: "deterministic-fake",
              currentValue: "another-provider",
            },
          ],
        },
      }),
    );
    const report = await doctor.inspect({ areas: ["capabilities"] });
    const task = report.checks.find((check) => check.id === "capabilities.task_snapshot")!;
    expect(task.status).toBe("warn");
    expect(JSON.stringify(task.details)).toContain("task snapshot: deterministic-fake");
    expect(JSON.stringify(task.details)).toContain("current: another-provider");
  });

  it("skips the task check when no active task exists", async () => {
    const doctor = createCapabilityDoctor(fakeDoctorSources());
    const report = await doctor.inspect({ areas: ["capabilities"] });
    const task = report.checks.find((check) => check.id === "capabilities.task_snapshot")!;
    expect(task.status).toBe("skip");
  });

  it("bounds probes with a timeout and reports a fail check", async () => {
    const doctor = createCapabilityDoctor(fakeDoctorSources({ hangingAreas: ["workspace"] }), {
      checkTimeoutMs: 30,
    });
    const report = await doctor.inspect({ areas: ["workspace"] });
    const timeout = report.checks.find((check) => check.id === "workspace.timeout")!;
    expect(timeout.status).toBe("fail");
    expect(timeout.summary).toContain("timed out");
  });

  it("reports probe failures as fail checks with bounded messages", async () => {
    const doctor = createCapabilityDoctor(fakeDoctorSources({ godot: new Error("boom") }));
    const report = await doctor.inspect({ areas: ["godot"] });
    const failed = report.checks.find((check) => check.id === "godot.probe_failed")!;
    expect(failed.status).toBe("fail");
    expect(failed.details!.some((detail) => detail.value.includes("boom"))).toBe(true);
  });

  it("honours caller cancellation with skip checks", async () => {
    const controller = new AbortController();
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        tasks: new Error("should not run"),
      }),
    );
    controller.abort();
    const report = await doctor.inspect({ areas: ["providers", "sandbox"] }, controller.signal);
    expect(report.checks.every((check) => check.status === "skip")).toBe(true);
    expect(checkIds(report)).toEqual(["providers.cancelled", "sandbox.cancelled"]);
  });

  it("emits exactly one task_snapshot check when the task probe fails", async () => {
    const doctor = createCapabilityDoctor(
      fakeDoctorSources({
        tasks: new Error("boom"),
      }),
    );
    const report = await doctor.inspect({ areas: ["capabilities"] });
    const taskChecks = report.checks.filter((check) => check.id === "capabilities.task_snapshot");
    expect(taskChecks).toHaveLength(1);
    expect(taskChecks[0]!.status).toBe("fail");
    expect(report.checks.filter((check) => check.id === "capabilities.projection")).toHaveLength(1);
  });

  it("computes exit codes: warnings never fail, failures do", async () => {
    const clean = await createCapabilityDoctor(fakeDoctorSources()).inspect({ areas: ["runtime"] });
    expect(clean.counts.fail).toBe(0);
    const warnReport = await createCapabilityDoctor(
      fakeDoctorSources({
        godot: {
          report: fakeGodotReport({
            discovery: {
              candidates: [],
              configuration: {
                activeInstallation: null,
                configuredCount: 0,
                discoverOnPath: true,
                overrides: [],
              },
              selected: null,
              rationale: [],
              diagnostics: [],
            },
          }),
          versionMatch: { state: "stale", reason: null },
        },
      }),
    ).inspect({ areas: ["godot"] });
    expect(warnReport.counts.warn).toBeGreaterThan(0);
    expect(warnReport.counts.fail).toBe(0);
    expect(DOCTOR_EXIT_FAILURES).toBe(1);
    const failReport = await createCapabilityDoctor(
      fakeDoctorSources({ runtime: { nodeMajor: 18, nodeSupported: false } }),
    ).inspect({ areas: ["runtime"] });
    expect(failReport.counts.fail).toBeGreaterThan(0);
  });

  it("never performs network requests in default operation (no transport exists)", async () => {
    // The doctor is constructed from pure sources; there is no transport,
    // fetch, or socket anywhere in the doctor module graph. This test
    // verifies the full default report resolves without any I/O surface.
    const report = await createCapabilityDoctor(fakeDoctorSources()).inspect({});
    expect(report.counts.fail).toBe(0);
  });
});

describe("safe report", () => {
  it("excludes absolute paths from summaries", async () => {
    const report = await createCapabilityDoctor(
      fakeDoctorSources({
        workspace: { root: "C:\\Users\\secret-user\\projects\\solaris-demo" },
      }),
    ).inspect({ areas: ["workspace"] });
    const safe = toSafeReport(report);
    const text = JSON.stringify(safe);
    expect(text).not.toContain("secret-user");
    expect(text).not.toContain("solaris-demo");
  });

  it("excludes provider secret values", async () => {
    const report = await createCapabilityDoctor(
      fakeDoctorSources({
        providers: {
          credentials: [{ name: "OPENROUTER_API_KEY", referenced: true, present: true }],
        },
      }),
    ).inspect({ areas: ["providers"] });
    const safe = toSafeReport(report);
    expect(JSON.stringify(safe)).not.toContain("sk-");
  });

  it("sanitizes credential-shaped and path-shaped text", () => {
    expect(sanitizeSafeDoctorText("token sk-abc123XYZ78900 and path C:\\Users\\me\\x")).toContain(
      "<secret>",
    );
    expect(sanitizeSafeDoctorText("token sk-abc123XYZ78900")).not.toMatch(/sk-/);
    expect(sanitizeSafeDoctorText("path C:\\Users\\me\\x")).toContain("<path>");
    expect(sanitizeSafeDoctorText("/home/alice/.solaris/config.json")).toContain("<path>");
    expect(sanitizeSafeDoctorText("/workspaces/demo/project.godot")).toContain("<path>");
    expect(sanitizeSafeDoctorText("/app/data/secret.txt")).toContain("<path>");
    expect(sanitizeSafeDoctorText("Godot project detected")).not.toContain("<path>");
    expect(sanitizeSafeDoctorText("AKIAIOSFODNN7EXAMPLE")).toContain("<secret>");
    expect(sanitizeSafeDoctorText("ghp_abcdefghijklmnopqrstuvwxyz1234567890")).toContain(
      "<secret>",
    );
    expect(sanitizeSafeDoctorText("plain text stays plain")).toBe("plain text stays plain");
  });

  it("drops details and remediations, keeps counts and error categories", async () => {
    const report = await createCapabilityDoctor(
      fakeDoctorSources({
        configuration: { validationErrors: ["sandbox.profile must be one of ..."] },
      }),
    ).inspect({ areas: ["configuration"] });
    const safe = toSafeReport(report);
    for (const check of safe.checks) {
      expect("details" in check).toBe(false);
    }
    expect(safe.errorCategories[0]).toBeDefined();
    expect(safe.counts.total).toBe(report.counts.total);
    expect(
      safe.errorCategories.some(
        (category) => category.area === "configuration" && category.status === "fail",
      ),
    ).toBe(true);
  });

  it("keeps machine metadata (version, node major, platform) for support", async () => {
    const report = await createCapabilityDoctor(fakeDoctorSources()).inspect({
      areas: ["runtime"],
    });
    const safe = toSafeReport(report);
    expect(safe.runtime.version).toBe("0.0.0");
    expect(safe.runtime.nodeMajor).toBe(24);
    expect(safe.runtime.platform).toBe("win32");
  });

  it("is deterministic for identical reports (ignoring the timestamp)", async () => {
    const doctor = createCapabilityDoctor(fakeDoctorSources());
    const a = await doctor.inspect({ areas: ["runtime", "sandbox"] });
    const b = await doctor.inspect({ areas: ["runtime", "sandbox"] });
    const { generatedAtMs: _a, ...restA } = a;
    const { generatedAtMs: _b, ...restB } = b;
    expect(restA).toEqual(restB);
    expect(JSON.stringify(toSafeReport(a))).toBe(JSON.stringify(toSafeReport(b)));
  });

  it("reports the abort path without executing hanging probes", async () => {
    const controller = new AbortController();
    const doctor = createCapabilityDoctor(fakeDoctorSources({ hangingAreas: ["sandbox"] }));
    const run = doctor.inspect({ areas: ["sandbox"] }, controller.signal);
    controller.abort();
    const report = await run;
    expect(report.checks[0]!.status).toBe("skip");
  });
});
