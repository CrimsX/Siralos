import {
  createAnthropicSandboxRuntimeBackend,
  getDefaultUserConfigPath,
  getSandboxDirectories,
  loadUserConfig,
  removeConformanceArtifacts,
  runSandboxConformance,
  type ConformanceReport,
} from "@solaris/adapters";
import {
  DEVELOP_OFFLINE_PROFILE,
  getBuiltInProfile,
  type SandboxBackendStatus,
} from "@solaris/core";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
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
  const workspaceRoot = options.workspaceRoot ?? process.cwd();
  const sandboxDirectories = getSandboxDirectories();
  const backend = createAnthropicSandboxRuntimeBackend({
    workspaceRoot,
    sandboxHome: sandboxDirectories.home,
    sandboxTemp: sandboxDirectories.temp,
  });
  let conformance: ConformanceReport | null = null;
  let probesRun = false;
  let probeWorkspace: string | undefined;
  try {
    const status = await backend.inspect();
    if (options.includeProbes && status.state === "available") {
      probesRun = true;
      probeWorkspace = await mkdtemp(join(tmpdir(), "solaris-doctor-probes-"));
      conformance = await runSandboxConformance(backend, {
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
    if (probeWorkspace !== undefined) {
      await removeConformanceArtifacts(probeWorkspace).catch(() => {});
    }
  }
}
