import {
  createAnthropicSandboxRuntimeBackend,
  getDefaultUserConfigPath,
  getSandboxDirectories,
  loadUserConfig,
  removeConformanceArtifacts,
  runSandboxConformance,
  resolveWorkspaceRoot,
  type ConformanceReport,
} from "@solaris/adapters";
import {
  DEVELOP_OFFLINE_PROFILE,
  getBuiltInProfile,
  type SandboxBackend,
  type SandboxBackendStatus,
} from "@solaris/core";
import { mkdtemp } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";

export interface SandboxDoctorReport {
  readonly profileId: string;
  readonly backendId: string;
  readonly backendVersion: string;
  readonly platform: string;
  readonly state: SandboxBackendStatus["state"];
  readonly capabilities: SandboxBackendStatus["capabilities"];
  readonly statusMessage: string | null;
  readonly probesRun: boolean;
  readonly conformance: ConformanceReport | null;
}

export interface SandboxDoctorOptions {
  readonly includeProbes: boolean;
  readonly configPath?: string;
  readonly workspaceRoot?: string;
  /**
   * Test seam: constructs the backend for a given workspace root. The
   * default factory builds the real Anthropic Sandbox Runtime backend.
   */
  readonly backendFactory?: (workspaceRoot: string) => SandboxBackend;
  /**
   * Test seam: runs the conformance probes against a backend. The default
   * executes the real probe suite.
   */
  readonly conformanceRunner?: typeof runSandboxConformance;
}

const FAKE_PROBE_SECRETS: Readonly<Record<string, string>> = {
  OPENROUTER_API_KEY: "probe-fake-openrouter-key",
  DEEPSEEK_API_KEY: "probe-fake-deepseek-key",
  GITHUB_TOKEN: "probe-fake-github-token",
};

export async function runSandboxDoctor(
  options: SandboxDoctorOptions,
): Promise<SandboxDoctorReport> {
  const config = await loadUserConfig(options.configPath ?? getDefaultUserConfigPath());
  const profile = getBuiltInProfile(config.sandbox.profile);
  const sandboxDirectories = getSandboxDirectories();
  const runRoot = join(homedir(), ".solaris", "runs");
  let conformance: ConformanceReport | null = null;
  let probesRun = false;
  let probeWorkspace: string | undefined;
  try {
    let workspaceRoot: string;
    if (options.includeProbes) {
      probeWorkspace = await mkdtemp(join(tmpdir(), "solaris-doctor-probes-"));
      workspaceRoot = probeWorkspace;
    } else {
      workspaceRoot = await resolveWorkspaceRoot(options.workspaceRoot ?? process.cwd());
    }
    const backend =
      options.backendFactory === undefined
        ? createAnthropicSandboxRuntimeBackend({
            workspaceRoot,
            sandboxHome: sandboxDirectories.home,
            sandboxTemp: sandboxDirectories.temp,
            runRoot,
          })
        : options.backendFactory(workspaceRoot);
    try {
      const status = await backend.inspect();
      if (options.includeProbes && status.state === "available" && probeWorkspace !== undefined) {
        probesRun = true;
        const runner = options.conformanceRunner ?? runSandboxConformance;
        conformance = await runner(backend, {
          workspaceRoot: probeWorkspace,
          profile: DEVELOP_OFFLINE_PROFILE,
          parentEnvironment: FAKE_PROBE_SECRETS,
        });
      }
      return {
        profileId: profile.id,
        backendId: status.backendId,
        backendVersion: status.version,
        platform: status.platform,
        state: status.state,
        capabilities: status.capabilities,
        statusMessage: status.message ?? null,
        probesRun,
        conformance,
      };
    } finally {
      await backend.close().catch(() => {});
    }
  } finally {
    if (probeWorkspace !== undefined) {
      await removeConformanceArtifacts(probeWorkspace).catch(() => {});
    }
  }
}

/**
 * Doctor exit codes for `--sandbox-doctor`:
 * 0 = probes requested and all passed (or no probes requested),
 * 1 = probes requested, ran, and at least one failed,
 * 3 = probes requested but could not run (backend unavailable, unsupported,
 *     setup required, or inspection failed).
 */
export function doctorExitCode(report: SandboxDoctorReport, includeProbes: boolean): number {
  if (!includeProbes) {
    return 0;
  }
  if (!report.probesRun || report.conformance === null) {
    return 3;
  }
  return report.conformance.failed > 0 ? 1 : 0;
}
